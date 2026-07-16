mod installer;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use cliph_core::{
    ClipboardClassification, ClipboardFormatPayload, ClipboardItem, ClipboardItemKind, FilePayload,
    FileTransferOperation, QuickInsertCategory, QuickInsertEntry, search_entries,
};
use cliph_storage::{ClipboardStorage, MAX_FORMAT_PAYLOAD_BYTES, MAX_IMAGE_BYTES};
use gtk::glib::types::StaticType;
use gtk::{Align, Orientation, gdk, gio, glib};

const APP_ID: &str = "com.cliph.ClipH";
const DISPLAYED_HISTORY_LIMIT: usize = 200;

const GNOME_INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const GNOME_COLOR_SCHEME_KEY: &str = "color-scheme";
const GNOME_MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_CUSTOM_KEYBINDING_SCHEMA: &str =
    "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const GNOME_CUSTOM_KEYBINDINGS_KEY: &str = "custom-keybindings";
const CLIPH_GNOME_SHORTCUT_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/cliph/";
const CLIPH_SHORTCUT_BINDING: &str = "<Super>p";

const GNOME_MUTTER_KEYBINDINGS_SCHEMA: &str = "org.gnome.mutter.keybindings";
const GNOME_SWITCH_MONITOR_KEY: &str = "switch-monitor";
const HTML_MIME_TYPES: &[&str] = &[
    "text/html",
    "text/html;charset=utf-8",
    "text/html;charset=UTF-8",
];
const RTF_MIME_TYPES: &[&str] = &["text/rtf", "application/rtf", "application/x-rtf"];
const TSV_MIME_TYPES: &[&str] = &["text/tab-separated-values"];
const CSV_MIME_TYPES: &[&str] = &["text/csv", "application/csv"];
const GNOME_COPIED_FILES_MIME_TYPES: &[&str] = &["x-special/gnome-copied-files"];
const URI_LIST_MIME_TYPES: &[&str] = &["text/uri-list"];

#[derive(Debug, Clone)]
enum DisplayedPayload {
    Text {
        plain_text: String,
        html_text: Option<String>,
        format_payloads: Vec<ClipboardFormatPayload>,
    },
    Image {
        path: Option<PathBuf>,
    },
    Files {
        files: Vec<FilePayload>,
    },
}

#[derive(Debug, Clone)]
struct DisplayedItem {
    id: i64,
    payload: DisplayedPayload,
}

impl DisplayedItem {
    fn from_item(item: &ClipboardItem, format_payloads: Vec<ClipboardFormatPayload>) -> Self {
        let payload = match item.kind {
            ClipboardItemKind::Text => DisplayedPayload::Text {
                plain_text: item.plain_text.clone(),
                html_text: item.html_text.clone(),
                format_payloads,
            },
            ClipboardItemKind::Image => {
                if let Some(image) = &item.image {
                    DisplayedPayload::Image {
                        path: Some(image.path.clone()),
                    }
                } else {
                    DisplayedPayload::Image { path: None }
                }
            }
            ClipboardItemKind::Files => DisplayedPayload::Files {
                files: item.files.clone(),
            },
        };

        Self {
            id: item.id,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PublishedKind {
    PlainText,
    RichText,
    MultiFormat,
    Image,
    Files,
}

impl PublishedKind {
    const fn success_message(self) -> &'static str {
        match self {
            Self::PlainText => "Élément copié — utilisez Ctrl + V pour le coller",
            Self::RichText => "Texte enrichi copié — formatage conservé",
            Self::MultiFormat => "Contenu copié — tous les formats disponibles sont restaurés",
            Self::Image => "Image copiée — utilisez Ctrl + V pour la coller",
            Self::Files => "Fichiers copiés — collez-les dans un dossier avec Ctrl + V",
        }
    }
}

type DisplayedHistory = Rc<RefCell<Vec<DisplayedItem>>>;

struct ClipboardCaptureContext {
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
}

#[derive(Clone)]
struct QuickInsertUiContext {
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
    clipboard: gdk::Clipboard,
    window: adw::ApplicationWindow,
}

fn show_toast(toast_overlay: &adw::ToastOverlay, message: &str) {
    let toast = adw::Toast::new(message);
    toast.set_timeout(3);
    toast_overlay.add_toast(toast);
}

fn format_byte_size(byte_size: u64) -> String {
    const KIBIBYTE: f64 = 1024.0;
    const MEBIBYTE: f64 = 1024.0 * 1024.0;

    if byte_size < 1024 {
        format!("{byte_size} o")
    } else if byte_size < 1024 * 1024 {
        format!("{:.1} Kio", byte_size as f64 / KIBIBYTE)
    } else {
        format!("{:.1} Mio", byte_size as f64 / MEBIBYTE)
    }
}

fn publish_item_to_clipboard(
    clipboard: &gdk::Clipboard,
    item: &DisplayedItem,
) -> Result<PublishedKind, String> {
    match &item.payload {
        DisplayedPayload::Text {
            plain_text,
            html_text,
            format_payloads,
        } => {
            let mut providers = Vec::new();
            let mut has_html = false;
            let mut has_non_html_payload = false;

            for payload in format_payloads {
                if payload.data.is_empty() {
                    continue;
                }

                has_html |= payload.mime_type == "text/html";
                has_non_html_payload |= payload.mime_type != "text/html";

                let bytes = glib::Bytes::from_owned(payload.data.clone());
                providers.push(gdk::ContentProvider::for_bytes(&payload.mime_type, &bytes));
            }

            if !has_html
                && let Some(html_text) = html_text.as_deref().filter(|html| !html.trim().is_empty())
            {
                let html_bytes = glib::Bytes::from_owned(html_text.as_bytes().to_vec());

                providers.push(gdk::ContentProvider::for_bytes("text/html", &html_bytes));

                has_html = true;
            }

            if !has_html && !has_non_html_payload {
                clipboard.set_text(plain_text);
                return Ok(PublishedKind::PlainText);
            }

            let plain_value = plain_text.to_value();
            providers.push(gdk::ContentProvider::for_value(&plain_value));

            let provider = gdk::ContentProvider::new_union(&providers);

            clipboard
                .set_content(Some(&provider))
                .map_err(|error| error.to_string())?;

            if has_non_html_payload {
                Ok(PublishedKind::MultiFormat)
            } else {
                Ok(PublishedKind::RichText)
            }
        }
        DisplayedPayload::Image { path, .. } => {
            let path = path
                .as_ref()
                .ok_or_else(|| String::from("le fichier de l’image est indisponible"))?;

            let file = gio::File::for_path(path);
            let texture = gdk::Texture::from_file(&file).map_err(|error| error.to_string())?;

            clipboard.set_texture(&texture);

            Ok(PublishedKind::Image)
        }
        DisplayedPayload::Files { files } => {
            let available_uris = files
                .iter()
                .filter(|file| file.exists_now() != Some(false))
                .map(|file| file.uri.clone())
                .collect::<Vec<_>>();

            if available_uris.is_empty() {
                return Err(String::from(
                    "aucun des fichiers locaux enregistrés n’existe encore",
                ));
            }

            /*
             * Nautilus sous GTK 4 utilise la représentation native
             * GdkFileList. Les formats MIME restent publiés pour les
             * autres applications et pour la compatibilité GNOME.
             */
            let gio_files = available_uris
                .iter()
                .map(|uri| gio::File::for_uri(uri))
                .collect::<Vec<_>>();

            let file_list = gdk::FileList::from_array(&gio_files);
            let file_list_value = file_list.to_value();

            let uri_list = format!("{}\r\n", available_uris.join("\r\n"));

            /*
             * On restaure volontairement une opération de copie,
             * même lorsque la sélection avait été coupée à l’origine.
             */
            let gnome_copied_files = format!("copy\n{}", available_uris.join("\n"));

            let uri_list_bytes = glib::Bytes::from_owned(uri_list.into_bytes());

            let gnome_bytes = glib::Bytes::from_owned(gnome_copied_files.into_bytes());

            let providers = [
                gdk::ContentProvider::for_value(&file_list_value),
                gdk::ContentProvider::for_bytes("text/uri-list", &uri_list_bytes),
                gdk::ContentProvider::for_bytes("x-special/gnome-copied-files", &gnome_bytes),
            ];

            let provider = gdk::ContentProvider::new_union(&providers);

            clipboard
                .set_content(Some(&provider))
                .map_err(|error| error.to_string())?;

            Ok(PublishedKind::Files)
        }
    }
}

fn create_text_content(item: &ClipboardItem) -> gtk::Box {
    let type_text = match item.classification_subtype.as_deref() {
        Some(subtype) => {
            format!("{} • {subtype}", item.classification_label())
        }
        None => item.classification_label().to_owned(),
    };

    let type_label = gtk::Label::new(Some(&type_text));
    type_label.set_halign(Align::Start);
    type_label.add_css_class("caption");
    type_label.add_css_class("dim-label");
    type_label.set_can_target(false);

    let preview = gtk::Label::new(Some(&item.plain_text));
    preview.set_halign(Align::Fill);
    preview.set_hexpand(true);
    preview.set_xalign(0.0);
    preview.set_wrap(true);
    preview.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    preview.set_lines(6);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
    preview.set_selectable(false);
    preview.set_can_target(false);

    if matches!(
        item.classification,
        ClipboardClassification::Code
            | ClipboardClassification::Html
            | ClipboardClassification::Rtf
    ) {
        preview.add_css_class("monospace");
    }

    let character_count = item.plain_text.chars().count();

    let character_description = match character_count {
        1 => String::from("1 caractère"),
        count => format!("{count} caractères"),
    };

    let classification_description = match item.classification {
        ClipboardClassification::PlainText => "enregistré",
        ClipboardClassification::RichText => "formatage HTML conservé",
        ClipboardClassification::Code => "code source détecté",
        ClipboardClassification::Link => "lien détecté",
        ClipboardClassification::Table => "structure de tableau détectée",
        ClipboardClassification::Html => "source HTML détectée",
        ClipboardClassification::Rtf => "contenu RTF détecté",
        ClipboardClassification::Color => "couleur détectée",
        ClipboardClassification::Image => "image",
        ClipboardClassification::Files => "fichiers",
        ClipboardClassification::Unknown => "contenu enregistré",
    };

    let format_count = item.mime_types.len();

    let metadata = match format_count {
        0 => format!(
            "{character_description} • {classification_description} • confiance {} %",
            item.classification_confidence,
        ),
        1 => format!(
            "{character_description} • {classification_description} • 1 format • confiance {} %",
            item.classification_confidence,
        ),
        count => format!(
            "{character_description} • {classification_description} • {count} formats • confiance {} %",
            item.classification_confidence,
        ),
    };

    let metadata_label = gtk::Label::new(Some(&metadata));
    metadata_label.set_halign(Align::Start);
    metadata_label.set_xalign(0.0);
    metadata_label.set_wrap(true);
    metadata_label.add_css_class("caption");
    metadata_label.add_css_class("dim-label");
    metadata_label.set_can_target(false);

    let content = gtk::Box::new(Orientation::Vertical, 6);
    content.set_hexpand(true);
    content.append(&type_label);
    content.append(&preview);
    content.append(&metadata_label);

    content
}

fn create_image_content(item: &ClipboardItem) -> gtk::Box {
    let container = gtk::Box::new(Orientation::Horizontal, 14);
    container.set_hexpand(true);

    let details = gtk::Box::new(Orientation::Vertical, 6);
    details.set_hexpand(true);
    details.set_valign(Align::Center);

    let type_label = gtk::Label::new(Some("IMAGE"));
    type_label.set_halign(Align::Start);
    type_label.add_css_class("caption");
    type_label.add_css_class("dim-label");
    type_label.set_can_target(false);

    details.append(&type_label);

    if let Some(image) = &item.image {
        let picture = gtk::Picture::for_filename(&image.path);
        picture.set_size_request(170, 110);
        picture.set_can_shrink(true);
        picture.set_halign(Align::Start);
        picture.set_valign(Align::Center);
        picture.set_alternative_text(Some("Aperçu de l’image copiée"));
        picture.set_can_target(false);

        let title = gtk::Label::new(Some("Image copiée"));
        title.set_halign(Align::Start);
        title.set_xalign(0.0);
        title.add_css_class("heading");
        title.set_can_target(false);

        let metadata = format!(
            "{} × {} • {} • PNG",
            image.width,
            image.height,
            format_byte_size(image.byte_size),
        );

        let metadata_label = gtk::Label::new(Some(&metadata));
        metadata_label.set_halign(Align::Start);
        metadata_label.set_xalign(0.0);
        metadata_label.set_wrap(true);
        metadata_label.add_css_class("caption");
        metadata_label.add_css_class("dim-label");
        metadata_label.set_can_target(false);

        details.append(&title);
        details.append(&metadata_label);
        container.append(&picture);
    } else {
        let missing_icon = gtk::Image::from_icon_name("image-missing-symbolic");
        missing_icon.set_pixel_size(64);
        missing_icon.set_can_target(false);

        let title = gtk::Label::new(Some("Image indisponible"));
        title.set_halign(Align::Start);
        title.add_css_class("heading");
        title.set_can_target(false);

        let metadata_label = gtk::Label::new(Some(
            "Le fichier associé à cette image n’a pas pu être retrouvé.",
        ));
        metadata_label.set_halign(Align::Start);
        metadata_label.set_xalign(0.0);
        metadata_label.set_wrap(true);
        metadata_label.add_css_class("caption");
        metadata_label.add_css_class("dim-label");
        metadata_label.set_can_target(false);

        details.append(&title);
        details.append(&metadata_label);
        container.append(&missing_icon);
    }

    container.append(&details);
    container
}

fn create_files_content(item: &ClipboardItem) -> gtk::Box {
    let container = gtk::Box::new(Orientation::Horizontal, 14);
    container.set_hexpand(true);

    let icon_name =
        if item.files.len() == 1 && item.files.first().is_some_and(|file| file.is_directory) {
            "folder-symbolic"
        } else {
            "text-x-generic-symbolic"
        };

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(48);
    icon.set_valign(Align::Center);
    icon.set_can_target(false);

    let details = gtk::Box::new(Orientation::Vertical, 6);
    details.set_hexpand(true);
    details.set_valign(Align::Center);

    let type_text = match item.classification_subtype.as_deref() {
        Some(subtype) => format!("FICHIERS • {subtype}"),
        None => String::from("FICHIERS"),
    };

    let type_label = gtk::Label::new(Some(&type_text));
    type_label.set_halign(Align::Start);
    type_label.add_css_class("caption");
    type_label.add_css_class("dim-label");
    type_label.set_can_target(false);

    let visible_names = item
        .files
        .iter()
        .take(3)
        .map(|file| file.display_name.as_str())
        .collect::<Vec<_>>();

    let mut title_text = visible_names.join("\n");

    if item.files.len() > visible_names.len() {
        let remaining = item.files.len() - visible_names.len();
        title_text.push_str(&format!("\n+ {remaining} autre(s)"));
    }

    if title_text.is_empty() {
        title_text.push_str("Sélection de fichiers indisponible");
    }

    let title = gtk::Label::new(Some(&title_text));
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title.set_lines(5);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("heading");
    title.set_can_target(false);

    let known_size = item
        .files
        .iter()
        .filter_map(|file| file.byte_size)
        .sum::<u64>();

    let missing_count = item
        .files
        .iter()
        .filter(|file| file.exists_now() == Some(false))
        .count();

    let operation = match item.file_transfer_operation {
        Some(FileTransferOperation::Cut) => "coupé à l’origine",
        Some(FileTransferOperation::Copy) | None => "copié",
    };

    let mut metadata_parts = vec![operation.to_owned()];

    if known_size > 0 {
        metadata_parts.push(format_byte_size(known_size));
    }

    if missing_count > 0 {
        metadata_parts.push(format!("{missing_count} introuvable(s)"));
    } else {
        metadata_parts.push(String::from("disponible"));
    }

    let metadata_label = gtk::Label::new(Some(&metadata_parts.join(" • ")));
    metadata_label.set_halign(Align::Start);
    metadata_label.set_xalign(0.0);
    metadata_label.set_wrap(true);
    metadata_label.add_css_class("caption");
    metadata_label.add_css_class("dim-label");
    metadata_label.set_can_target(false);

    details.append(&type_label);
    details.append(&title);
    details.append(&metadata_label);

    container.append(&icon);
    container.append(&details);
    container
}

fn create_history_row(
    item: &ClipboardItem,
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_selectable(false);
    row.set_tooltip_text(Some(
        "Cliquer sur la ligne pour remettre cet élément dans le presse-papiers",
    ));

    let item_content = match item.kind {
        ClipboardItemKind::Text => create_text_content(item),
        ClipboardItemKind::Image => create_image_content(item),
        ClipboardItemKind::Files => create_files_content(item),
    };

    let copy_icon = gtk::Image::from_icon_name("edit-copy-symbolic");
    copy_icon.set_valign(Align::Center);
    copy_icon.add_css_class("dim-label");
    copy_icon.set_can_target(false);

    let delete_button = gtk::Button::from_icon_name("edit-delete-symbolic");
    delete_button.set_tooltip_text(Some("Supprimer cet élément de l’historique"));
    delete_button.add_css_class("flat");
    delete_button.set_valign(Align::Center);
    delete_button.set_focus_on_click(false);

    let actions = gtk::Box::new(Orientation::Horizontal, 6);
    actions.set_valign(Align::Center);
    actions.append(&copy_icon);
    actions.append(&delete_button);

    let row_content = gtk::Box::new(Orientation::Horizontal, 12);
    row_content.set_margin_top(12);
    row_content.set_margin_bottom(12);
    row_content.set_margin_start(14);
    row_content.set_margin_end(14);
    row_content.append(&item_content);
    row_content.append(&actions);

    row.set_child(Some(&row_content));

    let item_id = item.id;
    let storage_for_delete = storage;
    let history_list_for_delete = history_list;
    let row_for_delete = row.clone();
    let empty_state_for_delete = empty_state;
    let counter_for_delete = counter_label;
    let displayed_for_delete = displayed_history;
    let toast_for_delete = toast_overlay;

    delete_button.connect_clicked(move |_| match storage_for_delete.delete_item(item_id) {
        Ok(true) => {
            history_list_for_delete.remove(&row_for_delete);
            displayed_for_delete
                .borrow_mut()
                .retain(|displayed_item| displayed_item.id != item_id);

            let total_count = match storage_for_delete.count() {
                Ok(count) => count,
                Err(error) => {
                    eprintln!("Impossible de compter les éléments après suppression : {error}");
                    displayed_for_delete.borrow().len()
                }
            };

            update_history_status(total_count, &empty_state_for_delete, &counter_for_delete);

            show_toast(&toast_for_delete, "Élément supprimé de l’historique");
        }
        Ok(false) => {
            show_toast(&toast_for_delete, "Cet élément a déjà été supprimé");
        }
        Err(error) => {
            eprintln!("Impossible de supprimer l’élément {item_id} : {error}");
            show_toast(&toast_for_delete, "Impossible de supprimer cet élément");
        }
    });

    row
}

fn clear_history_list(history_list: &gtk::ListBox) {
    while let Some(child) = history_list.first_child() {
        history_list.remove(&child);
    }
}

fn update_history_status(total_count: usize, empty_state: &gtk::Label, counter_label: &gtk::Label) {
    let counter_text = match total_count {
        0 => String::from("0 élément"),
        1 => String::from("1 élément enregistré"),
        count => format!("{count} éléments enregistrés"),
    };

    counter_label.set_text(&counter_text);
    empty_state.set_label(
        "L’historique est vide.\n\nCopiez un texte, une image ou un fichier avec Ctrl + C.",
    );
    empty_state.set_visible(total_count == 0);
}

fn refresh_history(
    storage: &Rc<ClipboardStorage>,
    history_list: &gtk::ListBox,
    empty_state: &gtk::Label,
    counter_label: &gtk::Label,
    displayed_history: &DisplayedHistory,
    toast_overlay: &adw::ToastOverlay,
) {
    match storage.list_recent(DISPLAYED_HISTORY_LIMIT) {
        Ok(items) => {
            clear_history_list(history_list);

            let displayed_items = items
                .iter()
                .map(|item| {
                    let format_payloads = if item.kind == ClipboardItemKind::Text {
                        match storage.load_format_payloads(item.id) {
                            Ok(payloads) => payloads,
                            Err(error) => {
                                eprintln!(
                                    "Impossible de charger les formats de l’élément {} : {error}",
                                    item.id,
                                );
                                Vec::new()
                            }
                        }
                    } else {
                        Vec::new()
                    };

                    DisplayedItem::from_item(item, format_payloads)
                })
                .collect::<Vec<_>>();

            *displayed_history.borrow_mut() = displayed_items;

            for item in &items {
                let row = create_history_row(
                    item,
                    storage.clone(),
                    history_list.clone(),
                    empty_state.clone(),
                    counter_label.clone(),
                    displayed_history.clone(),
                    toast_overlay.clone(),
                );

                history_list.append(&row);
            }

            let total_count = storage.count().unwrap_or(items.len());
            update_history_status(total_count, empty_state, counter_label);
        }
        Err(error) => {
            eprintln!("Impossible de charger l’historique : {error}");
            clear_history_list(history_list);
            displayed_history.borrow_mut().clear();
            counter_label.set_text("Erreur de chargement");
            empty_state.set_label("ClipH n’a pas pu charger l’historique.\nConsultez le terminal.");
            empty_state.set_visible(true);
        }
    }
}

fn clipboard_has_any_mime_type(
    available_mime_types: &[String],
    accepted_mime_types: &[&str],
) -> bool {
    available_mime_types.iter().any(|available| {
        accepted_mime_types
            .iter()
            .any(|accepted| available.eq_ignore_ascii_case(accepted))
    })
}

async fn read_format_payload(
    clipboard: &gdk::Clipboard,
    available_mime_types: &[String],
    accepted_mime_types: &[&str],
    canonical_mime_type: &str,
) -> Option<ClipboardFormatPayload> {
    if !clipboard_has_any_mime_type(available_mime_types, accepted_mime_types) {
        return None;
    }

    let (stream, _selected_mime_type) = match clipboard
        .read_future(accepted_mime_types, glib::Priority::DEFAULT)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Impossible de lire le format {canonical_mime_type} : {error}");
            return None;
        }
    };

    let buffer = vec![0_u8; MAX_FORMAT_PAYLOAD_BYTES + 1];

    let (buffer, bytes_read, partial_error) = match stream
        .read_all_future(buffer, glib::Priority::DEFAULT)
        .await
    {
        Ok(result) => result,
        Err((_buffer, error)) => {
            eprintln!("Impossible de lire les données {canonical_mime_type} : {error}");
            return None;
        }
    };

    if let Some(error) = partial_error {
        eprintln!("Lecture partielle du format {canonical_mime_type} : {error}");
    }

    if bytes_read == 0 {
        return None;
    }

    if bytes_read > MAX_FORMAT_PAYLOAD_BYTES {
        eprintln!(
            "Format {canonical_mime_type} ignoré : limite de {} octets dépassée",
            MAX_FORMAT_PAYLOAD_BYTES,
        );
        return None;
    }

    let mut data = buffer[..bytes_read].to_vec();

    while data.last() == Some(&0) {
        data.pop();
    }

    if data.is_empty() {
        return None;
    }

    Some(ClipboardFormatPayload::new(canonical_mime_type, data))
}

async fn read_text_format_payloads(
    clipboard: &gdk::Clipboard,
    available_mime_types: &[String],
) -> Vec<ClipboardFormatPayload> {
    let mut payloads = Vec::new();

    if let Some(payload) = read_format_payload(
        clipboard,
        available_mime_types,
        HTML_MIME_TYPES,
        "text/html",
    )
    .await
    {
        payloads.push(payload);
    }

    if let Some(payload) =
        read_format_payload(clipboard, available_mime_types, RTF_MIME_TYPES, "text/rtf").await
    {
        payloads.push(payload);
    }

    if let Some(payload) = read_format_payload(
        clipboard,
        available_mime_types,
        TSV_MIME_TYPES,
        "text/tab-separated-values",
    )
    .await
    {
        payloads.push(payload);
    }

    if let Some(payload) =
        read_format_payload(clipboard, available_mime_types, CSV_MIME_TYPES, "text/csv").await
    {
        payloads.push(payload);
    }

    payloads
}

fn html_text_from_payloads(payloads: &[ClipboardFormatPayload]) -> Option<String> {
    let payload = payloads
        .iter()
        .find(|payload| payload.mime_type == "text/html")?;

    let html_text = String::from_utf8_lossy(&payload.data)
        .trim_end_matches('\0')
        .to_string();

    (!html_text.trim().is_empty()).then_some(html_text)
}

fn is_file_like_uri(uri: &str) -> bool {
    let lowercase = uri.to_ascii_lowercase();

    [
        "file://",
        "smb://",
        "sftp://",
        "ftp://",
        "dav://",
        "davs://",
        "mtp://",
        "gphoto2://",
        "trash://",
        "recent://",
    ]
    .iter()
    .any(|scheme| lowercase.starts_with(scheme))
}

fn parse_file_transfer_payload(
    payload: &ClipboardFormatPayload,
) -> Option<(FileTransferOperation, Vec<String>)> {
    let text = String::from_utf8_lossy(&payload.data);
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));

    let mut operation = FileTransferOperation::Copy;
    let mut uris = Vec::new();

    if payload.mime_type == "x-special/gnome-copied-files"
        && let Some(first_line) = lines.next()
    {
        operation = if first_line.eq_ignore_ascii_case("cut") {
            FileTransferOperation::Cut
        } else {
            FileTransferOperation::Copy
        };

        // Garde ici tout le reste du contenu actuel.
    }

    uris.extend(
        lines
            .filter(|line| is_file_like_uri(line))
            .map(str::to_owned),
    );

    let mut unique_uris = Vec::new();

    for uri in uris {
        if !unique_uris.contains(&uri) {
            unique_uris.push(uri);
        }
    }

    (!unique_uris.is_empty()).then_some((operation, unique_uris))
}

fn mime_type_from_path(path: &Path, is_directory: bool) -> Option<String> {
    if is_directory {
        return Some(String::from("inode/directory"));
    }

    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();

    let mime_type = match extension.as_str() {
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "rtf" => "text/rtf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    };

    Some(mime_type.to_owned())
}

fn file_payload_from_uri(uri: &str) -> FilePayload {
    let file = gio::File::for_uri(uri);
    let path = file.path();
    let metadata = path.as_deref().and_then(|path| fs::metadata(path).ok());

    let is_directory = metadata.as_ref().is_some_and(std::fs::Metadata::is_dir);

    let byte_size = metadata
        .as_ref()
        .filter(|metadata| metadata.is_file())
        .map(std::fs::Metadata::len);

    let display_name = path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_owned());

    let mime_type = path
        .as_deref()
        .and_then(|path| mime_type_from_path(path, is_directory));

    FilePayload {
        uri: uri.to_owned(),
        path,
        display_name,
        mime_type,
        byte_size,
        is_directory,
        existed_at_capture: metadata.is_some(),
    }
}

async fn read_file_transfer_payload(
    clipboard: &gdk::Clipboard,
    available_mime_types: &[String],
) -> Option<ClipboardFormatPayload> {
    if let Some(payload) = read_format_payload(
        clipboard,
        available_mime_types,
        GNOME_COPIED_FILES_MIME_TYPES,
        "x-special/gnome-copied-files",
    )
    .await
    {
        return Some(payload);
    }

    read_format_payload(
        clipboard,
        available_mime_types,
        URI_LIST_MIME_TYPES,
        "text/uri-list",
    )
    .await
}

fn capture_file_content(
    clipboard: gdk::Clipboard,
    context: ClipboardCaptureContext,
    available_mime_types: Vec<String>,
) {
    glib::MainContext::default().spawn_local(async move {
        let Some(payload) = read_file_transfer_payload(&clipboard, &available_mime_types).await
        else {
            capture_text_content(clipboard, context, available_mime_types);
            return;
        };

        let Some((operation, uris)) = parse_file_transfer_payload(&payload) else {
            capture_text_content(clipboard, context, available_mime_types);
            return;
        };

        let files = uris
            .iter()
            .map(|uri| file_payload_from_uri(uri))
            .collect::<Vec<_>>();

        match context
            .storage
            .save_files(operation, &files, &available_mime_types)
        {
            Ok(_) => {
                refresh_history(
                    &context.storage,
                    &context.history_list,
                    &context.empty_state,
                    &context.counter_label,
                    &context.displayed_history,
                    &context.toast_overlay,
                );
            }
            Err(error) => {
                eprintln!("Impossible d’enregistrer les fichiers copiés : {error}");
                show_toast(
                    &context.toast_overlay,
                    "Impossible d’enregistrer cette sélection de fichiers",
                );
            }
        }
    });
}

fn capture_image_content(
    clipboard: gdk::Clipboard,
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
) {
    glib::MainContext::default().spawn_local(async move {
        let texture = match clipboard.read_texture_future().await {
            Ok(Some(texture)) => texture,
            Ok(None) => return,
            Err(error) => {
                eprintln!("Impossible de lire l’image du presse-papiers : {error}");
                return;
            }
        };

        let width = texture.width();
        let height = texture.height();
        let png_bytes = texture.save_to_png_bytes();
        let png_slice = png_bytes.as_ref();

        if png_slice.len() > MAX_IMAGE_BYTES {
            show_toast(
                &toast_overlay,
                "Image ignorée : sa taille dépasse la limite de 25 Mio",
            );
            return;
        }

        match storage.save_image_png(png_slice, width, height) {
            Ok(_) => {
                refresh_history(
                    &storage,
                    &history_list,
                    &empty_state,
                    &counter_label,
                    &displayed_history,
                    &toast_overlay,
                );
            }
            Err(error) => {
                eprintln!("Impossible d’enregistrer l’image : {error}");
                show_toast(&toast_overlay, "Impossible d’enregistrer cette image");
            }
        }
    });
}

fn capture_text_content(
    clipboard: gdk::Clipboard,
    context: ClipboardCaptureContext,
    available_mime_types: Vec<String>,
) {
    glib::MainContext::default().spawn_local(async move {
        let plain_text = match clipboard.read_text_future().await {
            Ok(Some(text)) => text.to_string(),
            Ok(None) => return,
            Err(_) => return,
        };

        if plain_text.trim().is_empty() {
            return;
        }

        let payloads = read_text_format_payloads(&clipboard, &available_mime_types).await;

        let html_text = html_text_from_payloads(&payloads);

        match context.storage.save_text_with_payloads(
            &plain_text,
            html_text.as_deref(),
            &available_mime_types,
            &payloads,
        ) {
            Ok(_) => {
                refresh_history(
                    &context.storage,
                    &context.history_list,
                    &context.empty_state,
                    &context.counter_label,
                    &context.displayed_history,
                    &context.toast_overlay,
                );
            }
            Err(error) => {
                eprintln!("Impossible d’enregistrer le contenu du presse-papiers : {error}");

                show_toast(
                    &context.toast_overlay,
                    "Impossible d’enregistrer tous les formats de ce contenu",
                );
            }
        }
    });
}

fn capture_clipboard_content(
    clipboard: &gdk::Clipboard,
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
) {
    if clipboard.is_local() {
        return;
    }

    let formats = clipboard.formats();
    let mime_types = formats.mime_types();

    let available_mime_types = mime_types
        .iter()
        .map(|mime_type| mime_type.as_str().to_owned())
        .collect::<Vec<_>>();

    let contains_files =
        clipboard_has_any_mime_type(&available_mime_types, GNOME_COPIED_FILES_MIME_TYPES)
            || clipboard_has_any_mime_type(&available_mime_types, URI_LIST_MIME_TYPES);

    if contains_files {
        capture_file_content(
            clipboard.clone(),
            ClipboardCaptureContext {
                storage,
                history_list,
                empty_state,
                counter_label,
                displayed_history,
                toast_overlay,
            },
            available_mime_types,
        );
        return;
    }

    let contains_image = formats.contains_type(gdk::Texture::static_type())
        || mime_types
            .iter()
            .any(|mime_type| mime_type.as_str().starts_with("image/"));

    if contains_image {
        capture_image_content(
            clipboard.clone(),
            storage,
            history_list,
            empty_state,
            counter_label,
            displayed_history,
            toast_overlay,
        );
        return;
    }

    let contains_text = mime_types.iter().any(|mime_type| {
        let mime_type = mime_type.as_str();

        mime_type.starts_with("text/plain")
            || mime_type.starts_with("text/html")
            || mime_type.eq_ignore_ascii_case("text/csv")
            || mime_type.eq_ignore_ascii_case("application/csv")
            || mime_type.eq_ignore_ascii_case("text/tab-separated-values")
            || mime_type.eq_ignore_ascii_case("text/rtf")
            || mime_type.eq_ignore_ascii_case("application/rtf")
            || matches!(mime_type, "UTF8_STRING" | "STRING" | "TEXT")
    });

    if contains_text {
        capture_text_content(
            clipboard.clone(),
            ClipboardCaptureContext {
                storage,
                history_list,
                empty_state,
                counter_label,
                displayed_history,
                toast_overlay,
            },
            available_mime_types,
        );
    }
}

fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn quick_insert_page_name(category: QuickInsertCategory) -> &'static str {
    match category {
        QuickInsertCategory::Emoji => "emoji",
        QuickInsertCategory::Kaomoji => "kaomoji",
        QuickInsertCategory::Symbol => "symbol",
    }
}

fn quick_insert_description(category: QuickInsertCategory) -> &'static str {
    match category {
        QuickInsertCategory::Emoji => "Recherchez un émoji puis cliquez dessus pour le copier.",
        QuickInsertCategory::Kaomoji => "Choisissez une expression japonaise prête à être collée.",
        QuickInsertCategory::Symbol => {
            "Retrouvez rapidement les signes mathématiques, techniques et typographiques."
        }
    }
}

fn create_quick_insert_button(
    entry: &'static QuickInsertEntry,
    context: &QuickInsertUiContext,
) -> gtk::Button {
    let value_label = gtk::Label::new(Some(entry.value));
    value_label.set_halign(Align::Center);
    value_label.set_valign(Align::Center);
    value_label.set_margin_top(8);
    value_label.set_margin_bottom(8);
    value_label.set_margin_start(10);
    value_label.set_margin_end(10);
    value_label.set_selectable(false);

    match entry.category {
        QuickInsertCategory::Emoji => {
            value_label.add_css_class("title-1");
        }
        QuickInsertCategory::Kaomoji => {
            value_label.add_css_class("monospace");
            value_label.add_css_class("title-4");
        }
        QuickInsertCategory::Symbol => {
            value_label.add_css_class("title-2");
        }
    }

    let button = gtk::Button::new();
    button.set_child(Some(&value_label));
    button.add_css_class("flat");
    button.set_focus_on_click(false);
    button.set_tooltip_text(Some(&format!("{} — cliquer pour copier", entry.label)));

    let value = entry.value.to_owned();
    let context = context.clone();

    button.connect_clicked(move |_| {
        context.clipboard.set_text(&value);

        let mime_types = vec![
            String::from("text/plain"),
            String::from("text/plain;charset=utf-8"),
        ];

        match context
            .storage
            .save_text_with_payloads(&value, None, &mime_types, &[])
        {
            Ok(_) => {
                refresh_history(
                    &context.storage,
                    &context.history_list,
                    &context.empty_state,
                    &context.counter_label,
                    &context.displayed_history,
                    &context.toast_overlay,
                );
            }
            Err(error) => {
                eprintln!("Impossible d’ajouter l’insertion rapide à l’historique : {error}");
            }
        }

        show_toast(
            &context.toast_overlay,
            &format!("{value} copié — utilisez Ctrl + V pour l’insérer"),
        );

        let window = context.window.clone();
        glib::timeout_add_local_once(Duration::from_millis(350), move || window.hide());
    });

    button
}

fn populate_quick_insert_results(
    category: QuickInsertCategory,
    query: &str,
    results_container: &gtk::Box,
    result_count_label: &gtk::Label,
    context: &QuickInsertUiContext,
) {
    clear_box_children(results_container);

    let entries = search_entries(category, query, 500);

    let count_text = match entries.len() {
        0 => String::from("Aucun résultat"),
        1 => String::from("1 résultat"),
        count => format!("{count} résultats"),
    };
    result_count_label.set_text(&count_text);

    if entries.is_empty() {
        let empty_label = gtk::Label::new(Some("Aucun élément ne correspond à cette recherche."));
        empty_label.set_halign(Align::Center);
        empty_label.set_valign(Align::Center);
        empty_label.set_vexpand(true);
        empty_label.set_wrap(true);
        empty_label.add_css_class("dim-label");

        results_container.append(&empty_label);
        return;
    }

    let mut groups: BTreeMap<&'static str, Vec<&'static QuickInsertEntry>> = BTreeMap::new();

    for entry in entries {
        groups.entry(entry.group).or_default().push(entry);
    }

    for (group_name, group_entries) in groups {
        let group_label = gtk::Label::new(Some(group_name));
        group_label.set_halign(Align::Start);
        group_label.set_xalign(0.0);
        group_label.add_css_class("heading");

        let flow_box = gtk::FlowBox::new();
        flow_box.set_halign(Align::Fill);
        flow_box.set_hexpand(true);
        flow_box.set_selection_mode(gtk::SelectionMode::None);
        flow_box.set_activate_on_single_click(false);
        flow_box.set_column_spacing(6);
        flow_box.set_row_spacing(6);
        flow_box.set_min_children_per_line(4);
        flow_box.set_max_children_per_line(10);

        for entry in group_entries {
            let button = create_quick_insert_button(entry, context);
            flow_box.insert(&button, -1);
        }

        let section = gtk::Box::new(Orientation::Vertical, 8);
        section.append(&group_label);
        section.append(&flow_box);

        results_container.append(&section);
    }
}

fn create_quick_insert_page(
    category: QuickInsertCategory,
    context: QuickInsertUiContext,
) -> gtk::Box {
    let title = gtk::Label::new(Some(category.display_label()));
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    title.add_css_class("title-2");

    let description = gtk::Label::new(Some(quick_insert_description(category)));
    description.set_halign(Align::Start);
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.add_css_class("dim-label");

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some(&format!(
        "Rechercher dans {}…",
        category.display_label().to_lowercase()
    )));

    let result_count_label = gtk::Label::new(None);
    result_count_label.set_halign(Align::Start);
    result_count_label.add_css_class("caption");
    result_count_label.add_css_class("dim-label");

    let introduction = gtk::Box::new(Orientation::Vertical, 8);
    introduction.append(&title);
    introduction.append(&description);
    introduction.append(&search_entry);
    introduction.append(&result_count_label);

    let results_container = gtk::Box::new(Orientation::Vertical, 20);
    results_container.set_hexpand(true);
    results_container.set_vexpand(true);

    populate_quick_insert_results(
        category,
        "",
        &results_container,
        &result_count_label,
        &context,
    );

    let results_for_search = results_container.clone();
    let count_for_search = result_count_label.clone();
    let context_for_search = context.clone();

    search_entry.connect_search_changed(move |entry| {
        populate_quick_insert_results(
            category,
            entry.text().as_str(),
            &results_for_search,
            &count_for_search,
            &context_for_search,
        );
    });

    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&results_container)
        .build();

    let page = gtk::Box::new(Orientation::Vertical, 16);
    page.set_margin_top(20);
    page.set_margin_bottom(20);
    page.set_margin_start(24);
    page.set_margin_end(24);
    page.append(&introduction);
    page.append(&scrolled_window);

    page
}

fn show_startup_error(app: &adw::Application, message: &str) {
    let title = gtk::Label::new(Some("ClipH ne peut pas démarrer"));
    title.add_css_class("title-2");

    let details = gtk::Label::new(Some(message));
    details.set_wrap(true);
    details.set_justify(gtk::Justification::Center);
    details.add_css_class("dim-label");

    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_margin_top(32);
    content.set_margin_bottom(32);
    content.set_margin_start(32);
    content.set_margin_end(32);
    content.append(&title);
    content.append(&details);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Erreur ClipH")
        .default_width(520)
        .default_height(300)
        .content(&content)
        .build();

    window.present();
}

fn shortcut_binding_is_super_p(binding: &str) -> bool {
    matches!(
        binding
            .trim()
            .replace(' ', "")
            .to_ascii_lowercase()
            .as_str(),
        "<super>p" | "<meta>p"
    )
}

fn release_super_p_from_mutter() -> Result<usize, String> {
    let settings = gio::Settings::new(GNOME_MUTTER_KEYBINDINGS_SCHEMA);

    let bindings = settings
        .strv(GNOME_SWITCH_MONITOR_KEY)
        .iter()
        .map(|binding| binding.as_str().to_owned())
        .collect::<Vec<_>>();

    let remaining_bindings = bindings
        .iter()
        .filter(|binding| !shortcut_binding_is_super_p(binding))
        .cloned()
        .collect::<Vec<_>>();

    let released_bindings = bindings.len() - remaining_bindings.len();

    if released_bindings == 0 {
        return Ok(0);
    }

    let remaining_binding_refs = remaining_bindings
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    settings
        .set_strv(GNOME_SWITCH_MONITOR_KEY, remaining_binding_refs)
        .map_err(|error| format!("impossible de libérer Super + P dans GNOME Mutter : {error}",))?;

    Ok(released_bindings)
}

fn install_gnome_super_p_shortcut() -> Result<usize, String> {
    let Some(schema_source) = gio::SettingsSchemaSource::default() else {
        return Err(String::from("la source des schémas GNOME est indisponible"));
    };

    for schema_id in [
        GNOME_MEDIA_KEYS_SCHEMA,
        GNOME_CUSTOM_KEYBINDING_SCHEMA,
        GNOME_MUTTER_KEYBINDINGS_SCHEMA,
    ] {
        if schema_source.lookup(schema_id, true).is_none() {
            return Err(format!("le schéma GNOME {schema_id} est indisponible",));
        }
    }

    let media_keys = gio::Settings::new(GNOME_MEDIA_KEYS_SCHEMA);

    let mut shortcut_paths = media_keys
        .strv(GNOME_CUSTOM_KEYBINDINGS_KEY)
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect::<Vec<_>>();

    let mut released_shortcuts = release_super_p_from_mutter()?;

    for shortcut_path in &shortcut_paths {
        if shortcut_path == CLIPH_GNOME_SHORTCUT_PATH {
            continue;
        }

        let shortcut = gio::Settings::with_path(GNOME_CUSTOM_KEYBINDING_SCHEMA, shortcut_path);

        let existing_binding = shortcut.string("binding");

        if shortcut_binding_is_super_p(existing_binding.as_str()) {
            shortcut.set_string("binding", "").map_err(|error| {
                format!("impossible de libérer Super + P pour ClipH : {error}",)
            })?;

            released_shortcuts += 1;
        }
    }

    if !shortcut_paths
        .iter()
        .any(|path| path == CLIPH_GNOME_SHORTCUT_PATH)
    {
        shortcut_paths.push(CLIPH_GNOME_SHORTCUT_PATH.to_owned());

        let shortcut_path_refs = shortcut_paths
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();

        media_keys
            .set_strv(GNOME_CUSTOM_KEYBINDINGS_KEY, shortcut_path_refs)
            .map_err(|error| format!("impossible d’enregistrer le raccourci ClipH : {error}",))?;
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("impossible de retrouver l’exécutable ClipH : {error}",))?;

    let executable = executable.to_string_lossy();

    let command = if executable.chars().any(char::is_whitespace) {
        format!("'{}'", executable.replace('\'', "'\"'\"'"),)
    } else {
        executable.into_owned()
    };

    let shortcut =
        gio::Settings::with_path(GNOME_CUSTOM_KEYBINDING_SCHEMA, CLIPH_GNOME_SHORTCUT_PATH);

    shortcut
        .set_string("name", "Afficher ou masquer ClipH")
        .map_err(|error| format!("impossible de nommer le raccourci ClipH : {error}"))?;

    shortcut
        .set_string("command", &command)
        .map_err(|error| format!("impossible de définir la commande ClipH : {error}"))?;

    shortcut
        .set_string("binding", CLIPH_SHORTCUT_BINDING)
        .map_err(|error| format!("impossible d’attribuer Super + P à ClipH : {error}"))?;

    Ok(released_shortcuts)
}

fn setup_global_shortcut(toast_overlay: &adw::ToastOverlay) {
    match install_gnome_super_p_shortcut() {
        Ok(released_shortcuts) => {
            if released_shortcuts > 0 {
                println!(
                    "{released_shortcuts} ancien raccourci utilisant \
                     Super + P a été libéré.",
                );
            }

            println!("Raccourci ClipH actif : Super + P");

            show_toast(toast_overlay, "Super + P est maintenant attribué à ClipH");
        }
        Err(error) => {
            eprintln!("Impossible d’activer le raccourci Super + P : {error}",);

            show_toast(toast_overlay, "Impossible d’activer Super + P");
        }
    }
}

fn build_ui(app: &adw::Application, start_hidden: bool) {
    let storage = match ClipboardStorage::open_default() {
        Ok(storage) => Rc::new(storage),
        Err(error) => {
            eprintln!("Impossible d’initialiser ClipH : {error}");
            show_startup_error(app, &error.to_string());
            return;
        }
    };

    println!(
        "Base de données ClipH : {}",
        storage.database_path().display()
    );
    println!("Images ClipH : {}", storage.images_directory().display());

    let displayed_history: DisplayedHistory = Rc::new(RefCell::new(Vec::new()));

    let header_logo = gtk::Image::from_icon_name(APP_ID);
    header_logo.set_pixel_size(28);
    header_logo.set_tooltip_text(Some("ClipH By Henry Gossou"));
    header_logo.set_can_target(false);

    let header_title = gtk::Label::new(Some("ClipH"));
    header_title.add_css_class("heading");
    header_title.set_can_target(false);

    let header_identity = gtk::Box::new(Orientation::Horizontal, 8);

    header_identity.set_valign(Align::Center);
    header_identity.append(&header_logo);
    header_identity.append(&header_title);

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&header_identity));

    let title = gtk::Label::new(Some("Historique du presse-papiers"));
    title.set_halign(Align::Start);
    title.add_css_class("title-2");

    let description = gtk::Label::new(Some(
        "Cliquez sur un texte, une image ou des fichiers pour les remettre dans le presse-papiers.",
    ));
    description.set_halign(Align::Start);
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.add_css_class("dim-label");

    let counter_label = gtk::Label::new(Some("0 élément"));
    counter_label.set_halign(Align::Start);
    counter_label.add_css_class("caption");
    counter_label.add_css_class("dim-label");

    let introduction = gtk::Box::new(Orientation::Vertical, 6);
    introduction.append(&title);
    introduction.append(&description);
    introduction.append(&counter_label);

    let empty_state = gtk::Label::new(Some(
        "L’historique est vide.\n\nCopiez un texte, une image ou un fichier avec Ctrl + C.",
    ));
    empty_state.set_halign(Align::Center);
    empty_state.set_valign(Align::Center);
    empty_state.set_justify(gtk::Justification::Center);
    empty_state.set_wrap(true);
    empty_state.set_vexpand(true);
    empty_state.add_css_class("dim-label");

    let history_list = gtk::ListBox::new();
    history_list.set_selection_mode(gtk::SelectionMode::None);
    history_list.set_activate_on_single_click(true);
    history_list.add_css_class("boxed-list");

    let history_container = gtk::Box::new(Orientation::Vertical, 12);
    history_container.append(&empty_state);
    history_container.append(&history_list);

    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&history_container)
        .build();

    let history_page = gtk::Box::new(Orientation::Vertical, 18);
    history_page.set_margin_top(20);
    history_page.set_margin_bottom(20);
    history_page.set_margin_start(24);
    history_page.set_margin_end(24);
    history_page.append(&introduction);
    history_page.append(&scrolled_window);

    let view_stack = gtk::Stack::new();
    view_stack.set_hexpand(true);
    view_stack.set_vexpand(true);
    view_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    view_stack.add_titled(&history_page, Some("history"), "Historique");

    let view_switcher = gtk::StackSwitcher::new();
    view_switcher.set_halign(Align::Center);
    view_switcher.set_stack(Some(&view_stack));

    let content = gtk::Box::new(Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.append(&view_switcher);
    content.append(&view_stack);

    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.append(&header_bar);
    page.append(&content);

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&page));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("ClipH")
        .default_width(680)
        .default_height(680)
        .content(&toast_overlay)
        .build();

    window.set_hide_on_close(true);

    /*
     * La croix ne termine jamais ClipH.
     * Elle masque seulement sa fenêtre.
     */
    window.connect_close_request(|window| {
        window.hide();
        glib::Propagation::Stop
    });

    let clear_menu_button = gtk::MenuButton::new();
    clear_menu_button.set_icon_name("edit-delete-symbolic");
    clear_menu_button.set_tooltip_text(Some("Effacer tout l’historique"));
    clear_menu_button.add_css_class("flat");

    let confirmation_title = gtk::Label::new(Some("Effacer tout l’historique ?"));
    confirmation_title.set_halign(Align::Start);
    confirmation_title.add_css_class("heading");

    let confirmation_text = gtk::Label::new(Some(
        "Tous les textes, images et fichiers associés seront définitivement supprimés.",
    ));
    confirmation_text.set_halign(Align::Start);
    confirmation_text.set_xalign(0.0);
    confirmation_text.set_wrap(true);
    confirmation_text.set_max_width_chars(34);
    confirmation_text.add_css_class("dim-label");

    let confirm_clear_button = gtk::Button::with_label("Tout effacer");
    confirm_clear_button.add_css_class("destructive-action");

    let confirmation_content = gtk::Box::new(Orientation::Vertical, 12);
    confirmation_content.set_margin_top(16);
    confirmation_content.set_margin_bottom(16);
    confirmation_content.set_margin_start(16);
    confirmation_content.set_margin_end(16);
    confirmation_content.append(&confirmation_title);
    confirmation_content.append(&confirmation_text);
    confirmation_content.append(&confirm_clear_button);

    let clear_popover = gtk::Popover::new();
    clear_popover.set_child(Some(&confirmation_content));
    clear_menu_button.set_popover(Some(&clear_popover));
    header_bar.pack_end(&clear_menu_button);

    let storage_for_clear = storage.clone();
    let list_for_clear = history_list.clone();
    let empty_state_for_clear = empty_state.clone();
    let counter_for_clear = counter_label.clone();
    let displayed_for_clear = displayed_history.clone();
    let toast_for_clear = toast_overlay.clone();
    let popover_for_clear = clear_popover.clone();

    confirm_clear_button.connect_clicked(move |_| {
        match storage_for_clear.clear_history() {
            Ok(0) => show_toast(&toast_for_clear, "L’historique est déjà vide"),
            Ok(deleted_count) => {
                refresh_history(
                    &storage_for_clear,
                    &list_for_clear,
                    &empty_state_for_clear,
                    &counter_for_clear,
                    &displayed_for_clear,
                    &toast_for_clear,
                );

                let message = match deleted_count {
                    1 => String::from("1 élément supprimé"),
                    count => format!("{count} éléments supprimés"),
                };

                show_toast(&toast_for_clear, &message);
            }
            Err(error) => {
                eprintln!("Impossible d’effacer l’historique : {error}");
                show_toast(&toast_for_clear, "Impossible d’effacer l’historique");
            }
        }

        popover_for_clear.popdown();
    });

    refresh_history(
        &storage,
        &history_list,
        &empty_state,
        &counter_label,
        &displayed_history,
        &toast_overlay,
    );

    let display =
        gdk::Display::default().expect("ClipH ne peut pas accéder à l’affichage graphique.");
    let clipboard = display.clipboard();

    let quick_insert_context = QuickInsertUiContext {
        storage: storage.clone(),
        history_list: history_list.clone(),
        empty_state: empty_state.clone(),
        counter_label: counter_label.clone(),
        displayed_history: displayed_history.clone(),
        toast_overlay: toast_overlay.clone(),
        clipboard: clipboard.clone(),
        window: window.clone(),
    };

    for category in [
        QuickInsertCategory::Emoji,
        QuickInsertCategory::Kaomoji,
        QuickInsertCategory::Symbol,
    ] {
        let quick_page = create_quick_insert_page(category, quick_insert_context.clone());

        view_stack.add_titled(
            &quick_page,
            Some(quick_insert_page_name(category)),
            category.display_label(),
        );
    }

    let displayed_for_activation = displayed_history.clone();
    let clipboard_for_activation = clipboard.clone();
    let toast_for_activation = toast_overlay.clone();

    history_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }

        let selected_item = displayed_for_activation
            .borrow()
            .get(index as usize)
            .cloned();

        let Some(selected_item) = selected_item else {
            eprintln!("Élément introuvable pour la ligne {index}");
            return;
        };

        match publish_item_to_clipboard(&clipboard_for_activation, &selected_item) {
            Ok(kind) => show_toast(&toast_for_activation, kind.success_message()),
            Err(error) => {
                eprintln!("Impossible de restaurer l’élément : {error}");
                show_toast(&toast_for_activation, "Impossible de copier cet élément");
            }
        }
    });

    let storage_for_signal = storage.clone();
    let list_for_signal = history_list.clone();
    let empty_state_for_signal = empty_state.clone();
    let counter_for_signal = counter_label.clone();
    let displayed_for_signal = displayed_history.clone();
    let toast_for_signal = toast_overlay.clone();

    clipboard.connect_changed(move |changed_clipboard| {
        capture_clipboard_content(
            changed_clipboard,
            storage_for_signal.clone(),
            list_for_signal.clone(),
            empty_state_for_signal.clone(),
            counter_for_signal.clone(),
            displayed_for_signal.clone(),
            toast_for_signal.clone(),
        );
    });

    capture_clipboard_content(
        &clipboard,
        storage,
        history_list,
        empty_state,
        counter_label,
        displayed_history,
        toast_overlay.clone(),
    );

    setup_global_shortcut(&toast_overlay);

    if start_hidden {
        window.hide();
    } else {
        window.present();
    }
}

fn apply_system_color_scheme(settings: &gio::Settings) {
    let preference = settings.string(GNOME_COLOR_SCHEME_KEY);

    let (color_scheme, label) = match preference.as_str() {
        "prefer-dark" => (adw::ColorScheme::PreferDark, "sombre"),
        "prefer-light" => (adw::ColorScheme::PreferLight, "clair"),
        _ => (adw::ColorScheme::Default, "automatique"),
    };

    adw::StyleManager::default().set_color_scheme(color_scheme);

    println!("Thème ClipH actif : {label} ({preference})",);
}

fn run_application() -> glib::ExitCode {
    let start_hidden = std::env::args_os().any(|argument| argument == "--background");

    let app = adw::Application::builder().application_id(APP_ID).build();

    let interface_settings = gio::Settings::new(GNOME_INTERFACE_SCHEMA);

    let settings_for_startup = interface_settings.clone();

    app.connect_startup(move |_| {
        apply_system_color_scheme(&settings_for_startup);
    });

    interface_settings.connect_changed(Some(GNOME_COLOR_SCHEME_KEY), move |settings, _| {
        apply_system_color_scheme(settings);
    });

    let _hold_guard = app.hold();

    app.connect_activate(move |app| {
        if app.windows().is_empty() {
            build_ui(app, start_hidden);
            return;
        }

        let window = app
            .active_window()
            .or_else(|| app.windows().into_iter().next());

        let Some(window) = window else {
            return;
        };

        if window.is_visible() {
            window.hide();
        } else {
            window.present();
        }
    });

    let exit_code = app.run_with_args(&["cliph"]);

    drop(interface_settings);

    exit_code
}

fn main() -> glib::ExitCode {
    if let Some(exit_code) = installer::dispatch_cli() {
        std::process::exit(exit_code);
    }

    run_application()
}

#[cfg(test)]
mod shortcut_tests {
    use super::shortcut_binding_is_super_p;

    #[test]
    fn detects_gnome_super_p_binding() {
        assert!(shortcut_binding_is_super_p("<Super>p"));
    }

    #[test]
    fn detects_meta_p_binding() {
        assert!(shortcut_binding_is_super_p("<Meta>p"));
    }

    #[test]
    fn detection_is_case_insensitive() {
        assert!(shortcut_binding_is_super_p("<SUPER>P"));
    }

    #[test]
    fn ignores_unrelated_shortcuts() {
        assert!(!shortcut_binding_is_super_p("<Super>h"));
        assert!(!shortcut_binding_is_super_p("<Control><Super>p"));
        assert!(!shortcut_binding_is_super_p("<Super>Page_Up"));
    }
}
