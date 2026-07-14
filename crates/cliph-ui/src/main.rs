use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use cliph_core::{
    ClipboardClassification, ClipboardFormatPayload, ClipboardItem, ClipboardItemKind,
};
use cliph_storage::{ClipboardStorage, MAX_FORMAT_PAYLOAD_BYTES, MAX_IMAGE_BYTES};
use futures_util::StreamExt;
use gtk::glib::types::StaticType;
use gtk::{Align, Orientation, gdk, gio, glib};

const APP_ID: &str = "com.cliph.ClipH";
const DISPLAYED_HISTORY_LIMIT: usize = 200;
const GLOBAL_SHORTCUT_ID: &str = "toggle-cliph";
const GLOBAL_SHORTCUT_TRIGGER: &str = "LOGO+h";
const HTML_MIME_TYPES: &[&str] = &[
    "text/html",
    "text/html;charset=utf-8",
    "text/html;charset=UTF-8",
];
const RTF_MIME_TYPES: &[&str] = &["text/rtf", "application/rtf", "application/x-rtf"];
const TSV_MIME_TYPES: &[&str] = &["text/tab-separated-values"];
const CSV_MIME_TYPES: &[&str] = &["text/csv", "application/csv"];

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
            ClipboardItemKind::Files => DisplayedPayload::Text {
                plain_text: item.plain_text.clone(),
                html_text: None,
                format_payloads,
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
}

impl PublishedKind {
    const fn success_message(self) -> &'static str {
        match self {
            Self::PlainText => "Élément copié — utilisez Ctrl + V pour le coller",
            Self::RichText => "Texte enrichi copié — formatage conservé",
            Self::MultiFormat => "Contenu copié — tous les formats disponibles sont restaurés",
            Self::Image => "Image copiée — utilisez Ctrl + V pour la coller",
        }
    }
}

type DisplayedHistory = Rc<RefCell<Vec<DisplayedItem>>>;

struct TextCaptureContext {
    storage: Rc<ClipboardStorage>,
    history_list: gtk::ListBox,
    empty_state: gtk::Label,
    counter_label: gtk::Label,
    displayed_history: DisplayedHistory,
    toast_overlay: adw::ToastOverlay,
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
        ClipboardItemKind::Text | ClipboardItemKind::Files => create_text_content(item),
        ClipboardItemKind::Image => create_image_content(item),
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
    empty_state.set_label("L’historique est vide.\n\nCopiez un texte ou une image avec Ctrl + C.");
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
    context: TextCaptureContext,
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
            TextCaptureContext {
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

fn setup_global_shortcut(window: &adw::ApplicationWindow, toast_overlay: &adw::ToastOverlay) {
    let window = window.clone();
    let toast_overlay = toast_overlay.clone();

    glib::MainContext::default().spawn_local(async move {
        if let Err(error) = run_global_shortcut(window, toast_overlay.clone()).await {
            eprintln!("Impossible d’activer Windows + H : {error}");
            show_toast(&toast_overlay, "Impossible d’activer Windows + H");
        }
    });
}

async fn run_global_shortcut(
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::new().await?;

    println!("Version du portail GlobalShortcuts : {}", portal.version());

    let session = portal.create_session(Default::default()).await?;

    let shortcut = NewShortcut::new(GLOBAL_SHORTCUT_ID, "Afficher ou masquer ClipH")
        .preferred_trigger(GLOBAL_SHORTCUT_TRIGGER);

    let bind_request = portal
        .bind_shortcuts(&session, &[shortcut], None, Default::default())
        .await?;

    let bind_response = bind_request.response()?;

    let shortcut_is_bound = bind_response
        .shortcuts()
        .iter()
        .any(|shortcut| shortcut.id() == GLOBAL_SHORTCUT_ID);

    if !shortcut_is_bound {
        eprintln!("Le raccourci Windows + H n’a pas été autorisé.");
        show_toast(&toast_overlay, "Windows + H n’a pas été autorisé");
        return Ok(());
    }

    let trigger_description = bind_response
        .shortcuts()
        .iter()
        .find(|shortcut| shortcut.id() == GLOBAL_SHORTCUT_ID)
        .map(|shortcut| shortcut.trigger_description())
        .unwrap_or("Windows + H");

    println!("Raccourci global actif : {trigger_description}");
    show_toast(&toast_overlay, "Windows + H est maintenant actif");

    let mut activations = portal.receive_activated().await?;

    while let Some(activation) = activations.next().await {
        if activation.shortcut_id() != GLOBAL_SHORTCUT_ID {
            continue;
        }

        if window.is_visible() {
            window.hide();
        } else {
            window.present();
        }
    }

    drop(session);
    Ok(())
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

    let header_title = gtk::Label::new(Some("ClipH"));
    header_title.add_css_class("heading");

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&header_title));

    let quit_button = gtk::Button::from_icon_name("application-exit-symbolic");
    quit_button.set_tooltip_text(Some("Quitter complètement ClipH"));
    quit_button.add_css_class("flat");

    let app_for_quit = app.clone();
    quit_button.connect_clicked(move |_| {
        app_for_quit.quit();
    });

    header_bar.pack_end(&quit_button);

    let title = gtk::Label::new(Some("Historique du presse-papiers"));
    title.set_halign(Align::Start);
    title.add_css_class("title-2");

    let description = gtk::Label::new(Some(
        "Cliquez sur un texte ou une image pour le remettre dans le presse-papiers.",
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
        "L’historique est vide.\n\nCopiez un texte ou une image avec Ctrl + C.",
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

    let content = gtk::Box::new(Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&introduction);
    content.append(&scrolled_window);

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

    setup_global_shortcut(&window, &toast_overlay);

    if start_hidden {
        window.hide();
    } else {
        window.present();
    }
}

fn main() -> glib::ExitCode {
    let start_hidden = std::env::args_os().any(|argument| argument == "--background");

    let app = adw::Application::builder().application_id(APP_ID).build();

    let _hold_guard = app.hold();

    app.connect_activate(move |app| {
        if app.windows().is_empty() {
            build_ui(app, start_hidden);
        }
    });

    app.run_with_args(&["cliph"])
}
