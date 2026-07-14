//! Modèles métier partagés par les composants de ClipH.

use std::path::PathBuf;

pub mod classifier;

pub use classifier::{ClassificationResult, ClipboardClassification, classify_text};

/// Représentation MIME binaire conservée pour un élément textuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFormatPayload {
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl ClipboardFormatPayload {
    pub fn new(mime_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            mime_type: mime_type.into(),
            data,
        }
    }

    pub fn byte_size(&self) -> usize {
        self.data.len()
    }
}

/// Type technique principal d'un élément enregistré.
///
/// La classification sémantique est conservée séparément dans
/// `ClipboardItem::classification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardItemKind {
    Text,
    Image,
    Files,
}

impl ClipboardItemKind {
    /// Valeur conservée dans SQLite.
    pub const fn as_database_value(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Files => "files",
        }
    }

    /// Convertit une valeur SQLite en type métier.
    pub fn from_database_value(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "image" => Some(Self::Image),
            "files" => Some(Self::Files),
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

    /// Classification sémantique calculée par le moteur ClipH.
    pub classification: ClipboardClassification,

    /// Langage, protocole, format de tableau ou autre précision.
    pub classification_subtype: Option<String>,

    /// Niveau de confiance du classificateur, compris entre 0 et 100.
    pub classification_confidence: u8,

    /// Représentations MIME connues pour cet élément.
    pub mime_types: Vec<String>,

    /// Texte simple. Cette chaîne est vide pour une image.
    pub plain_text: String,

    /// Représentation HTML facultative.
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

    pub const fn is_files(&self) -> bool {
        matches!(self.kind, ClipboardItemKind::Files)
    }

    /// Libellé principal destiné à l'interface.
    pub const fn classification_label(&self) -> &'static str {
        self.classification.display_label()
    }
}
