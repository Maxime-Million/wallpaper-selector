//! Scan de l'index des fonds d'écran.
//!
//! Contrairement au script bash d'origine, ce module **ne lance aucun
//! sous-processus** : plus de `grep`/`find` forkés pour chaque dossier. Un
//! `read_dir` par dossier Steam et une seule lecture de `read_dir` pour le
//! dossier perso suffisent (mesuré : ~2-5 ms pour 70 + 388 entrées).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::Config;
use crate::favorites::Favorites;
use crate::model::{sort_for_display, Kind, Source, Wallpaper};

const PREVIEW_STEMS: [&str; 4] = ["preview", "thumbnail", "thumb", "screenshot"];

pub fn scan(cfg: &Config, favs: &Favorites) -> Vec<Wallpaper> {
    let mut out = Vec::new();
    scan_steam(cfg, favs, &mut out);
    scan_perso(cfg, favs, &mut out);
    sort_for_display(&mut out);
    out
}

fn scan_steam(cfg: &Config, favs: &Favorites, out: &mut Vec<Wallpaper>) {
    let Ok(entries) = fs::read_dir(&cfg.steam_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let ws_id = entry.file_name().to_string_lossy().into_owned();
        let (declared_title, declared_type) = read_project_json(&dir);
        let title = declared_title.unwrap_or_else(|| ws_id.clone());

        let mut has_pkg = false;
        let mut media: Option<PathBuf> = None;
        let mut media_kind: Option<Kind> = None;
        let mut preview: Option<PathBuf> = None;

        if let Ok(inner) = fs::read_dir(&dir) {
            for f in inner.flatten() {
                if !f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                let path = f.path();
                let name = f.file_name().to_string_lossy().to_ascii_lowercase();

                if name.ends_with(".pkg") {
                    has_pkg = true;
                    continue;
                }

                let stem = Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let ext = Path::new(&name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                if PREVIEW_STEMS.contains(&stem.as_str()) && Kind::from_extension(&ext).is_some() {
                    // On garde le premier trouvé, mais un JPEG prend le pas
                    // (preview.jpg est plus léger que preview.png chez la
                    // plupart des items du Workshop).
                    let is_jpeg = ext == "jpg" || ext == "jpeg";
                    if preview.is_none() || is_jpeg {
                        preview = Some(path.clone());
                    }
                    continue;
                }
                if name.contains("preview") || name.contains("thumb") {
                    continue;
                }

                if media.is_none() {
                    if let Some(k) = Kind::from_extension(&ext) {
                        media_kind = Some(k);
                        media = Some(path);
                    }
                }
            }
        }

        // Certains items rangent leurs médias dans un sous-dossier : on ne
        // descend que si le scan à plat n'a rien trouvé, pour ne pas payer le
        // coût sur les 70 items.
        if !has_pkg && media.is_none() && preview.is_none() {
            find_media_deep(&dir, 3, &mut preview, &mut media, &mut media_kind);
        }

        let kind = if has_pkg {
            Kind::Pkg
        } else {
            media_kind
                .or(match declared_type.as_deref() {
                    Some("video") => Some(Kind::Video),
                    _ => None,
                })
                .unwrap_or(Kind::Image)
        };

        let ext = if has_pkg {
            "pkg".to_string()
        } else {
            media
                .as_deref()
                .and_then(Path::extension)
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        };

        let id = format!("steam:{ws_id}");
        out.push(Wallpaper {
            favorite: favs.is_favorite(&id),
            id,
            title,
            path: dir,
            preview,
            media,
            kind,
            ext,
            source: Source::Steam,
        });
    }
}

fn find_media_deep(
    dir: &Path,
    depth: u8,
    preview: &mut Option<PathBuf>,
    media: &mut Option<PathBuf>,
    media_kind: &mut Option<Kind>,
) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for f in entries.flatten() {
        let path = f.path();
        let name = f.file_name().to_string_lossy().to_ascii_lowercase();
        if f.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if name == "assets" || name == "media" || name == "images" || depth > 1 {
                find_media_deep(&path, depth - 1, preview, media, media_kind);
            }
            continue;
        }
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let ext = Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if PREVIEW_STEMS.contains(&stem) && preview.is_none() {
            *preview = Some(path);
        } else if media.is_none() && !name.contains("preview") && !name.contains("thumb") {
            if let Some(k) = Kind::from_extension(ext) {
                *media_kind = Some(k);
                *media = Some(path);
            }
        }
    }
}

/// Lit `title` et `type` de `project.json`.
///
/// L'ancien script prenait le **dernier** `"type"` du fichier via `tail -1`, ce
/// qui pouvait capturer une clé imbriquée ; ici on lit la clé de premier niveau.
fn read_project_json(dir: &Path) -> (Option<String>, Option<String>) {
    let path = dir.join("project.json");
    let Ok(bytes) = fs::read(&path) else {
        return (None, None);
    };
    let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
        return (None, None);
    };
    let title = json
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let ty = json
        .get("type")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());
    (title, ty)
}

fn scan_perso(cfg: &Config, favs: &Favorites, out: &mut Vec<Wallpaper>) {
    let Ok(entries) = fs::read_dir(&cfg.perso_dir) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let ext = Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let Some(kind) = Kind::from_extension(ext) else {
            continue;
        };
        if !Kind::is_image_extension(ext) {
            continue;
        }

        let path = entry.path();
        let id = format!("perso:{name}");
        // Calculé avant la construction : `title` déplace `name`, dont `ext`
        // n'est qu'un emprunt.
        let ext = ext.to_ascii_lowercase();
        out.push(Wallpaper {
            favorite: favs.is_favorite(&id),
            id,
            title: name,
            preview: Some(path.clone()),
            media: Some(path.clone()),
            path,
            kind,
            ext,
            source: Source::Perso,
        });
    }
}
