//! Modèles métier partagés par les composants de ClipH.

use std::path::{Path, PathBuf};

pub mod classifier;

pub mod quick_insert;
pub use classifier::{ClassificationResult, ClipboardClassification, classify_text};

pub use quick_insert::{
    QuickInsertCategory, QuickInsertEntry, catalog, entries_for_category, find_entry_by_value,
    groups_for_category, search_entries,
};

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

/// Action initiale associée à une sélection de fichiers.
///
/// ClipH conserve cette information pour représenter fidèlement ce qui a été
/// copié. L'interface pourra toutefois restaurer un ancien élément en mode
/// copie afin d'éviter un déplacement involontaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTransferOperation {
    Copy,
    Cut,
}

impl FileTransferOperation {
    pub const fn as_database_value(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Cut => "cut",
        }
    }

    pub fn from_database_value(value: &str) -> Option<Self> {
        match value {
            "copy" => Some(Self::Copy),
            "cut" => Some(Self::Cut),
            _ => None,
        }
    }
}

/// Référence vers un fichier ou un dossier copié.
///
/// ClipH ne duplique pas le contenu du fichier : il conserve son URI et ses
/// métadonnées. Le chemin peut donc devenir indisponible après un déplacement
/// ou une suppression externe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePayload {
    pub uri: String,
    pub path: Option<PathBuf>,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub byte_size: Option<u64>,
    pub is_directory: bool,
    pub existed_at_capture: bool,
}

impl FilePayload {
    pub fn new(
        uri: impl Into<String>,
        path: Option<PathBuf>,
        display_name: impl Into<String>,
    ) -> Self {
        Self {
            uri: uri.into(),
            path,
            display_name: display_name.into(),
            mime_type: None,
            byte_size: None,
            is_directory: false,
            existed_at_capture: true,
        }
    }

    /// Indique si la cible locale existe encore.
    ///
    /// Pour une URI non locale, l'information n'est pas vérifiable ici.
    pub fn exists_now(&self) -> Option<bool> {
        self.path.as_deref().map(Path::exists)
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

    /// Texte simple. Pour un groupe de fichiers, il contient leurs noms.
    pub plain_text: String,

    /// Représentation HTML facultative.
    pub html_text: Option<String>,

    /// Informations de l'image lorsque `kind == Image`.
    pub image: Option<ImagePayload>,

    /// Action associée aux fichiers lorsque `kind == Files`.
    pub file_transfer_operation: Option<FileTransferOperation>,

    /// Fichiers et dossiers lorsque `kind == Files`.
    pub files: Vec<FilePayload>,

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
