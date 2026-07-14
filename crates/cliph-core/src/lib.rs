//! Modèles métier partagés par les composants de ClipH.

use std::path::PathBuf;

/// Type principal d'un élément enregistré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardItemKind {
    Text,
    Image,
}

impl ClipboardItemKind {
    /// Valeur conservée dans SQLite.
    pub const fn as_database_value(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }

    /// Convertit une valeur SQLite en type métier.
    pub fn from_database_value(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

/// Informations associées à une image persistante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePayload {
    /// Chemin absolu du fichier PNG conservé par ClipH.
    pub path: PathBuf,

    pub mime_type: String,
    pub width: i32,
    pub height: i32,
    pub byte_size: u64,
}

/// Élément présent dans l'historique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ClipboardItemKind,

    /// Texte simple. Cette chaîne est vide pour une image.
    pub plain_text: String,

    /// Représentation HTML facultative d'un texte enrichi.
    pub html_text: Option<String>,

    /// Informations de l'image lorsque `kind == Image`.
    pub image: Option<ImagePayload>,

    pub created_at_ns: i64,
    pub last_used_at_ns: i64,
    pub is_pinned: bool,
}

impl ClipboardItem {
    pub fn has_rich_text(&self) -> bool {
        self.kind == ClipboardItemKind::Text
            && self
                .html_text
                .as_deref()
                .is_some_and(|html| !html.trim().is_empty())
    }

    pub const fn is_image(&self) -> bool {
        matches!(self.kind, ClipboardItemKind::Image)
    }
}
