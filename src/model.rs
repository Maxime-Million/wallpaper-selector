//! Le modèle de données : un `Wallpaper` par entrée listée.
//!
//! Chaque entrée porte un booléen `favorite` — c'est la variable « favori :
//! oui/non » demandée. Les favoris sont remontés en tête de liste dans une
//! section dédiée au moment de construire les lignes (`ui::build_rows`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Steam,
    Perso,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Steam => "steam",
            Source::Perso => "perso",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Wallpaper Engine : un fichier `.pkg` est présent, il faut le moteur natif.
    Pkg,
    Video,
    Gif,
    Image,
}

impl Kind {
    pub fn badge(self) -> &'static str {
        match self {
            Kind::Pkg => "PKG",
            Kind::Video => "MP4",
            Kind::Gif => "GIF",
            Kind::Image => "IMG",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Kind> {
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "webm" | "mkv" | "mov" => Some(Kind::Video),
            "gif" => Some(Kind::Gif),
            "jpg" | "jpeg" | "png" | "webp" | "avif" | "bmp" | "tiff" | "tif" => Some(Kind::Image),
            _ => None,
        }
    }

    /// Extensions acceptées pour les fichiers du dossier personnel.
    pub fn is_image_extension(ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "avif" | "bmp" | "tiff" | "tif" | "gif"
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallpaper {
    /// Identifiant stable et unique : `steam:<id>` ou `perso:<fichier>`.
    /// C'est ce qui est écrit dans le fichier de favoris.
    pub id: String,
    pub title: String,
    /// Dossier (Steam) ou fichier (perso).
    pub path: PathBuf,
    /// Image utilisée pour la vignette (preview.jpg pour Steam).
    pub preview: Option<PathBuf>,
    /// Média appliqué au fond d'écran (non utilisé pour les `.pkg`).
    pub media: Option<PathBuf>,
    pub kind: Kind,
    pub source: Source,
    /// Extension réelle en minuscules (`png`, `mp4`, `pkg`…). C'est elle que
    /// cible la recherche par type (`#png`, `#pkg`, `#video`).
    pub ext: String,
    /// Variable « favori : oui/non ».
    pub favorite: bool,
}

impl Wallpaper {
    /// Fichier de prévisualisation (vignette) de cette entrée.
    pub fn thumb_source(&self) -> Option<&Path> {
        self.preview
            .as_deref()
            .or(self.media.as_deref())
            .or(Some(self.path.as_path()))
    }

    /// Sous-titre affiché sous le titre dans la liste.
    pub fn subtitle(&self) -> String {
        let extra = if self.source == Source::Steam {
            self.id.trim_start_matches("steam:")
        } else {
            "perso"
        };
        let ext = if self.ext.is_empty() {
            self.kind.badge().to_ascii_lowercase()
        } else {
            self.ext.clone()
        };
        format!("{extra} · {ext} · {}", self.source.label())
    }

    /// Vrai si l'entrée correspond à un jeton de type (`png`, `pkg`, `video`…).
    pub fn matches_type(&self, token: &str) -> bool {
        if self.ext == token {
            return true;
        }
        match token {
            "video" | "videos" => matches!(self.ext.as_str(), "mp4" | "webm" | "mkv" | "mov"),
            "image" | "img" | "photo" => matches!(
                self.ext.as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "avif" | "bmp" | "tiff" | "tif"
            ),
            "jpeg" => self.ext == "jpg" || self.ext == "jpeg",
            "anim" | "animation" => self.ext == "gif",
            "scene" | "we" => self.ext == "pkg",
            "perso" | "local" => self.source == Source::Perso,
            "steam" | "workshop" => self.source == Source::Steam,
            _ => false,
        }
    }
}

/// Filtres proposés par `Ctrl+T`, dans l'ordre de rotation.
pub const TYPE_CYCLE: [&str; 6] = ["pkg", "mp4", "gif", "img", "perso", "steam"];

/// Trie les entrées : favoris d'abord, puis ordre alphabétique insensible à la casse.
pub fn sort_for_display(list: &mut [Wallpaper]) {
    list.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
}
