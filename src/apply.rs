//! Application du fond d'écran choisi.
//!
//! Reprend la logique du script d'origine — `awww` pour les images, `mpvpaper`
//! pour les vidéos, `linux-wallpaperengine` pour les scènes Wallpaper Engine —
//! en la rendant explicite et en conservant le fichier
//! `~/.cache/last_wallpaper_id` au même format, pour ne rien casser des autres
//! scripts de la session.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::model::{Kind, Source, Wallpaper};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Awww,
    Mpvpaper,
    WallpaperEngine,
}

impl Backend {
    pub fn cache_tag(self) -> &'static str {
        match self {
            Backend::Awww => "AWWW",
            Backend::Mpvpaper => "MPV",
            Backend::WallpaperEngine => "LWE",
        }
    }
}

fn spawn(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(e) = cmd.spawn() {
        eprintln!("[wallpaper-selector] échec du lancement : {e}");
    }
}

/// `pkill` renvoie 1 quand aucun processus ne correspond : ce n'est pas une erreur.
fn pkill(args: &[&str]) {
    let _ = Command::new("pkill")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Nom de processus limité à 15 caractères côté noyau (`comm`). C'est pour ça
/// que `pkill -x linux-wallpaperengine` ne marcherait pas : on cible la ligne
/// de commande complète avec `-f`.
fn stop_mpvpaper() {
    pkill(&["-x", "mpvpaper"]);
}
fn stop_wallpaper_engine() {
    pkill(&["-f", "linux-wallpaperengine"]);
}
fn stop_awww() {
    pkill(&["-x", "awww"]);
    pkill(&["-x", "awww-daemon"]);
}

fn awww_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    let path = Path::new(&runtime).join(format!("{display}-awww-daemon.sock"));
    Some(path)
}

/// Démarre le démon awww si nécessaire, sans le lancer deux fois.
fn ensure_awww_daemon() {
    if let Some(sock) = awww_socket() {
        if sock.exists() {
            return;
        }
    }
    spawn(Command::new("awww-daemon").args(["--format", "argb"]));

    // Attente du socket, bornée : on ne veut pas bloquer l'application.
    if let Some(sock) = awww_socket() {
        for _ in 0..50 {
            if sock.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Position du curseur, pour la transition « grow » qui part du pointeur.
fn cursor_pos() -> Option<String> {
    let out = Command::new("hyprctl").args(["-j", "cursorpos"]).output().ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let x = json.get("x")?.as_i64()?;
    let y = json.get("y")?.as_i64()?;
    Some(format!("{x},{y}"))
}

/// Écran actuellement focalisé, pour `--screen-root` de linux-wallpaperengine.
fn focused_monitor() -> Option<String> {
    let out = Command::new("hyprctl").args(["-j", "monitors"]).output().ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json.as_array()?
        .iter()
        .find(|m| m.get("focused").and_then(serde_json::Value::as_bool).unwrap_or(false))
        .and_then(|m| m.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

pub fn backend_for(kind: Kind) -> Backend {
    match kind {
        Kind::Pkg => Backend::WallpaperEngine,
        Kind::Video => Backend::Mpvpaper,
        Kind::Gif | Kind::Image => Backend::Awww,
    }
}

pub fn apply(cfg: &Config, w: &Wallpaper) -> io::Result<()> {
    let backend = backend_for(w.kind);

    // On coupe systématiquement les moteurs qui ne servent pas : le script
    // d'origine le faisait aussi, sinon deux fonds se superposent.
    match backend {
        Backend::Awww => {
            stop_mpvpaper();
            stop_wallpaper_engine();
        }
        Backend::Mpvpaper => {
            stop_awww();
            stop_wallpaper_engine();
            stop_mpvpaper();
        }
        Backend::WallpaperEngine => {
            stop_mpvpaper();
            stop_awww();
        }
    }

    let target: String = match backend {
        Backend::WallpaperEngine => {
            let monitor = cfg
                .monitor
                .clone()
                .or_else(focused_monitor)
                .unwrap_or_else(|| "eDP-1".to_string());
            spawn(
                Command::new("linux-wallpaperengine")
                    .arg(&w.path)
                    .args(["--screen-root", &monitor]),
            );
            w.path.display().to_string()
        }
        Backend::Mpvpaper => {
            let Some(media) = w.media.as_deref() else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "aucun média vidéo"));
            };
            spawn(Command::new("mpvpaper").args([
                "-o",
                "no-audio --loop-playlist",
                &cfg.mpv_output,
                &media.to_string_lossy(),
            ]));
            media.display().to_string()
        }
        Backend::Awww => {
            let media = w.media.as_deref().unwrap_or(w.path.as_path());
            ensure_awww_daemon();
            let mut cmd = Command::new("awww");
            cmd.arg("img").arg(media);
            cmd.args(["--transition-type", &cfg.transition_type]);
            if let Some(pos) = cursor_pos() {
                cmd.args(["--transition-pos", &pos]);
            }
            cmd.args(["--transition-duration", &cfg.transition_duration.to_string()]);
            spawn(&mut cmd);
            media.display().to_string()
        }
    };

    // Format conservé à l'identique pour ne pas casser les autres scripts.
    if let Some(parent) = cfg.last_wallpaper.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &cfg.last_wallpaper,
        format!("{}|{target}", backend.cache_tag()),
    );

    Ok(())
}

/// Analyse le contenu de `last_wallpaper_id` (`TYPE|chemin`).
///
/// Le format est celui du script bash d'origine, pour que les deux
/// continuent de se comprendre : `AWWW|photo.png`, `MPV|clip.mp4`,
/// `LWE|/chemin/vers/dossier`.
pub fn parse_cache(text: &str) -> Option<(Backend, PathBuf)> {
    let (tag, target) = text.trim().split_once('|')?;
    let backend = match tag {
        "AWWW" => Backend::Awww,
        "MPV" => Backend::Mpvpaper,
        "LWE" => Backend::WallpaperEngine,
        _ => return None,
    };
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    Some((backend, PathBuf::from(target)))
}

/// Rejoue le dernier fond d'écran appliqué, sans ouvrir l'interface.
///
/// C'est ce que lance `wallpaper-selector --restore`, utilisé au démarrage de
/// la session (voir `scripts/init-wallpaper.sh`) pour retrouver le fond
/// d'écran choisi la veille.
pub fn restore(cfg: &Config) -> io::Result<()> {
    let text = std::fs::read_to_string(&cfg.last_wallpaper).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("aucun fond d'écran enregistré ({})", cfg.last_wallpaper.display()),
        )
    })?;
    let (backend, target) = parse_cache(&text).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("contenu illisible : {}", text.trim()),
        )
    })?;

    let kind = match backend {
        Backend::WallpaperEngine => Kind::Pkg,
        Backend::Mpvpaper => Kind::Video,
        Backend::Awww => Kind::Image,
    };
    // Pour un `.pkg`, la cible est le dossier de la scène ; pour les autres,
    // c'est le média lui-même.
    let media = match kind {
        Kind::Pkg => None,
        _ => Some(target.clone()),
    };

    let w = Wallpaper {
        id: String::new(),
        title: String::new(),
        path: target,
        preview: None,
        media,
        kind,
        source: Source::Perso,
        ext: String::new(),
        favorite: false,
    };
    apply(cfg, &w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cache_reconnait_les_trois_moteurs() {
        assert_eq!(
            parse_cache("AWWW|/tmp/a.png\n"),
            Some((Backend::Awww, PathBuf::from("/tmp/a.png")))
        );
        assert_eq!(
            parse_cache("MPV|/tmp/b.mp4"),
            Some((Backend::Mpvpaper, PathBuf::from("/tmp/b.mp4")))
        );
        assert_eq!(
            parse_cache("LWE|/games/123"),
            Some((Backend::WallpaperEngine, PathBuf::from("/games/123")))
        );
    }

    #[test]
    fn parse_cache_rejette_le_restee() {
        assert_eq!(parse_cache(""), None);
        assert_eq!(parse_cache("sans separateur"), None);
        assert_eq!(parse_cache("AWWW|"), None);
        assert_eq!(parse_cache("INCONNU|/tmp/a.png"), None);
    }
}
