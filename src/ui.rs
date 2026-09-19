//! Interface : un overlay GTK4 façon rofi/fuzzel, posé en couche `overlay` par
//! layer-shell.
//!
//! La liste est virtualisée (`GtkListView` + `GtkSingleSelection`) : seules les
//! lignes visibles créent des widgets, donc 458 entrées ne coûtent pas plus
//! cher à afficher qu'une dizaine.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::apply;
use crate::config::Config;
use crate::favorites::Favorites;
use crate::model::{sort_for_display, Wallpaper, TYPE_CYCLE};
use crate::scan;
use crate::thumbs;

// ---------------------------------------------------------------------------
// Communication inter-threads
// ---------------------------------------------------------------------------

thread_local! {
    /// Instance courante de l'interface, accessible uniquement depuis le thread
    /// GTK. Les threads de travail passent par [`on_main`] pour y accéder, ce
    /// qui évite d'avoir à rendre `UiState` `Send`.
    static CURRENT: RefCell<Option<Rc<UiState>>> = const { RefCell::new(None) };
}

pub fn set_current(ui: Rc<UiState>) {
    CURRENT.with(|c| *c.borrow_mut() = Some(ui));
}

pub fn current<R>(f: impl FnOnce(&Rc<UiState>) -> R) -> Option<R> {
    CURRENT.with(|c| c.borrow().as_ref().map(f))
}

/// Exécute `f` sur la boucle principale GTK, depuis n'importe quel thread.
pub fn on_main<F: FnOnce() + Send + 'static>(f: F) {
    glib::MainContext::default().invoke(f);
}

// ---------------------------------------------------------------------------
// Modèle de lignes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Header { text: String },
    Item(usize),
}

/// Thème intégré, remplaçable par `~/.config/wallpaper-selector/style.css`.
const DEFAULT_CSS: &str = include_str!("style.css");

/// Recherche analysée : un texte libre, plus des filtres de type.
///
/// Les jetons préfixés par `#` sont des filtres de type et ne participent pas
/// à la recherche textuelle : `#png montagne` cherche « montagne » parmi les
/// PNG. Plusieurs filtres se cumulent en OU (`#png #jpg`).
pub struct Query {
    pub text: String,
    pub types: Vec<String>,
}

impl Query {
    pub fn parse(raw: &str) -> Self {
        let mut words: Vec<&str> = Vec::new();
        let mut types = Vec::new();
        for token in raw.split_whitespace() {
            match token.strip_prefix('#') {
                Some(t) if !t.is_empty() => types.push(t.to_ascii_lowercase()),
                _ => words.push(token),
            }
        }
        Query {
            text: words.join(" ").to_ascii_lowercase(),
            types,
        }
    }

    pub fn matches(&self, w: &Wallpaper) -> bool {
        let by_text = self.text.is_empty()
            || w.title.to_lowercase().contains(&self.text)
            || w.id.to_lowercase().contains(&self.text);
        let by_type = self.types.is_empty() || self.types.iter().any(|t| w.matches_type(t));
        by_text && by_type
    }

    pub fn type_label(&self) -> Option<String> {
        if self.types.is_empty() {
            None
        } else {
            Some(self.types.join(", "))
        }
    }
}

/// Construit les lignes affichées : section « Favoris » en tête, puis le reste.
///
/// `all` est déjà trié favoris d'abord, donc chaque groupe est contigu.
pub fn build_rows(all: &[Wallpaper], query: &str) -> Vec<Row> {
    let q = Query::parse(query);
    let mut rows = Vec::with_capacity(all.len() + 2);

    let favs: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, w)| w.favorite && q.matches(w))
        .map(|(i, _)| i)
        .collect();
    let rest: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.favorite && q.matches(w))
        .map(|(i, _)| i)
        .collect();

    if !favs.is_empty() {
        rows.push(Row::Header {
            text: format!("FAVORIS ({})", favs.len()),
        });
        rows.extend(favs.into_iter().map(Row::Item));
    }
    if !rest.is_empty() {
        rows.push(Row::Header {
            text: format!("TOUS LES FONDS D'ÉCRAN ({})", rest.len()),
        });
        rows.extend(rest.into_iter().map(Row::Item));
    }
    rows
}

// ---------------------------------------------------------------------------
// État de l'interface
// ---------------------------------------------------------------------------

pub struct UiState {
    pub cfg: Config,
    pub all: Rc<RefCell<Vec<Wallpaper>>>,
    pub favs: Rc<RefCell<Favorites>>,
    pub current: Rc<RefCell<Option<PathBuf>>>,
    rows: RefCell<Vec<Row>>,
    gen: thumbs::GenerationState,
    /// Vignettes déjà décodées, gardées en mémoire.
    ///
    /// C'est le second pilier de l'instantanéité : 458 vignettes à la taille
    /// d'affichage pèsent ~7 Mo, et les garder décodées évite de relire et
    /// redécoder un PNG à chaque liaison de ligne.
    textures: Rc<RefCell<std::collections::HashMap<String, gdk::Texture>>>,
    warm_cursor: std::cell::Cell<usize>,


    app: gtk::Application,
    window: gtk::ApplicationWindow,
    entry: gtk::Entry,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    list_view: gtk::ListView,
    status: gtk::Label,
}

/// Récupère le n-ième enfant d'un widget (la structure des lignes est fixe).
fn nth_child<W: IsA<gtk::Widget>>(parent: &W, n: i32) -> Option<gtk::Widget> {
    let mut w = parent.first_child();
    let mut i = 0;
    while i < n {
        w = w.and_then(|x| x.next_sibling());
        i += 1;
    }
    w
}

impl UiState {
    pub fn build(
        app: &gtk::Application,
        cfg: Config,
        items: Vec<Wallpaper>,
        favs: Favorites,
    ) -> Rc<Self> {
        let all = Rc::new(RefCell::new(items));
        let favs = Rc::new(RefCell::new(favs));
        let current = Rc::new(RefCell::new(read_current(&cfg)));
        let textures: Rc<RefCell<std::collections::HashMap<String, gdk::Texture>>> =
            Rc::new(RefCell::new(std::collections::HashMap::new()));

        load_css(&cfg);

        // --- Fenêtre ------------------------------------------------------
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Wallpapers")
            .decorated(false)
            .build();
        window.add_css_class("ws-window");
        setup_layer_shell(&window, &cfg);

        // --- Structure ----------------------------------------------------
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("ws-card");

        let input_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        input_bar.add_css_class("ws-input");
        let prompt = gtk::Label::new(Some("Wallpapers"));
        prompt.add_css_class("ws-prompt");
        let entry = gtk::Entry::new();
        entry.add_css_class("ws-entry");
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("Rechercher un fond d'écran…"));
        input_bar.append(&prompt);
        input_bar.append(&entry);

        // --- Liste virtualisée --------------------------------------------
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(false);
        selection.set_selected(u32::MAX);

        let factory = gtk::SignalListItemFactory::new();
        let all_for_factory = all.clone();
        let current_for_factory = current.clone();
        let textures_for_factory = textures.clone();
        let thumb_px = cfg.thumb_px;
        let thumbs_dir = cfg.thumbs_dir.clone();

        factory.connect_setup(move |_, obj| {
            let Some(item) = obj.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            root.add_css_class("ws-row");

            let wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            wrap.add_css_class("ws-thumbwrap");
            wrap.set_overflow(gtk::Overflow::Hidden);
            wrap.set_size_request(thumb_px, thumb_px);
            let thumb = gtk::Image::new();
            thumb.set_pixel_size(thumb_px);
            wrap.append(&thumb);

            let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
            texts.set_valign(gtk::Align::Center);
            texts.set_hexpand(true);
            let title = gtk::Label::new(None);
            title.add_css_class("ws-title");
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let sub = gtk::Label::new(None);
            sub.add_css_class("ws-sub");
            sub.set_xalign(0.0);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            texts.append(&title);
            texts.append(&sub);

            let star = gtk::Label::new(None);
            star.add_css_class("ws-star");

            root.append(&wrap);
            root.append(&texts);
            root.append(&star);
            item.set_child(Some(&root));
        });

        factory.connect_bind(move |_, obj| {
            let Some(item) = obj.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(root) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(row_obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let row = row_obj.borrow::<Row>();

            let wrap = nth_child(&root, 0);
            let texts = nth_child(&root, 1).and_downcast::<gtk::Box>();
            let star = nth_child(&root, 2).and_downcast::<gtk::Label>();

            let title = texts
                .as_ref()
                .and_then(|t| nth_child(t, 0))
                .and_downcast::<gtk::Label>();
            let sub = texts
                .as_ref()
                .and_then(|t| nth_child(t, 1))
                .and_downcast::<gtk::Label>();

            match &*row {
                Row::Header { text } => {
                    root.remove_css_class("ws-row");
                    root.add_css_class("ws-header");
                    if let Some(w) = &wrap {
                        w.set_visible(false);
                    }
                    if let Some(t) = &title {
                        t.set_text(text);
                        t.set_visible(true);
                    }
                    if let Some(s) = &sub {
                        s.set_visible(false);
                    }
                    if let Some(s) = &star {
                        s.set_visible(false);
                    }
                }
                Row::Item(i) => {
                    root.remove_css_class("ws-header");
                    root.add_css_class("ws-row");
                    let all = all_for_factory.borrow();
                    let Some(w) = all.get(*i) else {
                        return;
                    };

                    if let Some(wrap) = &wrap {
                        wrap.set_visible(true);
                        if let Some(img) = nth_child(wrap, 0).and_downcast::<gtk::Image>() {
                            match thumb_texture(&textures_for_factory, &thumbs_dir, thumb_px, w) {
                                Some(tex) => img.set_paintable(Some(&tex)),
                                None => img.set_icon_name(Some("image-missing")),
                            }
                        }
                    }
                    if let Some(t) = &title {
                        t.set_visible(true);
                        t.set_text(&w.title);
                    }
                    if let Some(s) = &sub {
                        s.set_visible(true);
                        let applied = current_for_factory
                            .borrow()
                            .as_ref()
                            .is_some_and(|p| *p == w.path || w.media.as_deref() == Some(p.as_path()));
                        s.set_text(&format!(
                            "{}{}",
                            if applied { "● " } else { "" },
                            w.subtitle()
                        ));
                    }
                    if let Some(s) = &star {
                        s.set_visible(true);
                        s.set_text(if w.favorite { "★" } else { "" });
                    }
                }
            }
        });

        let list_view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(false)
            .show_separators(false)
            .build();
        list_view.add_css_class("ws-list");

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&list_view)
            .build();
        scrolled.add_css_class("ws-scroll");

        // --- Pied ---------------------------------------------------------
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        footer.add_css_class("ws-footer");
        let hints = gtk::Label::new(Some(
            "↑↓ naviguer · ⏎ appliquer · Tab favori · Ctrl+T type · #png filtre · Échap fermer",
        ));
        hints.add_css_class("ws-hints");
        hints.set_xalign(0.0);
        hints.set_hexpand(true);
        let status = gtk::Label::new(None);
        status.add_css_class("ws-status");
        status.set_xalign(1.0);
        footer.append(&hints);
        footer.append(&status);

        card.append(&input_bar);
        card.append(&scrolled);
        card.append(&footer);
        window.set_child(Some(&card));

        let ui = Rc::new(UiState {
            cfg,
            all,
            favs,
            current,
            rows: RefCell::new(Vec::new()),
            gen: thumbs::GenerationState::default(),
            textures,
            warm_cursor: std::cell::Cell::new(0),
            app: app.clone(),
            window: window.clone(),
            entry: entry.clone(),
            store,
            selection,
            list_view,
            status,
        });

        ui.connect_signals();
        ui
    }

    fn connect_signals(self: &Rc<Self>) {
        // Frappe au clavier : la sélection repart sur le premier résultat.
        {
            let ui = self.clone();
            self.entry.connect_changed(move |_| ui.refilter(None));
        }

        // Navigation et actions. Phase « capture » pour que GtkListView ne voie
        // jamais les flèches ni Entrée (sinon double activation).
        {
            let ui = Rc::downgrade(self);
            let key = gtk::EventControllerKey::new();
            key.set_propagation_phase(gtk::PropagationPhase::Capture);
            key.connect_key_pressed(move |_, keyval, _, mods| {
                let Some(ui) = ui.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
                match keyval {
                    gdk::Key::Escape => {
                        ui.hide();
                        glib::Propagation::Stop
                    }
                    gdk::Key::Up | gdk::Key::KP_Up => {
                        ui.move_selection(-1);
                        glib::Propagation::Stop
                    }
                    gdk::Key::Down | gdk::Key::KP_Down => {
                        ui.move_selection(1);
                        glib::Propagation::Stop
                    }
                    // Début / fin des résultats filtrés.
                    gdk::Key::Page_Up | gdk::Key::Home => {
                        ui.select_first();
                        glib::Propagation::Stop
                    }
                    gdk::Key::Page_Down | gdk::Key::End => {
                        ui.select_last();
                        glib::Propagation::Stop
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                        ui.activate_selected();
                        glib::Propagation::Stop
                    }
                    // Tab n'est pas un caractère saisissable : c'est la seule
                    // touche simple qui ne rentre pas en conflit avec la
                    // recherche par frappe.
                    gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                        ui.toggle_favorite();
                        glib::Propagation::Stop
                    }
                    _ if ctrl && matches!(keyval, gdk::Key::f | gdk::Key::F) => {
                        ui.toggle_favorite();
                        glib::Propagation::Stop
                    }
                    _ if ctrl && matches!(keyval, gdk::Key::t | gdk::Key::T) => {
                        ui.cycle_type_filter();
                        glib::Propagation::Stop
                    }
                    _ if ctrl && matches!(keyval, gdk::Key::n | gdk::Key::j) => {
                        ui.move_selection(1);
                        glib::Propagation::Stop
                    }
                    _ if ctrl && matches!(keyval, gdk::Key::p | gdk::Key::k) => {
                        ui.move_selection(-1);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            self.window.add_controller(key);
        }

        // Double-clic souris.
        {
            let ui = self.clone();
            self.list_view.connect_activate(move |_, _| ui.activate_selected());
        }

        // Fermer la fenêtre = la cacher, pas tuer le processus résident.
        {
            let ui = self.clone();
            self.window.connect_close_request(move |w| {
                w.set_visible(false);
                if !ui.cfg.resident {
                    ui.app.quit();
                }
                glib::Propagation::Stop
            });
        }
    }

    // -- Actions -----------------------------------------------------------

    pub fn toggle(&self) {
        if self.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    pub fn show(&self) {
        let t = Timing::start();
        self.rescan();
        // Remet la recherche à zéro ; le signal « changed » déclenche le
        // refiltrage, l'appel explicite couvre le cas où le texte était déjà vide.
        self.entry.set_text("");
        self.refilter(None);
        self.window.set_visible(true);
        self.window.present();
        t.mark("present");
        self.entry.grab_focus();
        t.mark("focus");
        self.spawn_thumb_generation();
        t.mark("vignettes");
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
        if !self.cfg.resident {
            self.app.quit();
        }
    }

    pub fn quit(&self) {
        let _ = std::fs::remove_file(&self.cfg.socket);
        self.app.quit();
    }

    pub fn reload(&self) {
        self.rescan();
        self.refresh_preserving_selection();
        self.spawn_thumb_generation();
    }

    /// Recopie l'état du fichier de favoris dans les entrées affichées.
    fn sync_favorites(&self) {
        let favs = self.favs.borrow();
        let mut all = self.all.borrow_mut();
        for w in all.iter_mut() {
            w.favorite = favs.is_favorite(&w.id);
        }
        drop(favs);
        sort_for_display(&mut all);
    }

    /// Relit l'index : quelques millisecondes, donc on le fait à chaque
    /// ouverture pour prendre en compte les wallpapers ajoutés entre-temps.
    ///
    /// Le fichier de favoris est relu au passage : `--favorite` peut l'avoir
    /// modifié pendant que le processus résident tournait.
    fn rescan(&self) {
        self.favs.borrow_mut().reload();
        let fresh = scan::scan(&self.cfg, &self.favs.borrow());
        *self.all.borrow_mut() = fresh;
    }

    fn spawn_thumb_generation(&self) {
        let cfg = self.cfg.clone();
        let items = self.all.borrow().clone();
        let gen = self.gen.clone();
        std::thread::spawn(move || {
            thumbs::generate_missing(cfg, items, gen, || {
                on_main(|| {
                    current(|ui| ui.refresh_preserving_selection());
                });
            });
        });
    }

    pub fn refresh_preserving_selection(&self) {
        let keep = self.selected_id();
        self.refilter(keep.as_deref());
    }

    fn selected_id(&self) -> Option<String> {
        let sel = self.selection.selected();
        if sel == u32::MAX {
            return None;
        }
        let rows = self.rows.borrow();
        match rows.get(sel as usize) {
            Some(Row::Item(i)) => self.all.borrow().get(*i).map(|w| w.id.clone()),
            _ => None,
        }
    }

    fn selected_all_index(&self) -> Option<usize> {
        let sel = self.selection.selected();
        if sel == u32::MAX {
            return None;
        }
        let rows = self.rows.borrow();
        match rows.get(sel as usize) {
            Some(Row::Item(i)) => Some(*i),
            _ => None,
        }
    }

    /// Reconstruit la liste. `keep_id` permet de rester sur la même entrée
    /// (utile quand les vignettes se remplissent ou après avoir épinglé).
    fn refilter(&self, keep_id: Option<&str>) {
        let t = Timing::start();
        let query = self.entry.text().to_string();
        let rows = {
            let all = self.all.borrow();
            build_rows(&all, &query)
        };

        let target = keep_id.and_then(|id| {
            let all = self.all.borrow();
            rows.iter()
                .position(|r| matches!(r, Row::Item(i) if all.get(*i).is_some_and(|w| w.id == id)))
        });
        let first_item = rows.iter().position(|r| matches!(r, Row::Item(_)));

        // Cas le plus fréquent à l'ouverture : rien n'a bougé depuis la dernière
        // fois. On replace alors seulement la sélection. Reconstruire le modèle
        // ferait retomber la vue sur des centaines de liaisons de lignes, et
        // donc autant de vignettes à redécoder.
        let unchanged = {
            let current = self.rows.borrow();
            *current == rows
        };
        if unchanged {
            match target.or(first_item) {
                Some(i) => self.select_row(i),
                None => self.selection.set_selected(u32::MAX),
            }
            t.mark("refilter (inchangé)");
            self.update_status();
            return;
        }

        // Un seul `splice` remplace tout le contenu : boucler sur `append`
        // coûtait 650 ms pour 458 lignes, chaque ajout émettant son propre
        // `items-changed`.
        let objects: Vec<glib::BoxedAnyObject> =
            rows.iter().cloned().map(glib::BoxedAnyObject::new).collect();
        self.store.splice(0, self.store.n_items(), &objects);

        *self.rows.borrow_mut() = rows;
        match target.or(first_item) {
            Some(i) => self.select_row(i),
            None => self.selection.set_selected(u32::MAX),
        }
        t.mark("refilter (reconstruit)");
        self.update_status();
    }

    /// Pré-décode les vignettes par petits paquets, pendant que le processus est
    /// oisif, pour que la toute première ouverture soit aussi rapide que les
    /// suivantes.
    pub fn start_warming(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let _ = glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if ui.warm_cursor.get() >= ui.all.borrow().len() {
                return glib::ControlFlow::Break;
            }
            for _ in 0..16 {
                let idx = ui.warm_cursor.get();
                let item = {
                    let all = ui.all.borrow();
                    match all.get(idx) {
                        Some(w) => w.clone(),
                        None => break,
                    }
                };
                ui.warm_cursor.set(idx + 1);
                thumb_texture(&ui.textures, &ui.cfg.thumbs_dir, ui.cfg.thumb_px, &item);
            }
            glib::ControlFlow::Continue
        });
    }

    fn select_row(&self, row_index: usize) {
        if row_index >= self.rows.borrow().len() {
            return;
        }
        let pos = row_index as u32;
        self.selection.set_selected(pos);
        self.list_view.scroll_to(
            pos,
            gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT,
            None,
        );
    }

    fn select_first(&self) {
        let rows = self.rows.borrow();
        if let Some(i) = rows.iter().position(|r| matches!(r, Row::Item(_))) {
            drop(rows);
            self.select_row(i);
        }
    }

    fn select_last(&self) {
        let rows = self.rows.borrow();
        if let Some(i) = rows.iter().rposition(|r| matches!(r, Row::Item(_))) {
            drop(rows);
            self.select_row(i);
        }
    }

    /// Fait tourner le filtre de type en modifiant le jeton `#…` du champ de
    /// recherche : un seul mécanisme pour filtrer, et le filtre actif reste
    /// toujours visible à l'écran.
    fn cycle_type_filter(&self) {
        let mut words: Vec<String> = self
            .entry
            .text()
            .split_whitespace()
            .map(str::to_string)
            .collect();

        let current = words
            .last()
            .and_then(|w| w.strip_prefix('#'))
            .map(|t| t.to_ascii_lowercase());

        let next = match current.as_deref() {
            None => Some(TYPE_CYCLE[0]),
            Some(cur) => match TYPE_CYCLE.iter().position(|t| *t == cur) {
                // Fin du cycle : on retire le filtre.
                Some(i) if i + 1 == TYPE_CYCLE.len() => None,
                Some(i) => Some(TYPE_CYCLE[i + 1]),
                // Jeton saisi à la main (`#png`) : on entre dans le cycle.
                None => Some(TYPE_CYCLE[0]),
            },
        };

        if current.is_some() {
            words.pop();
        }
        if let Some(t) = next {
            words.push(format!("#{t}"));
        }

        let text = if words.is_empty() {
            String::new()
        } else {
            // Espace final conservé : on peut continuer à taper directement.
            format!("{} ", words.join(" "))
        };
        self.entry.set_text(&text);
        self.entry.set_position(-1);
    }

    /// Déplace la sélection en sautant les en-têtes de section.
    fn move_selection(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        let rows = self.rows.borrow();
        let n = rows.len() as i64;
        if n == 0 {
            return;
        }
        let sel = self.selection.selected();
        let mut idx = if sel == u32::MAX { -1 } else { sel as i64 };
        let dir: i64 = if delta > 0 { 1 } else { -1 };
        let mut left = delta.unsigned_abs() as i64;

        while left > 0 {
            let mut next = idx + dir;
            while next >= 0 && next < n && matches!(rows[next as usize], Row::Header { .. }) {
                next += dir;
            }
            if next < 0 || next >= n {
                break;
            }
            idx = next;
            left -= 1;
        }

        if idx >= 0 && idx != sel as i64 {
            drop(rows);
            self.select_row(idx as usize);
        }
    }

    fn activate_selected(&self) {
        let Some(i) = self.selected_all_index() else {
            return;
        };
        let item = self.all.borrow()[i].clone();
        let cfg = self.cfg.clone();
        self.hide();

        // Mémorisé avant de déplacer `item` dans le thread d'application.
        *self.current.borrow_mut() = Some(
            item.media
                .clone()
                .unwrap_or_else(|| item.path.clone()),
        );

        std::thread::spawn(move || {
            if let Err(e) = apply::apply(&cfg, &item) {
                eprintln!("[wallpaper-selector] {e}");
            }
        });
    }

    fn toggle_favorite(&self) {
        let Some(i) = self.selected_all_index() else {
            return;
        };
        let (id, favorite) = {
            let mut all = self.all.borrow_mut();
            let Some(w) = all.get_mut(i) else { return };
            w.favorite = !w.favorite;
            (w.id.clone(), w.favorite)
        };

        {
            let mut favs = self.favs.borrow_mut();
            // On repart du fichier pour ne pas écraser un `--favorite` lancé
            // depuis un script entre-temps.
            favs.reload();
            favs.set(&id, favorite);
            if let Err(e) = favs.save() {
                eprintln!("[wallpaper-selector] favoris non enregistrés : {e}");
            }
        }

        self.sync_favorites();
        self.refilter(Some(&id));
    }

    fn update_status(&self) {
        let total = self.all.borrow().len();
        let favs = self.favs.borrow().len();
        let shown = self.rows.borrow().len();
        let filter = match Query::parse(&self.entry.text()).type_label() {
            Some(f) => format!("type {f} · "),
            None => String::new(),
        };
        self.status.set_text(&format!(
            "{filter}{shown} affichés · {favs} favoris · {total} au total"
        ));
    }
}

/// Chronomètre de développement, activé par `WALLPAPER_SELECTOR_TIMING=1`.
///
/// Sert à distinguer le coût du travail sur la liste de celui de la mise à
/// l'écran, qui sont deux choses très différentes sur Wayland.
struct Timing {
    start: std::time::Instant,
    last: std::cell::Cell<std::time::Instant>,
    enabled: bool,
}

impl Timing {
    fn start() -> Self {
        let now = std::time::Instant::now();
        Timing {
            start: now,
            last: std::cell::Cell::new(now),
            enabled: std::env::var_os("WALLPAPER_SELECTOR_TIMING").is_some(),
        }
    }

    fn mark(&self, label: &str) {
        if !self.enabled {
            return;
        }
        let now = std::time::Instant::now();
        eprintln!(
            "[timing] {label:<10} {:>8.3} ms   (total {:>8.3} ms)",
            (now - self.last.get()).as_secs_f64() * 1000.0,
            (now - self.start).as_secs_f64() * 1000.0,
        );
        self.last.set(now);
    }
}

/// Renvoie la vignette décodée d'une entrée, en la mettant en cache au passage.
fn thumb_texture(
    cache: &Rc<RefCell<std::collections::HashMap<String, gdk::Texture>>>,
    thumbs_dir: &std::path::Path,
    px: i32,
    w: &Wallpaper,
) -> Option<gdk::Texture> {
    if let Some(tex) = cache.borrow().get(&w.id).cloned() {
        return Some(tex);
    }
    let pb = thumbs::load(&thumbs_dir.join(thumbs::hash_id(&w.id)), px)?;
    let tex = gdk::Texture::for_pixbuf(&pb);
    cache.borrow_mut().insert(w.id.clone(), tex.clone());
    Some(tex)
}

fn setup_layer_shell(window: &gtk::ApplicationWindow, cfg: &Config) {
    let display = gdk::Display::default();

    // Dimensions : pourcentages de l'écran, comme le thème rofi d'origine.
    let (mon_w, mon_h) = display
        .as_ref()
        .and_then(|d| d.monitors().item(0))
        .and_downcast::<gdk::Monitor>()
        .map(|m| {
            let g = m.geometry();
            (g.width(), g.height())
        })
        .unwrap_or((1920, 1080));

    let w = (mon_w as f64 * cfg.width_pct / 100.0).round() as i32;
    let h = (mon_h as f64 * cfg.height_pct / 100.0).round() as i32;
    window.set_default_size(w, h);

    if !gtk4_layer_shell::is_supported() {
        // Repli hors Wayland/layer-shell : fenêtre centrée classique.
        eprintln!("[wallpaper-selector] layer-shell indisponible, fenêtre normale");
        window.set_decorated(true);
        window.set_default_size(w, h);
        return;
    }

    // `init_layer_shell` doit être appelé avant la réalisation de la fenêtre.
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("wallpaper-selector"));
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_exclusive_zone(-1);

    // Une seule arête ancrée : la couche non ancrée est centrée par le
    // compositeur, et marge haute = centrage vertical.
    window.set_anchor(Edge::Top, true);
    window.set_margin(Edge::Top, ((mon_h - h) / 2).max(0));
}

/// Charge le thème : celui de l'utilisateur s'il existe, sinon celui embarqué.
fn load_css(cfg: &Config) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let css = std::fs::read_to_string(&cfg.user_css).unwrap_or_else(|_| DEFAULT_CSS.to_string());
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Lit le fond d'écran actuellement appliqué depuis le cache du script.
fn read_current(cfg: &Config) -> Option<PathBuf> {
    let text = std::fs::read_to_string(&cfg.last_wallpaper).ok()?;
    let (_, path) = text.trim().split_once('|')?;
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, Source};

    fn wp(title: &str, ext: &str, source: Source, favorite: bool) -> Wallpaper {
        let kind = match ext {
            "pkg" => Kind::Pkg,
            "gif" => Kind::Gif,
            "mp4" | "webm" => Kind::Video,
            _ => Kind::Image,
        };
        Wallpaper {
            id: format!("{}:{title}", source.label()),
            title: title.to_string(),
            path: PathBuf::from("/tmp"),
            preview: None,
            media: None,
            kind,
            ext: ext.to_string(),
            source,
            favorite,
        }
    }

    fn items() -> Vec<Wallpaper> {
        vec![
            wp("neocity.png", "png", Source::Perso, true),
            wp("cabin.jpg", "jpg", Source::Perso, true),
            wp("Dragon Bones", "pkg", Source::Steam, false),
            wp("Snow", "mp4", Source::Steam, false),
            wp("Loop", "gif", Source::Steam, false),
        ]
    }

    #[test]
    fn parse_separe_texte_et_filtres() {
        let q = Query::parse("#PNG montagne #jpg");
        assert_eq!(q.text, "montagne");
        assert_eq!(q.types, vec!["png", "jpg"]);

        let vide = Query::parse("   ");
        assert!(vide.text.is_empty() && vide.types.is_empty());

        // Un « # » seul reste du texte libre.
        let diese = Query::parse("#");
        assert_eq!(diese.text, "#");
        assert!(diese.types.is_empty());
    }

    #[test]
    fn filtre_sur_l_extension_reelle() {
        let q = Query::parse("#png");
        assert!(q.matches(&wp("a.png", "png", Source::Perso, false)));
        assert!(!q.matches(&wp("a.jpg", "jpg", Source::Perso, false)));
    }

    #[test]
    fn alias_de_type() {
        let video = Query::parse("#video");
        assert!(video.matches(&wp("a.mp4", "mp4", Source::Steam, false)));
        assert!(video.matches(&wp("a.webm", "webm", Source::Steam, false)));
        assert!(!video.matches(&wp("a.png", "png", Source::Perso, false)));

        let image = Query::parse("#img");
        assert!(image.matches(&wp("a.png", "png", Source::Perso, false)));
        assert!(image.matches(&wp("a.jpg", "jpg", Source::Perso, false)));
        assert!(!image.matches(&wp("a.pkg", "pkg", Source::Steam, false)));
    }

    #[test]
    fn filtre_par_source() {
        let steam = Query::parse("#steam");
        assert!(steam.matches(&wp("Dragon", "pkg", Source::Steam, false)));
        assert!(!steam.matches(&wp("cabin", "png", Source::Perso, false)));
    }

    #[test]
    fn plusieurs_filtres_se_cumulent_en_ou() {
        let q = Query::parse("#png #jpg");
        assert!(q.matches(&wp("a.png", "png", Source::Perso, false)));
        assert!(q.matches(&wp("b.jpg", "jpg", Source::Perso, false)));
        assert!(!q.matches(&wp("c.pkg", "pkg", Source::Steam, false)));
    }

    #[test]
    fn texte_et_filtre_se_cumulent_en_et() {
        let q = Query::parse("#png neocity");
        assert!(q.matches(&wp("neocity.png", "png", Source::Perso, false)));
        assert!(!q.matches(&wp("cabin.png", "png", Source::Perso, false)));
    }

    #[test]
    fn favoris_en_tete_avec_en_tete_de_section() {
        let rows = build_rows(&items(), "");
        assert!(matches!(rows[0], Row::Header { .. }));
        assert_eq!(rows[1], Row::Item(0));
        assert_eq!(rows[2], Row::Item(1));
        // Puis une seconde section pour le reste.
        let second = rows
            .iter()
            .rposition(|r| matches!(r, Row::Header { .. }))
            .map(|i| i + 1)
            .unwrap();
        assert_eq!(rows[second], Row::Item(2));
        assert_eq!(rows.len(), 7); // 2 en-têtes + 5 entrées
    }

    #[test]
    fn aucun_resultat_ne_produit_aucune_ligne() {
        assert!(build_rows(&items(), "#webm").is_empty());
        assert!(build_rows(&items(), "introuvable").is_empty());
    }

    #[test]
    fn filtre_reduit_les_deux_sections() {
        let rows = build_rows(&items(), "#png");
        // Le favori PNG garde sa section, le reste disparaît.
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], Row::Header { .. }));
        assert_eq!(rows[1], Row::Item(0));
    }
}
