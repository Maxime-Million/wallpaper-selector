//! Persistance des favoris.
//!
//! Format volontairement lisible et éditable à la main, une entrée par ligne :
//!
//! ```text
//! # Wallpaper Selector — favoris
//! steam:1355236618      → favori : oui
//! perso:neocity.png     → favori : oui
//! !perso:kirokaze.gif   → favori : non (explicite, prioritaire)
//! ```
//!
//! Les identifiants sont ceux de [`crate::model::Wallpaper::id`].

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Favorites {
    yes: HashSet<String>,
    no: HashSet<String>,
    path: PathBuf,
}

impl Favorites {
    pub fn load(path: &Path) -> Self {
        let mut f = Favorites {
            yes: HashSet::new(),
            no: HashSet::new(),
            path: path.to_path_buf(),
        };
        if let Ok(text) = std::fs::read_to_string(path) {
            f.parse(&text);
        }
        f
    }

    fn parse(&mut self, text: &str) {
        self.yes.clear();
        self.no.clear();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(id) = line.strip_prefix('!') {
                self.no.insert(id.trim().to_string());
            } else {
                self.yes.insert(line.to_string());
            }
        }
    }

    /// Relit le fichier depuis le disque.
    ///
    /// Indispensable pour le processus résident : `wallpaper-selector
    /// --favorite <id>` peut modifier le fichier pendant qu'il tourne, et sans
    /// cette relecture la prochaine épinglette écraserait la modification avec
    /// l'état en mémoire.
    pub fn reload(&mut self) {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            self.parse(&text);
        }
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.yes.contains(id) && !self.no.contains(id)
    }

    /// Met à jour l'état et renvoie le nombre de favoris.
    pub fn set(&mut self, id: &str, favorite: bool) {
        if favorite {
            self.no.remove(id);
            self.yes.insert(id.to_string());
        } else {
            self.yes.remove(id);
        }
    }

    pub fn len(&self) -> usize {
        self.yes.len()
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut yes: Vec<&String> = self.yes.iter().collect();
        yes.sort();
        let mut no: Vec<&String> = self.no.iter().collect();
        no.sort();

        let mut out = String::from("# Wallpaper Selector — favoris\n");
        out.push_str("# Une ligne = un identifiant. Prefixer par '!' force « non favori ».\n");
        out.push_str("# Les identifiants sont des dossiers Steam ou des fichiers perso.\n\n");
        for id in yes {
            out.push_str(id);
            out.push('\n');
        }
        for id in no {
            out.push('!');
            out.push_str(id);
            out.push('\n');
        }

        // Écriture atomique : on ne veut jamais corrompre le fichier de favoris.
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, &self.path)
    }
}
