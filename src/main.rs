mod apply;
mod config;
mod favorites;
mod ipc;
mod model;
mod scan;
mod thumbs;
mod ui;

use std::process::ExitCode;

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use config::Config;
use favorites::Favorites;
use ipc::Command;
use model::Wallpaper;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Marqueur posé dans l'environnement du processus détaché, pour qu'il ne se
/// détache pas une seconde fois.
const DETACHED_ENV: &str = "WALLPAPER_SELECTOR_DETACHED";

#[derive(Debug)]
enum Mode {
    Toggle,
    Show,
    Hide,
    Quit,
    Reload,
    RebuildCache,
    Favorites,
    Favorite(String, bool),
    Print(bool),
    Help,
    Version,
}

#[derive(Debug)]
struct Cli {
    mode: Mode,
    /// Rester attaché au terminal au lieu de passer en tâche de fond.
    foreground: bool,
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut mode = Mode::Toggle;
    let mut foreground = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => return Ok(Cli { mode: Mode::Help, foreground }),
            "-V" | "--version" => return Ok(Cli { mode: Mode::Version, foreground }),
            "-F" | "--foreground" => foreground = true,
            "-t" | "--toggle" => mode = Mode::Toggle,
            "-s" | "--show" => mode = Mode::Show,
            "--hide" => mode = Mode::Hide,
            "-q" | "--quit" => mode = Mode::Quit,
            "--reload" => mode = Mode::Reload,
            "--rebuild-cache" => mode = Mode::RebuildCache,
            "--favorites" | "--list-favorites" => mode = Mode::Favorites,
            "--print" => mode = Mode::Print(false),
            "--json" => mode = Mode::Print(true),
            "-f" | "--favorite" | "--unfavorite" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{a} attend un identifiant (ex. steam:1355236618)"))?;
                mode = Mode::Favorite(value.clone(), a != "--unfavorite");
                i += 1;
            }
            other => return Err(format!("option inconnue : {other}")),
        }
        i += 1;
    }
    Ok(Cli { mode, foreground })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[wallpaper-selector] {e}");
            eprintln!("Essayez --help.");
            return ExitCode::FAILURE;
        }
    };

    let cfg = Config::load();
    cfg.ensure_dirs();
    let favs = Favorites::load(&cfg.favorites_file);
    let foreground = cli.foreground;
    let mode = cli.mode;

    match mode {
        Mode::Help => {
            print_help();
            ExitCode::SUCCESS
        }
        Mode::Version => {
            println!("wallpaper-selector {VERSION}");
            ExitCode::SUCCESS
        }
        Mode::Toggle => dispatch(&cfg, favs, Command::Toggle, foreground),
        Mode::Show => dispatch(&cfg, favs, Command::Show, foreground),
        Mode::Hide => dispatch(&cfg, favs, Command::Hide, foreground),
        Mode::Reload => dispatch(&cfg, favs, Command::Reload, foreground),
        Mode::Quit => {
            if ipc::send(&cfg.socket, Command::Quit).is_ok() {
                ExitCode::SUCCESS
            } else {
                eprintln!("[wallpaper-selector] aucune instance en cours");
                ExitCode::FAILURE
            }
        }
        Mode::Favorites => {
            let mut ids: Vec<String> = scan::scan(&cfg, &favs)
                .into_iter()
                .filter(|w| w.favorite)
                .map(|w| w.id)
                .collect();
            ids.sort();
            for id in ids {
                println!("{id}");
            }
            ExitCode::SUCCESS
        }
        Mode::Favorite(id, on) => {
            let mut favs = favs;
            if !id_exists(&cfg, &favs, &id) {
                eprintln!("[wallpaper-selector] identifiant inconnu : {id}");
                return ExitCode::FAILURE;
            }
            favs.set(&id, on);
            match favs.save() {
                Ok(()) => {
                    println!("{} → {}", id, if on { "favori" } else { "non favori" });
                    if ipc::send(&cfg.socket, Command::Reload).is_err() {
                        // Pas de daemon : rien à rafraîchir, c'est très bien.
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("[wallpaper-selector] écriture impossible : {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Mode::Print(json) => {
            let items = scan::scan(&cfg, &favs);
            if json {
                match serde_json::to_string_pretty(&items) {
                    Ok(s) => {
                        println!("{s}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("[wallpaper-selector] {e}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                for w in &items {
                    println!(
                        "{} {:<40} {:<6} {}",
                        if w.favorite { "★" } else { " " },
                        w.title,
                        w.kind.badge(),
                        w.id
                    );
                }
                ExitCode::SUCCESS
            }
        }
        Mode::RebuildCache => rebuild_cache(&cfg, &favs),
    }
}

fn id_exists(cfg: &Config, favs: &Favorites, id: &str) -> bool {
    scan::scan(cfg, favs).iter().any(|w| w.id == id)
}

/// Envoie la commande au processus résident, ou démarre l'interface.
fn dispatch(cfg: &Config, favs: Favorites, command: Command, foreground: bool) -> ExitCode {
    if cfg.resident && ipc::send(&cfg.socket, command).is_ok() {
        return ExitCode::SUCCESS;
    }
    // Pas de daemon : c'est nous qui prenons le relais.
    run_ui(cfg.clone(), favs, command, foreground)
}

/// Relance le binaire en tâche de fond, détaché du terminal.
///
/// Sans ça, lancer la commande à la main bloque le shell : le processus
/// résident ne rend jamais la main, il faut un Ctrl+C pour récupérer l'invite.
fn detach(cfg: &Config) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let log_path = cfg.cache_root.join("daemon.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log = std::fs::File::create(&log_path)?;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args)
        .env(DETACHED_ENV, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log);

    unsafe {
        cmd.pre_exec(|| {
            // Nouvelle session : le résident ne meurt pas avec le terminal et
            // ne reçoit pas son SIGHUP.
            libc::setsid();
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    println!(
        "[wallpaper-selector] processus résident lancé (pid {}) · journal : {}",
        child.id(),
        log_path.display()
    );
    Ok(())
}

fn run_ui(cfg: Config, favs: Favorites, command: Command, foreground: bool) -> ExitCode {
    if cfg.resident && !foreground && std::env::var_os(DETACHED_ENV).is_none() {
        match detach(&cfg) {
            Ok(()) => return ExitCode::SUCCESS,
            Err(e) => eprintln!(
                "[wallpaper-selector] détachement impossible ({e}), on reste au premier plan"
            ),
        }
    }

    let items = scan::scan(&cfg, &favs);

    // On s'approprie le socket avant de lancer GTK : si une instance répond
    // déjà (course au tout premier lancement), on lui délègue la commande.
    if cfg.resident {
        let handler = move |cmd: Command, ack: Box<dyn FnOnce() + Send>| {
            ui::on_main(move || {
                ui::current(|ui| match cmd {
                    Command::Toggle => ui.toggle(),
                    Command::Show => ui.show(),
                    Command::Hide => ui.hide(),
                    Command::Quit => ui.quit(),
                    Command::Reload => ui.reload(),
                });
                ack();
            });
        };
        match ipc::serve(&cfg.socket, handler) {
            Ok(()) => {}
            Err(ipc::ServeError::AlreadyRunning) => {
                let _ = ipc::send(&cfg.socket, command);
                return ExitCode::SUCCESS;
            }
            Err(ipc::ServeError::Io(e)) => {
                eprintln!("[wallpaper-selector] socket indisponible ({e}), mode sans daemon");
            }
        }
    }

    let app = gtk::Application::builder()
        .application_id("dev.wallpaperselector.WallpaperSelector")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let payload = std::rc::Rc::new(std::cell::RefCell::new(Some((items, favs, command))));
    let cfg_for_app = cfg.clone();

    app.connect_activate(move |app| {
        let Some((items, favs, command)) = payload.borrow_mut().take() else {
            return;
        };
        // Empêche GTK de quitter quand la fenêtre est seulement masquée. Le
        // garde libère la référence en étant détruit, on le fuit donc
        // volontairement : il doit vivre aussi longtemps que le processus.
        std::mem::forget(app.hold());
        let ui = ui::UiState::build(app, cfg_for_app.clone(), items, favs);
        ui::set_current(ui.clone());
        ui.start_warming();
        match command {
            Command::Hide => {}
            // Premier lancement (toggle, show ou reload) : on montre la fenêtre.
            _ => ui.show(),
        }
    });

    // `run()` passe tout `argv` à GOptionContext, qui rejetterait nos propres
    // options (`--toggle`, `--show`…). On ne lui transmet donc que argv[0].
    app.run_with_args(&["wallpaper-selector"]);
    ExitCode::SUCCESS
}

fn rebuild_cache(cfg: &Config, favs: &Favorites) -> ExitCode {
    match thumbs::clear(cfg) {
        Ok(n) => eprintln!("[wallpaper-selector] {n} vignettes supprimées"),
        Err(e) => eprintln!("[wallpaper-selector] nettoyage impossible : {e}"),
    }
    let items: Vec<Wallpaper> = scan::scan(cfg, favs);
    let total = items.len();
    eprintln!("[wallpaper-selector] génération de {total} vignettes…");

    thumbs::generate_missing(
        cfg.clone(),
        items,
        thumbs::GenerationState::default(),
        || {},
    );

    // Passe finale pour compter ce qui a réellement été produit.
    let mut ok = 0;
    for w in scan::scan(cfg, favs) {
        if thumbs::thumb_path(cfg, &w.id).is_file() {
            ok += 1;
        }
    }
    println!("{ok}/{total} vignettes en cache dans {}", cfg.thumbs_dir.display());
    ExitCode::SUCCESS
}

fn print_help() {
    println!(
        r#"wallpaper-selector {VERSION}
Sélecteur de fonds d'écran instantané pour Hyprland.

USAGE:
    wallpaper-selector [OPTIONS]

OPTIONS:
    -t, --toggle          Bascule la fenêtre (action par défaut)
    -s, --show            Affiche la fenêtre
        --hide            Masque la fenêtre
    -q, --quit            Arrête le processus résident
        --reload          Relit la liste sans rouvrir la fenêtre
        --rebuild-cache   Régénère toutes les vignettes
        --print           Liste l'index en texte
        --json            Liste l'index en JSON (champ « favorite » inclus)
        --favorites       Liste les identifiants favoris
    -f, --favorite ID     Marque une entrée comme favorite
        --unfavorite ID   Retire une entrée des favoris
    -F, --foreground      Reste attaché au terminal (débogage)
    -h, --help            Affiche cette aide
    -V, --version         Affiche la version

Le premier lancement passe en tâche de fond et se détache du terminal :
le processus résident garde l'index et les vignettes en mémoire, ce qui rend
les ouvertures suivantes quasi instantanées. Pour tout voir défiler dans le
terminal, utiliser -F.

RACCOURCIS DANS L'INTERFACE:
    ↑ ↓ / Ctrl+N Ctrl+P   Naviguer (les en-têtes sont sautés)
    Page↑ / Début         Premier résultat
    Page↓ / Fin           Dernier résultat
    Entrée                Appliquer le fond d'écran sélectionné
    Tab / Ctrl+F          Épingler ou désépingler le favori
    Ctrl+T                Faire tourner le filtre de type
    Échap                 Fermer

RECHERCHE PAR TYPE:
    Un jeton préfixé par « # » filtre sur l'extension réelle :
        #png      PNG
        #jpg      JPEG
        #pkg      scènes Wallpaper Engine
        #video    mp4, webm, mkv, mov
        #gif      GIF
        #steam    entrées du Workshop    #perso  fichiers locaux
    Plusieurs jetons se cumulent en OU, et le texte libre continue de filtrer :
        #png montagne      les PNG dont le titre contient « montagne »
        #png #jpg          les PNG et les JPEG

CONFIGURATION:
    ~/.config/wallpaper-selector/config    (clé = valeur)
    ~/.config/wallpaper-selector/style.css (remplace le thème intégré)
    ~/.config/wallpaper-selector/favorites (identifiants, un par ligne)

Journal du processus résident : ~/.cache/wallpaper-selector/daemon.log
"#
    );
}
