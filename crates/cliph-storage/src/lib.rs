//! Stockage persistant de l'historique ClipH.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use cliph_core::{
    ClipboardClassification, ClipboardFormatPayload, ClipboardItem, ClipboardItemKind,
    ImagePayload, classify_text,
};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

const CURRENT_SCHEMA_VERSION: i64 = 5;

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");
const RICH_TEXT_MIGRATION: &str = include_str!("../migrations/0002_rich_text.sql");
const IMAGE_HISTORY_MIGRATION: &str = include_str!("../migrations/0003_image_history.sql");
const CLASSIFICATION_MIGRATION: &str =
    include_str!("../migrations/0004_classification_metadata.sql");
const MULTIFORMAT_MIGRATION: &str = include_str!("../migrations/0005_multiformat_payloads.sql");

/// Limite de sécurité actuelle : 25 Mio par image.
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// Limite par représentation textuelle riche : 4 Mio.
pub const MAX_FORMAT_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

/// Limite cumulée des représentations d'un même élément : 12 Mio.
pub const MAX_TOTAL_FORMAT_PAYLOAD_BYTES: usize = 12 * 1024 * 1024;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug)]
pub enum StorageError {
    ProjectDirectoryUnavailable,
    Io(std::io::Error),
    Database(rusqlite::Error),
    SystemTime(SystemTimeError),
    TimestampOverflow,
    UnsupportedSchemaVersion(i64),
    UnknownItemKind(String),
    UnknownClassification(String),
    InvalidClassificationConfidence(i64),
    InvalidImageDimensions {
        width: i32,
        height: i32,
    },
    EmptyImage,
    ImageTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidStoredByteSize(i64),
    FormatPayloadTooLarge {
        mime_type: String,
        actual: usize,
        maximum: usize,
    },
    TotalFormatPayloadsTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidStoredFormatByteSize(i64),
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectDirectoryUnavailable => write!(
                formatter,
                "le répertoire de données de l'utilisateur est indisponible"
            ),
            Self::Io(error) => write!(formatter, "erreur d'accès au disque : {error}"),
            Self::Database(error) => {
                write!(formatter, "erreur de base de données : {error}")
            }
            Self::SystemTime(error) => {
                write!(formatter, "erreur liée à l'horloge système : {error}")
            }
            Self::TimestampOverflow => {
                write!(formatter, "la date système dépasse la capacité de stockage")
            }
            Self::UnsupportedSchemaVersion(version) => write!(
                formatter,
                "version de base de données non prise en charge : {version}"
            ),
            Self::UnknownItemKind(kind) => {
                write!(formatter, "type d'élément inconnu dans la base : {kind}")
            }
            Self::UnknownClassification(classification) => write!(
                formatter,
                "classification inconnue dans la base : {classification}"
            ),
            Self::InvalidClassificationConfidence(confidence) => write!(
                formatter,
                "niveau de confiance invalide dans la base : {confidence}"
            ),
            Self::InvalidImageDimensions { width, height } => write!(
                formatter,
                "dimensions d'image invalides : {width} × {height}"
            ),
            Self::EmptyImage => write!(formatter, "l'image ne contient aucune donnée"),
            Self::ImageTooLarge { actual, maximum } => write!(
                formatter,
                "l'image fait {actual} octets, limite autorisée : {maximum} octets"
            ),
            Self::InvalidStoredByteSize(size) => {
                write!(formatter, "taille d'image invalide dans la base : {size}")
            }
            Self::FormatPayloadTooLarge {
                mime_type,
                actual,
                maximum,
            } => write!(
                formatter,
                "le format {mime_type} fait {actual} octets, limite autorisée : {maximum} octets"
            ),
            Self::TotalFormatPayloadsTooLarge { actual, maximum } => write!(
                formatter,
                "les formats textuels font {actual} octets au total, limite autorisée : {maximum} octets"
            ),
            Self::InvalidStoredFormatByteSize(size) => write!(
                formatter,
                "taille de représentation MIME invalide dans la base : {size}"
            ),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<SystemTimeError> for StorageError {
    fn from(error: SystemTimeError) -> Self {
        Self::SystemTime(error)
    }
}

/// Point d'accès au stockage SQLite et aux fichiers binaires.
pub struct ClipboardStorage {
    connection: Connection,
    database_path: PathBuf,
    data_directory: PathBuf,
    images_directory: PathBuf,
}

impl ClipboardStorage {
    /// Ouvre la base standard de ClipH.
    pub fn open_default() -> StorageResult<Self> {
        let project_dirs = ProjectDirs::from("com", "ClipH", "ClipH")
            .ok_or(StorageError::ProjectDirectoryUnavailable)?;

        let data_directory = project_dirs.data_dir();
        fs::create_dir_all(data_directory)?;

        Self::open(data_directory.join("cliph.db"))
    }

    /// Ouvre une base précise.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let database_path = path.as_ref().to_path_buf();

        let data_directory = database_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or(StorageError::ProjectDirectoryUnavailable)?;

        let images_directory = data_directory.join("blobs").join("images");
        fs::create_dir_all(&images_directory)?;

        let connection = Connection::open(&database_path)?;

        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
            ",
        )?;

        let storage = Self {
            connection,
            database_path,
            data_directory,
            images_directory,
        };

        storage.run_migrations()?;

        Ok(storage)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    pub fn images_directory(&self) -> &Path {
        &self.images_directory
    }

    /// Enregistre un texte simple.
    pub fn save_text(&self, plain_text: &str) -> StorageResult<ClipboardItem> {
        self.save_text_with_payloads(plain_text, None, &[String::from("text/plain")], &[])
    }

    /// Enregistre un texte et son éventuel HTML.
    pub fn save_rich_text(
        &self,
        plain_text: &str,
        html_text: Option<&str>,
    ) -> StorageResult<ClipboardItem> {
        let mut mime_types = vec![String::from("text/plain")];
        let mut payloads = Vec::new();

        if let Some(html_text) = html_text.filter(|html| !html.trim().is_empty()) {
            mime_types.push(String::from("text/html"));
            payloads.push(ClipboardFormatPayload::new(
                "text/html",
                html_text.as_bytes().to_vec(),
            ));
        }

        self.save_text_with_payloads(plain_text, html_text, &mime_types, &payloads)
    }

    /// Compatibilité avec l'API précédente.
    pub fn save_text_with_formats(
        &self,
        plain_text: &str,
        html_text: Option<&str>,
        mime_types: &[String],
    ) -> StorageResult<ClipboardItem> {
        let payloads = html_text
            .filter(|html| !html.trim().is_empty())
            .map(|html| {
                vec![ClipboardFormatPayload::new(
                    "text/html",
                    html.as_bytes().to_vec(),
                )]
            })
            .unwrap_or_default();

        self.save_text_with_payloads(plain_text, html_text, mime_types, &payloads)
    }

    /// Enregistre le texte et ses représentations HTML, RTF, TSV ou CSV.
    pub fn save_text_with_payloads(
        &self,
        plain_text: &str,
        html_text: Option<&str>,
        mime_types: &[String],
        payloads: &[ClipboardFormatPayload],
    ) -> StorageResult<ClipboardItem> {
        let normalized_html = html_text.filter(|html| !html.trim().is_empty());
        let normalized_payloads = normalize_format_payloads(payloads)?;
        let normalized_mime_types =
            normalize_text_mime_types(mime_types, normalized_html.is_some(), &normalized_payloads);

        let classification = classify_text(plain_text, normalized_html, &normalized_mime_types);

        let content_hash = calculate_text_hash(plain_text, normalized_html, &normalized_payloads);
        let now = current_timestamp_ns()?;

        self.connection.execute(
            "
            INSERT INTO clipboard_items (
                kind,
                plain_text,
                html_text,
                content_hash,
                created_at_ns,
                last_used_at_ns,
                is_pinned,
                classification,
                classification_subtype,
                classification_confidence
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?5, 0, ?6, ?7, ?8)

            ON CONFLICT(content_hash) DO UPDATE SET
                plain_text = excluded.plain_text,
                html_text = COALESCE(
                    excluded.html_text,
                    clipboard_items.html_text
                ),
                last_used_at_ns = excluded.last_used_at_ns,
                classification = excluded.classification,
                classification_subtype = excluded.classification_subtype,
                classification_confidence = excluded.classification_confidence
            ",
            params![
                ClipboardItemKind::Text.as_database_value(),
                plain_text,
                normalized_html,
                content_hash,
                now,
                classification.classification.as_database_value(),
                classification.subtype.as_deref(),
                i64::from(classification.confidence),
            ],
        )?;

        let item_id = self.item_id_by_hash(&content_hash)?;
        self.merge_item_mime_types(item_id, &normalized_mime_types)?;
        self.merge_format_payloads(item_id, &normalized_payloads)?;

        self.find_by_hash(&content_hash)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows.into())
    }

    /// Enregistre une image déjà encodée au format PNG.
    pub fn save_image_png(
        &self,
        png_bytes: &[u8],
        width: i32,
        height: i32,
    ) -> StorageResult<ClipboardItem> {
        if png_bytes.is_empty() {
            return Err(StorageError::EmptyImage);
        }

        if png_bytes.len() > MAX_IMAGE_BYTES {
            return Err(StorageError::ImageTooLarge {
                actual: png_bytes.len(),
                maximum: MAX_IMAGE_BYTES,
            });
        }

        if width <= 0 || height <= 0 {
            return Err(StorageError::InvalidImageDimensions { width, height });
        }

        let content_hash = calculate_image_hash(png_bytes);

        let relative_path = PathBuf::from("blobs")
            .join("images")
            .join(format!("{content_hash}.png"));

        let absolute_path = self.data_directory.join(&relative_path);
        write_blob_if_missing(&absolute_path, png_bytes)?;

        let now = current_timestamp_ns()?;

        self.connection.execute(
            "
            INSERT INTO clipboard_items (
                kind,
                plain_text,
                html_text,
                content_hash,
                created_at_ns,
                last_used_at_ns,
                is_pinned,
                classification,
                classification_subtype,
                classification_confidence
            )
            VALUES (?1, '', NULL, ?2, ?3, ?3, 0, 'image', 'PNG', 100)

            ON CONFLICT(content_hash) DO UPDATE SET
                last_used_at_ns = excluded.last_used_at_ns,
                classification = excluded.classification,
                classification_subtype = excluded.classification_subtype,
                classification_confidence = excluded.classification_confidence
            ",
            params![
                ClipboardItemKind::Image.as_database_value(),
                content_hash,
                now,
            ],
        )?;

        let item_id = self.item_id_by_hash(&content_hash)?;

        let relative_path_string = relative_path.to_string_lossy().into_owned();

        let byte_size =
            i64::try_from(png_bytes.len()).map_err(|_| StorageError::ImageTooLarge {
                actual: png_bytes.len(),
                maximum: MAX_IMAGE_BYTES,
            })?;

        self.connection.execute(
            "
            INSERT INTO image_payloads (
                item_id,
                relative_path,
                mime_type,
                width,
                height,
                byte_size
            )
            VALUES (?1, ?2, 'image/png', ?3, ?4, ?5)

            ON CONFLICT(item_id) DO UPDATE SET
                relative_path = excluded.relative_path,
                mime_type = excluded.mime_type,
                width = excluded.width,
                height = excluded.height,
                byte_size = excluded.byte_size
            ",
            params![item_id, relative_path_string, width, height, byte_size,],
        )?;

        self.merge_item_mime_types(item_id, &[String::from("image/png")])?;

        self.find_by_hash(&content_hash)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows.into())
    }

    /// Retourne les éléments récents.
    pub fn list_recent(&self, limit: usize) -> StorageResult<Vec<ClipboardItem>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        let mut statement = self.connection.prepare(
            "
            SELECT
                clipboard_items.id,
                clipboard_items.kind,
                clipboard_items.plain_text,
                clipboard_items.html_text,
                clipboard_items.created_at_ns,
                clipboard_items.last_used_at_ns,
                clipboard_items.is_pinned,
                image_payloads.relative_path,
                image_payloads.mime_type,
                image_payloads.width,
                image_payloads.height,
                image_payloads.byte_size,
                clipboard_items.classification,
                clipboard_items.classification_subtype,
                clipboard_items.classification_confidence,
                COALESCE((
                    SELECT GROUP_CONCAT(
                        clipboard_item_mime_types.mime_type,
                        CHAR(31)
                    )
                    FROM clipboard_item_mime_types
                    WHERE clipboard_item_mime_types.item_id =
                          clipboard_items.id
                ), '')
            FROM clipboard_items
            LEFT JOIN image_payloads
                ON image_payloads.item_id = clipboard_items.id
            ORDER BY
                clipboard_items.is_pinned DESC,
                clipboard_items.last_used_at_ns DESC,
                clipboard_items.id DESC
            LIMIT ?1
            ",
        )?;

        let data_directory = self.data_directory.clone();

        let items = statement
            .query_map([limit], move |row| map_clipboard_item(row, &data_directory))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    pub fn count(&self) -> StorageResult<usize> {
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))?;

        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Supprime un élément et son fichier image éventuel.
    pub fn delete_item(&self, item_id: i64) -> StorageResult<bool> {
        let image_relative_path: Option<String> = self
            .connection
            .query_row(
                "
                SELECT relative_path
                FROM image_payloads
                WHERE item_id = ?1
                ",
                [item_id],
                |row| row.get(0),
            )
            .optional()?;

        let deleted_count = self.connection.execute(
            "
            DELETE FROM clipboard_items
            WHERE id = ?1
            ",
            [item_id],
        )?;

        if deleted_count > 0
            && let Some(relative_path) = image_relative_path
        {
            remove_blob_if_present(&self.data_directory.join(relative_path))?;
        }

        Ok(deleted_count > 0)
    }

    /// Supprime tout l'historique et les fichiers d'images.
    pub fn clear_history(&self) -> StorageResult<usize> {
        let mut statement = self
            .connection
            .prepare("SELECT relative_path FROM image_payloads")?;

        let relative_paths = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        drop(statement);

        let deleted_count = self.connection.execute("DELETE FROM clipboard_items", [])?;

        for relative_path in relative_paths {
            remove_blob_if_present(&self.data_directory.join(relative_path))?;
        }

        Ok(deleted_count)
    }

    fn item_id_by_hash(&self, content_hash: &str) -> StorageResult<i64> {
        self.connection
            .query_row(
                "
                SELECT id
                FROM clipboard_items
                WHERE content_hash = ?1
                ",
                [content_hash],
                |row| row.get(0),
            )
            .map_err(StorageError::from)
    }

    fn merge_item_mime_types(&self, item_id: i64, mime_types: &[String]) -> StorageResult<()> {
        for mime_type in mime_types {
            self.connection.execute(
                "
                INSERT OR IGNORE INTO clipboard_item_mime_types (
                    item_id,
                    mime_type
                )
                VALUES (?1, ?2)
                ",
                params![item_id, mime_type],
            )?;
        }

        Ok(())
    }

    fn merge_format_payloads(
        &self,
        item_id: i64,
        payloads: &[ClipboardFormatPayload],
    ) -> StorageResult<()> {
        for payload in payloads {
            let byte_size = i64::try_from(payload.data.len()).map_err(|_| {
                StorageError::FormatPayloadTooLarge {
                    mime_type: payload.mime_type.clone(),
                    actual: payload.data.len(),
                    maximum: MAX_FORMAT_PAYLOAD_BYTES,
                }
            })?;

            self.connection.execute(
                "
                INSERT INTO clipboard_item_format_payloads (
                    item_id,
                    mime_type,
                    data,
                    byte_size
                )
                VALUES (?1, ?2, ?3, ?4)

                ON CONFLICT(item_id, mime_type) DO UPDATE SET
                    data = excluded.data,
                    byte_size = excluded.byte_size
                ",
                params![item_id, &payload.mime_type, &payload.data, byte_size,],
            )?;
        }

        Ok(())
    }

    /// Charge les représentations binaires nécessaires à une restauration fidèle.
    pub fn load_format_payloads(&self, item_id: i64) -> StorageResult<Vec<ClipboardFormatPayload>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                mime_type,
                data,
                byte_size
            FROM clipboard_item_format_payloads
            WHERE item_id = ?1
            ORDER BY mime_type ASC
            ",
        )?;

        let payloads = statement
            .query_map([item_id], |row| {
                let mime_type: String = row.get(0)?;
                let data: Vec<u8> = row.get(1)?;
                let stored_byte_size: i64 = row.get(2)?;

                let expected_byte_size =
                    usize::try_from(stored_byte_size).map_err(|_| {
                        conversion_error(
                            2,
                            rusqlite::types::Type::Integer,
                            StorageError::InvalidStoredFormatByteSize(
                                stored_byte_size,
                            )
                            .to_string(),
                        )
                    })?;

                if expected_byte_size != data.len() {
                    return Err(conversion_error(
                        2,
                        rusqlite::types::Type::Integer,
                        format!(
                            "taille incohérente pour {mime_type} : base={expected_byte_size}, données={}",
                            data.len()
                        ),
                    ));
                }

                Ok(ClipboardFormatPayload { mime_type, data })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(payloads)
    }

    fn find_by_hash(&self, content_hash: &str) -> StorageResult<Option<ClipboardItem>> {
        let data_directory = self.data_directory.clone();

        let item = self
            .connection
            .query_row(
                "
                SELECT
                    clipboard_items.id,
                    clipboard_items.kind,
                    clipboard_items.plain_text,
                    clipboard_items.html_text,
                    clipboard_items.created_at_ns,
                    clipboard_items.last_used_at_ns,
                    clipboard_items.is_pinned,
                    image_payloads.relative_path,
                    image_payloads.mime_type,
                    image_payloads.width,
                    image_payloads.height,
                    image_payloads.byte_size,
                    clipboard_items.classification,
                    clipboard_items.classification_subtype,
                    clipboard_items.classification_confidence,
                    COALESCE((
                        SELECT GROUP_CONCAT(
                            clipboard_item_mime_types.mime_type,
                            CHAR(31)
                        )
                        FROM clipboard_item_mime_types
                        WHERE clipboard_item_mime_types.item_id =
                              clipboard_items.id
                    ), '')
                FROM clipboard_items
                LEFT JOIN image_payloads
                    ON image_payloads.item_id = clipboard_items.id
                WHERE clipboard_items.content_hash = ?1
                ",
                [content_hash],
                move |row| map_clipboard_item(row, &data_directory),
            )
            .optional()?;

        Ok(item)
    }

    fn run_migrations(&self) -> StorageResult<()> {
        let mut schema_version: i64 =
            self.connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if schema_version > CURRENT_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion(schema_version));
        }

        if schema_version == 0 {
            self.connection.execute_batch(INITIAL_MIGRATION)?;
            schema_version = 1;
        }

        if schema_version == 1 {
            self.connection.execute_batch(RICH_TEXT_MIGRATION)?;
            schema_version = 2;
        }

        if schema_version == 2 {
            self.connection.execute_batch(IMAGE_HISTORY_MIGRATION)?;
            schema_version = 3;
        }

        if schema_version == 3 {
            self.connection.execute_batch(CLASSIFICATION_MIGRATION)?;
            schema_version = 4;
        }

        if schema_version == 4 {
            self.connection.execute_batch(MULTIFORMAT_MIGRATION)?;
            schema_version = 5;
        }

        if schema_version != CURRENT_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion(schema_version));
        }

        Ok(())
    }
}

fn map_clipboard_item(
    row: &rusqlite::Row<'_>,
    data_directory: &Path,
) -> rusqlite::Result<ClipboardItem> {
    let kind_value: String = row.get(1)?;
    let kind = ClipboardItemKind::from_database_value(&kind_value).ok_or_else(|| {
        conversion_error(
            1,
            rusqlite::types::Type::Text,
            StorageError::UnknownItemKind(kind_value).to_string(),
        )
    })?;

    let is_pinned: i64 = row.get(6)?;
    let relative_path: Option<String> = row.get(7)?;

    let image = match relative_path {
        Some(relative_path) => {
            let stored_byte_size: i64 = row.get(11)?;
            let byte_size = u64::try_from(stored_byte_size).map_err(|_| {
                conversion_error(
                    11,
                    rusqlite::types::Type::Integer,
                    StorageError::InvalidStoredByteSize(stored_byte_size).to_string(),
                )
            })?;

            Some(ImagePayload {
                path: data_directory.join(relative_path),
                mime_type: row.get(8)?,
                width: row.get(9)?,
                height: row.get(10)?,
                byte_size,
            })
        }
        None => None,
    };

    let classification_value: String = row.get(12)?;
    let classification = ClipboardClassification::from_database_value(&classification_value)
        .ok_or_else(|| {
            conversion_error(
                12,
                rusqlite::types::Type::Text,
                StorageError::UnknownClassification(classification_value).to_string(),
            )
        })?;

    let stored_confidence: i64 = row.get(14)?;
    let classification_confidence = u8::try_from(stored_confidence).map_err(|_| {
        conversion_error(
            14,
            rusqlite::types::Type::Integer,
            StorageError::InvalidClassificationConfidence(stored_confidence).to_string(),
        )
    })?;

    if classification_confidence > 100 {
        return Err(conversion_error(
            14,
            rusqlite::types::Type::Integer,
            StorageError::InvalidClassificationConfidence(stored_confidence).to_string(),
        ));
    }

    let mime_types_joined: String = row.get(15)?;
    let mime_types = if mime_types_joined.is_empty() {
        Vec::new()
    } else {
        mime_types_joined
            .split('\u{1f}')
            .map(str::to_owned)
            .collect()
    };

    Ok(ClipboardItem {
        id: row.get(0)?,
        kind,
        classification,
        classification_subtype: row.get(13)?,
        classification_confidence,
        mime_types,
        plain_text: row.get(2)?,
        html_text: row.get(3)?,
        image,
        created_at_ns: row.get(4)?,
        last_used_at_ns: row.get(5)?,
        is_pinned: is_pinned != 0,
    })
}

fn conversion_error(
    column: usize,
    data_type: rusqlite::types::Type,
    message: String,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        data_type,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )),
    )
}

fn normalize_text_mime_types(
    mime_types: &[String],
    contains_html: bool,
    payloads: &[ClipboardFormatPayload],
) -> Vec<String> {
    let mut normalized = mime_types
        .iter()
        .map(|mime_type| canonical_text_mime_type(mime_type))
        .filter(|mime_type| !mime_type.is_empty())
        .collect::<Vec<_>>();

    normalized.extend(payloads.iter().map(|payload| payload.mime_type.clone()));

    if !normalized
        .iter()
        .any(|mime_type| mime_type.starts_with("text/plain"))
    {
        normalized.push(String::from("text/plain"));
    }

    if contains_html && !normalized.iter().any(|mime_type| mime_type == "text/html") {
        normalized.push(String::from("text/html"));
    }

    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalize_format_payloads(
    payloads: &[ClipboardFormatPayload],
) -> StorageResult<Vec<ClipboardFormatPayload>> {
    let mut normalized: Vec<ClipboardFormatPayload> = Vec::new();
    let mut total_byte_size = 0_usize;

    for payload in payloads {
        let mime_type = canonical_text_mime_type(&payload.mime_type);

        if !is_persisted_text_payload_mime(&mime_type) || payload.data.is_empty() {
            continue;
        }

        if payload.data.len() > MAX_FORMAT_PAYLOAD_BYTES {
            return Err(StorageError::FormatPayloadTooLarge {
                mime_type,
                actual: payload.data.len(),
                maximum: MAX_FORMAT_PAYLOAD_BYTES,
            });
        }

        total_byte_size = total_byte_size.checked_add(payload.data.len()).ok_or(
            StorageError::TotalFormatPayloadsTooLarge {
                actual: usize::MAX,
                maximum: MAX_TOTAL_FORMAT_PAYLOAD_BYTES,
            },
        )?;

        if total_byte_size > MAX_TOTAL_FORMAT_PAYLOAD_BYTES {
            return Err(StorageError::TotalFormatPayloadsTooLarge {
                actual: total_byte_size,
                maximum: MAX_TOTAL_FORMAT_PAYLOAD_BYTES,
            });
        }

        if let Some(existing) = normalized
            .iter_mut()
            .find(|existing| existing.mime_type == mime_type)
        {
            existing.data.clone_from(&payload.data);
        } else {
            normalized.push(ClipboardFormatPayload {
                mime_type,
                data: payload.data.clone(),
            });
        }
    }

    normalized.sort_by(|left, right| left.mime_type.cmp(&right.mime_type));

    Ok(normalized)
}

fn canonical_text_mime_type(mime_type: &str) -> String {
    let mime_type = mime_type.trim().to_ascii_lowercase();

    if mime_type.starts_with("text/html") {
        return String::from("text/html");
    }

    if matches!(
        mime_type.as_str(),
        "text/rtf" | "application/rtf" | "application/x-rtf"
    ) {
        return String::from("text/rtf");
    }

    if mime_type == "text/tab-separated-values" {
        return mime_type;
    }

    if matches!(mime_type.as_str(), "text/csv" | "application/csv") {
        return String::from("text/csv");
    }

    if mime_type.starts_with("text/plain") {
        return String::from("text/plain");
    }

    mime_type
}

fn is_persisted_text_payload_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "text/html" | "text/rtf" | "text/tab-separated-values" | "text/csv"
    )
}

fn current_timestamp_ns() -> StorageResult<i64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;

    i64::try_from(duration.as_nanos()).map_err(|_| StorageError::TimestampOverflow)
}

fn calculate_text_hash(
    plain_text: &str,
    html_text: Option<&str>,
    payloads: &[ClipboardFormatPayload],
) -> String {
    let mut hasher = Sha256::new();

    hasher.update(b"text\0");
    hasher.update(plain_text.as_bytes());

    if let Some(html_text) = html_text {
        hasher.update(b"\0html\0");
        hasher.update(html_text.as_bytes());
    }

    for payload in payloads {
        if payload.mime_type == "text/html" && html_text.is_some() {
            continue;
        }

        hasher.update(b"\0mime\0");
        hasher.update(payload.mime_type.as_bytes());
        hasher.update(b"\0data\0");
        hasher.update(&payload.data);
    }

    format_digest(hasher.finalize())
}

fn calculate_image_hash(png_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();

    hasher.update(b"image/png\0");
    hasher.update(png_bytes);

    format_digest(hasher.finalize())
}

fn format_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_blob_if_missing(destination: &Path, content: &[u8]) -> StorageResult<()> {
    if destination.exists() {
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary_path =
        destination.with_extension(format!("png.tmp-{}", current_timestamp_ns()?,));

    fs::write(&temporary_path, content)?;

    match fs::rename(&temporary_path, destination) {
        Ok(()) => Ok(()),
        Err(_error) if destination.exists() => {
            remove_blob_if_present(&temporary_path)?;
            Ok(())
        }
        Err(error) => {
            remove_blob_if_present(&temporary_path)?;
            Err(StorageError::Io(error))
        }
    }
}

fn remove_blob_if_present(path: &Path) -> StorageResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process;

    use super::*;

    fn create_test_directory(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            process::id(),
            current_timestamp_ns().expect("timestamp"),
        ))
    }

    fn fake_png() -> Vec<u8> {
        vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4, 5, 6, 7, 8,
        ]
    }

    #[test]
    fn text_and_rich_text_are_preserved() {
        let directory = create_test_directory("cliph-text-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            storage
                .save_text("Texte simple")
                .expect("saving plain text");

            storage
                .save_rich_text("Texte enrichi", Some("<strong>Texte enrichi</strong>"))
                .expect("saving rich text");

            assert_eq!(storage.count().expect("counting items"), 2);
        }

        {
            let storage = ClipboardStorage::open(&database_path).expect("reopening database");

            let items = storage.list_recent(10).expect("listing items");

            assert_eq!(items.len(), 2);
            assert!(items.iter().any(ClipboardItem::has_rich_text));
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn classifications_and_mime_types_are_persistent() {
        let directory = create_test_directory("cliph-classification-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            let item = storage
                .save_text_with_formats(
                    "fn main() {\n    println!(\"Bonjour\");\n}",
                    None,
                    &[String::from("text/plain;charset=utf-8")],
                )
                .expect("saving classified code");

            assert_eq!(item.classification, ClipboardClassification::Code);
            assert_eq!(item.classification_subtype.as_deref(), Some("Rust"));
            assert!(
                item.mime_types
                    .iter()
                    .any(|mime_type| mime_type.starts_with("text/plain"))
            );
        }

        {
            let storage = ClipboardStorage::open(&database_path).expect("reopening database");

            let items = storage.list_recent(10).expect("listing items");

            assert_eq!(items.len(), 1);
            assert_eq!(items[0].classification, ClipboardClassification::Code);
            assert_eq!(items[0].classification_subtype.as_deref(), Some("Rust"));
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn link_classification_is_persistent() {
        let directory = create_test_directory("cliph-link-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            let item = storage
                .save_text_with_formats("https://example.com", None, &[String::from("text/plain")])
                .expect("saving link");

            assert_eq!(item.classification, ClipboardClassification::Link);
            assert_eq!(item.classification_subtype.as_deref(), Some("HTTPS"));
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn image_is_persistent_and_deduplicated() {
        let directory = create_test_directory("cliph-image-test");
        let database_path = directory.join("cliph.db");
        let png = fake_png();
        let stored_path;

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            let first = storage
                .save_image_png(&png, 640, 480)
                .expect("saving image");

            let second = storage
                .save_image_png(&png, 640, 480)
                .expect("saving duplicate");

            assert_eq!(first.id, second.id);
            assert_eq!(storage.count().expect("counting items"), 1);

            let payload = second.image.expect("image payload");

            assert_eq!(payload.width, 640);
            assert_eq!(payload.height, 480);
            assert_eq!(payload.byte_size, png.len() as u64);
            assert!(payload.path.exists());
            assert_eq!(second.classification, ClipboardClassification::Image);
            assert!(second.mime_types.contains(&String::from("image/png")));

            stored_path = payload.path;
        }

        {
            let storage = ClipboardStorage::open(&database_path).expect("reopening database");

            let items = storage.list_recent(10).expect("listing images");

            assert_eq!(items.len(), 1);
            assert!(items[0].is_image());
            assert!(stored_path.exists());
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn deleting_image_removes_its_file() {
        let directory = create_test_directory("cliph-delete-image-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            let item = storage
                .save_image_png(&fake_png(), 320, 240)
                .expect("saving image");

            let image_path = item.image.expect("image payload").path;
            assert!(image_path.exists());

            assert!(storage.delete_item(item.id).expect("deleting item"));

            assert!(!image_path.exists());
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn clearing_history_removes_images() {
        let directory = create_test_directory("cliph-clear-image-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            storage.save_text("Texte").expect("saving text");

            let image = storage
                .save_image_png(&fake_png(), 100, 100)
                .expect("saving image");

            let image_path = image.image.expect("image payload").path;

            assert_eq!(storage.clear_history().expect("clearing history"), 2);

            assert_eq!(storage.count().expect("counting items"), 0);
            assert!(!image_path.exists());
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }
    #[test]
    fn multiformat_payloads_are_persistent() {
        let directory = create_test_directory("cliph-multiformat-payload-test");
        let database_path = directory.join("cliph.db");

        let rtf = br"{\rtf1\ansi Bonjour ClipH}".to_vec();
        let tsv = b"Nom\tAge\nAda\t25".to_vec();

        let item_id;

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            let item = storage
                .save_text_with_payloads(
                    "Nom\tAge\nAda\t25",
                    None,
                    &[
                        String::from("text/plain"),
                        String::from("text/rtf"),
                        String::from("text/tab-separated-values"),
                    ],
                    &[
                        ClipboardFormatPayload::new("text/rtf", rtf.clone()),
                        ClipboardFormatPayload::new("text/tab-separated-values", tsv.clone()),
                    ],
                )
                .expect("saving multiformat text");

            item_id = item.id;

            assert_eq!(item.classification, ClipboardClassification::Table);
            assert_eq!(item.classification_subtype.as_deref(), Some("TSV"));
        }

        {
            let storage = ClipboardStorage::open(&database_path).expect("reopening database");

            let payloads = storage
                .load_format_payloads(item_id)
                .expect("loading payloads");

            assert!(
                payloads
                    .iter()
                    .any(|payload| { payload.mime_type == "text/rtf" && payload.data == rtf })
            );

            assert!(payloads.iter().any(|payload| {
                payload.mime_type == "text/tab-separated-values" && payload.data == tsv
            }));
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }

    #[test]
    fn payload_variants_create_distinct_items() {
        let directory = create_test_directory("cliph-payload-variant-test");
        let database_path = directory.join("cliph.db");

        {
            let storage = ClipboardStorage::open(&database_path).expect("opening database");

            storage
                .save_text_with_payloads(
                    "Bonjour",
                    None,
                    &[String::from("text/plain"), String::from("text/rtf")],
                    &[ClipboardFormatPayload::new(
                        "text/rtf",
                        br"{\rtf1 Bonjour}".to_vec(),
                    )],
                )
                .expect("saving first RTF");

            storage
                .save_text_with_payloads(
                    "Bonjour",
                    None,
                    &[String::from("text/plain"), String::from("text/rtf")],
                    &[ClipboardFormatPayload::new(
                        "text/rtf",
                        br"{\rtf1\b Bonjour}".to_vec(),
                    )],
                )
                .expect("saving second RTF");

            assert_eq!(storage.count().expect("counting items"), 2);
        }

        fs::remove_dir_all(directory).expect("removing directory");
    }
}
