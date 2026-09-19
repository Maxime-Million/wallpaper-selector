//! Chemins, constantes et fichier de configuration utilisateur.
//!
//! La config est un simple fichier `clé = valeur` dans
//! `~/.config/wallpaper-selector/config` — volontairement minimal pour rester
//! éditable à la main et sans dépendance supplémentaire.

use std::collections::HashMap;
use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub steam_dir: PathBuf,
    pub perso_dir: PathBuf,

    pub config_file: PathBuf,
    pub favorites_file: PathBuf,
    pub cache_root: PathBuf,
    pub thumbs_dir: PathBuf,
    pub user_css: PathBuf,
    pub last_wallpaper: PathBuf,
    pub socket: PathBuf,

    /// Largeur de la fenêtre, en pourcentage de l'écran.
    pub width_pct: f64,
    /// Hauteur de la fenêtre, en pourcentage de l'écran.
    pub height_pct: f64,
    /// Taille d'affichage des vignettes (px logiques).
    pub thumb_px: i32,
    /// Taille de décodage des vignettes mises en cache (px).
    pub thumb_cache_px: i32,

    /// Écran ciblé par `linux-wallpaperengine --screen-root`.
    pub monitor: Option<String>,
    /// Cible passée à `mpvpaper` (`*` = tous les écrans).
    pub mpv_output: String,

    pub transition_type: String,
    pub transition_duration: f64,

    /// Si faux, le processus ne reste pas en mémoire après la fermeture.
    pub resident: bool,
    /// Appliquer au simple survol ? (non implémenté, réservé)
    pub autoselect_first: bool,
}

impl Default for Config {
    fn default() -> Self {
        let cfg_dir = config_dir().join("wallpaper-selector");
        let cache = cache_dir().join("wallpaper-selector");

        let steam_dir = std::env::var_os("WALLPAPER_STEAM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                home().join(".local/share/Steam/steamapps/workshop/content/431960")
            });

        let perso_dir = std::env::var_os("WALLPAPER_PERSO_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("Images/wallpapers"));

        Config {
            steam_dir,
            perso_dir,
            config_file: cfg_dir.join("config"),
            favorites_file: cfg_dir.join("favorites"),
            cache_root: cache.clone(),
            thumbs_dir: cache.join("thumbs"),
            user_css: cfg_dir.join("style.css"),
            last_wallpaper: cache_dir().join("last_wallpaper_id"),
            socket: runtime_dir().join("wallpaper-selector.sock"),

            width_pct: 35.0,
            height_pct: 55.0,
            thumb_px: 80,
            thumb_cache_px: 256,

            monitor: None,
            mpv_output: "*".to_string(),

            transition_type: "grow".to_string(),
            transition_duration: 1.2,

            resident: true,
            autoselect_first: true,
        }
    }
}

fn parse_kv(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            out.insert(k.trim().to_ascii_lowercase(), v.to_string());
        }
    }
    out
}

fn unquote_path(v: &str) -> PathBuf {
    // `~` et `~/...` sont développés, les variables d'env ne le sont pas.
    if v == "~" {
        return home();
    }
    if let Some(rest) = v.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(v)
}

impl Config {
    pub fn load() -> Self {
        let mut cfg = Config::default();
        if let Ok(text) = std::fs::read_to_string(&cfg.config_file) {
            cfg.apply(&parse_kv(&text));
        }
        cfg
    }

    fn apply(&mut self, kv: &HashMap<String, String>) {
        let b = |k: &str| -> Option<bool> {
            kv.get(k).map(|v| {
                matches!(
                    v.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "oui" | "on"
                )
            })
        };
        let f = |k: &str| -> Option<f64> { kv.get(k).and_then(|v| v.parse().ok()) };
        let i = |k: &str| -> Option<i32> { kv.get(k).and_then(|v| v.parse().ok()) };

        if let Some(v) = kv.get("steam_dir") {
            self.steam_dir = unquote_path(v);
        }
        if let Some(v) = kv.get("perso_dir") {
            self.perso_dir = unquote_path(v);
        }
        if let Some(v) = kv.get("mpv_output") {
            self.mpv_output = v.clone();
        }
        if let Some(v) = kv.get("monitor") {
            self.monitor = if v.is_empty() || v == "auto" {
                None
            } else {
                Some(v.clone())
            };
        }
        if let Some(v) = kv.get("transition_type") {
            self.transition_type = v.clone();
        }

        if let Some(v) = f("width_pct") {
            self.width_pct = v.clamp(10.0, 100.0);
        }
        if let Some(v) = f("height_pct") {
            self.height_pct = v.clamp(10.0, 100.0);
        }
        if let Some(v) = f("width") {
            self.width_pct = v.clamp(10.0, 100.0);
        }
        if let Some(v) = f("height") {
            self.height_pct = v.clamp(10.0, 100.0);
        }
        if let Some(v) = i("thumb_px") {
            self.thumb_px = v.clamp(16, 512);
        }
        if let Some(v) = i("thumb_cache_px") {
            self.thumb_cache_px = v.clamp(64, 2048);
        }
        if let Some(v) = f("transition_duration") {
            self.transition_duration = v.clamp(0.0, 20.0);
        }
        if let Some(v) = b("resident") {
            self.resident = v;
        }
        if let Some(v) = b("autoselect_first") {
            self.autoselect_first = v;
        }
    }

    /// Crée les dossiers de cache/config nécessaires.
    pub fn ensure_dirs(&self) {
        for d in [self.cache_root.as_path(), self.thumbs_dir.as_path()] {
            let _ = std::fs::create_dir_all(d);
        }
        if let Some(parent) = self.favorites_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
}
