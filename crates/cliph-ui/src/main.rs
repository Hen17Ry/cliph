use adw::prelude::*;
use gtk::{Align, Orientation, glib};

const APP_ID: &str = "com.cliph.ClipH";

fn build_ui(app: &adw::Application) {
    let header_bar = adw::HeaderBar::new();

    let title = gtk::Label::new(Some("ClipH"));
    title.add_css_class("title-1");

    let subtitle = gtk::Label::new(Some(
        "Votre gestionnaire intelligent de presse-papiers pour Linux",
    ));
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk::Justification::Center);
    subtitle.add_css_class("dim-label");

    let status = gtk::Label::new(Some(
        "✓ Socle GTK 4 et Libadwaita opérationnel",
    ));
    status.add_css_class("heading");

    let shortcut = gtk::Label::new(Some(
        "Le panneau sera bientôt accessible avec Super + H",
    ));
    shortcut.set_wrap(true);
    shortcut.set_justify(gtk::Justification::Center);

    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_margin_top(32);
    content.set_margin_bottom(32);
    content.set_margin_start(32);
    content.set_margin_end(32);

    content.append(&title);
    content.append(&subtitle);
    content.append(&status);
    content.append(&shortcut);

    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.append(&header_bar);
    page.append(&content);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("ClipH")
        .default_width(700)
        .default_height(480)
        .build();

    window.set_content(Some(&page));
    window.present();
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(build_ui);

    app.run()
}
