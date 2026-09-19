//! Cache de vignettes.
//!
//! C'est ici que se joue l'essentiel du gain de vitesse. Le dossier perso pèse
//! 652 Mo avec des PNG de 21 Mo : les décoder en pleine résolution à chaque
//! ouverture de l'interface coûtait des centaines de millisecondes. On les
//! décode **une fois** vers `~/.cache/wallpaper-selector/thumbs/<hash>.png`
//! (~256 px), et l'affichage ne lit plus qu'un petit fichier.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use gdk_pixbuf::Pixbuf;

use crate::config::Config;
use crate::model::Wallpaper;

/// Hash FNV-1a 64 bits, stable entre les versions de Rust (contrairement à
/// `DefaultHasher`) : les noms de fichiers du cache ne changent jamais.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Nom de fichier de la vignette d'un identifiant, sans le dossier.
pub fn hash_id(id: &str) -> String {
    format!("{:016x}.png", fnv1a(id))
}

pub fn thumb_path(cfg: &Config, id: &str) -> PathBuf {
    cfg.thumbs_dir.join(hash_id(id))
}

/// Charge une vignette pour l'affichage.
pub fn load(path: &Path, px: i32) -> Option<Pixbuf> {
    Pixbuf::from_file_at_scale(path, px, px, true).ok()
}

/// Compteur indiquant qu'une passe de génération est déjà en cours.
#[derive(Clone, Default)]
pub struct GenerationState(Arc<AtomicBool>);

impl GenerationState {
    /// Tente de réserver la passe. Renvoie `false` si une passe tourne déjà.
    fn try_start(&self) -> bool {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    fn finish(&self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Génère en parallèle les vignettes manquantes.
///
/// `on_progress` est appelé régulièrement (pour rafraîchir l'affichage au fil
/// de l'eau) et une dernière fois à la fin. Le travail suit l'ordre de `items`
/// — donc l'ordre affiché — pour que le haut de la liste soit prêt en premier.
pub fn generate_missing<F>(cfg: Config, items: Vec<Wallpaper>, state: GenerationState, on_progress: F)
where
    F: Fn() + Send + Sync + 'static,
{
    if !state.try_start() {
        return;
    }

    let jobs: Vec<(String, PathBuf)> = items
        .iter()
        .filter_map(|w| {
            let dest = thumb_path(&cfg, &w.id);
            if dest.is_file() {
                return None;
            }
            let src = w.thumb_source()?.to_path_buf();
            if !src.is_file() {
                return None;
            }
            Some((w.id.clone(), src))
        })
        .collect();

    if jobs.is_empty() {
        state.finish();
        return;
    }

    let done = AtomicUsize::new(0);
    let total = jobs.len();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    let next = AtomicUsize::new(0);
    let progress = Arc::new(on_progress);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let (id, src) = &jobs[i];
                let dest = thumb_path(&cfg, id);
                if generate_one(src, &dest, cfg.thumb_cache_px) {
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    // Rafraîchissement par paquets : les vignettes apparaissent
                    // progressivement sans reconstruire la liste à chaque fois.
                    if n.is_multiple_of(8) || n == total {
                        progress();
                    }
                }
            });
        }
    });

    progress();
    state.finish();
}

fn generate_one(src: &Path, dest: &Path, size: i32) -> bool {
    if dest.is_file() {
        return false;
    }
    let Ok(pb) = Pixbuf::from_file_at_scale(src, size, size, true) else {
        return false;
    };
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = dest.with_extension("part");
    match pb.savev(&tmp, "png", &[]) {
        Ok(()) => std::fs::rename(&tmp, dest).is_ok(),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

/// Supprime tout le cache (utilisé par `--rebuild-cache`).
pub fn clear(cfg: &Config) -> std::io::Result<usize> {
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(&cfg.thumbs_dir) {
        for e in entries.flatten() {
            if std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}
