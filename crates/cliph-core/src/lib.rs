//! Modèles métier partagés par les différents composants de ClipH.

/// Type de contenu enregistré dans l'historique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardItemKind {
    Text,
}

impl ClipboardItemKind {
    /// Représentation utilisée dans la base de données.
    pub const fn as_database_value(self) -> &'static str {
        match self {
            Self::Text => "text",
        }
    }
}

/// Élément présent dans l'historique du presse-papiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ClipboardItemKind,
    pub plain_text: String,
    pub created_at_ns: i64,
    pub last_used_at_ns: i64,
    pub is_pinned: bool,
}
