//! Installation locale et gestion du service utilisateur ClipH.
//!
//! La commande `cliph install` installe le binaire dans `~/.local/bin`,
//! crée un service systemd utilisateur, l'active et le démarre immédiatement.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

const SERVICE_NAME: &str = "app-com.cliph.ClipH.service";
const LEGACY_SERVICE_NAME: &str = "cliph.service";
const DESKTOP_FILE_NAME: &str = "com.cliph.ClipH.desktop";
const INSTALLED_BINARY_NAME: &str = "cliph";
const GITHUB_URL: &str = "https://github.com/Hen17Ry";
const LINKEDIN_URL: &str = "https://www.linkedin.com/in/henrygossou/";

/// Intercepte les commandes d'administration avant le démarrage de GTK.
///
/// `None` signifie que ClipH doit continuer son démarrage graphique normal.
/// `Some(code)` signifie que la commande CLI a été traitée et que le
/// processus doit se terminer avec ce code.
pub fn dispatch_cli() -> Option<i32> {
    let command = env::args_os().nth(1)?;

    match command.to_str() {
        Some("install") => Some(run_cli(install)),
        Some("uninstall") => Some(run_cli(uninstall)),
        Some("status") => Some(run_cli(show_status)),
        Some("help" | "--help" | "-h") => {
            print_help();
            Some(0)
        }
        Some("version" | "--version" | "-V") => {
            println!("ClipH By Henry Gossou {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        _ => None,
    }
}

fn run_cli(operation: fn() -> Result<(), InstallerError>) -> i32 {
    match operation() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!();
            eprintln!("Erreur : {error}");
            eprintln!(
                "Consultez les journaux avec : \
                 journalctl --user -u {SERVICE_NAME} -n 100"
            );
            1
        }
    }
}

fn install() -> Result<(), InstallerError> {
    print_banner();
    println!();
    print_step("Préparation de l’installation");

    ensure_systemctl_available()?;

    let paths = InstallPaths::discover()?;
    paths.create_parent_directories()?;

    /*
     * Suppression de l’ancienne installation.
     * Le portail reconnaît la nouvelle unité grâce à son
     * nom app-com.cliph.ClipH.service.
     */
    if paths.legacy_service_path.exists() {
        let _ = systemctl(["disable", "--now", LEGACY_SERVICE_NAME]);

        remove_file_if_present(&paths.legacy_service_path)?;
    }

    if paths.service_path.exists() {
        let _ = systemctl(["stop", SERVICE_NAME]);
    }

    print_step("Installation du binaire");

    install_current_executable(&paths.binary_path)?;

    print_success(&format!(
        "Binaire installé dans {}",
        paths.binary_path.display()
    ));

    print_step("Intégration de ClipH au bureau");

    write_atomic(
        &paths.desktop_path,
        desktop_entry(&paths.binary_path).as_bytes(),
        0o644,
    )?;

    if let Some(applications_directory) = paths.desktop_path.parent() {
        let _ = Command::new("update-desktop-database")
            .arg(applications_directory)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    print_success(&format!(
        "Application enregistrée dans {}",
        paths.desktop_path.display()
    ));

    print_step("Création du service automatique");

    write_atomic(&paths.service_path, service_unit().as_bytes(), 0o644)?;

    print_success(&format!(
        "Service créé dans {}",
        paths.service_path.display()
    ));

    print_step("Activation au démarrage de la session");

    systemctl_checked(["daemon-reload"])?;
    systemctl_checked(["enable", "--now", SERVICE_NAME])?;

    let active = systemctl_success(["is-active", "--quiet", SERVICE_NAME]);

    let enabled = systemctl_success(["is-enabled", "--quiet", SERVICE_NAME]);

    if !active || !enabled {
        return Err(InstallerError::Message(String::from(
            "le service a été créé mais son activation n’a pas pu être confirmée",
        )));
    }

    println!();
    print_completion_box();

    Ok(())
}

fn uninstall() -> Result<(), InstallerError> {
    print_banner();
    println!();
    print_step("Désinstallation de ClipH");

    let paths = InstallPaths::discover()?;

    if command_exists("systemctl") {
        if paths.service_path.exists() {
            let _ = systemctl(["disable", "--now", SERVICE_NAME]);
        }

        if paths.legacy_service_path.exists() {
            let _ = systemctl(["disable", "--now", LEGACY_SERVICE_NAME]);
        }
    }

    remove_file_if_present(&paths.service_path)?;
    remove_file_if_present(&paths.legacy_service_path)?;
    remove_file_if_present(&paths.desktop_path)?;
    remove_file_if_present(&paths.binary_path)?;

    if command_exists("systemctl") {
        let _ = systemctl(["daemon-reload"]);
    }

    if let Some(applications_directory) = paths.desktop_path.parent() {
        let _ = Command::new("update-desktop-database")
            .arg(applications_directory)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    print_success("Le binaire, le service et le lanceur ont été supprimés.");

    println!(
        "L’historique personnel n’a pas été supprimé : \
         ~/.local/share/cliph"
    );

    Ok(())
}

fn show_status() -> Result<(), InstallerError> {
    print_small_header();

    let paths = InstallPaths::discover()?;
    let binary_installed = paths.binary_path.is_file();
    let service_installed = paths.service_path.is_file();
    let active =
        command_exists("systemctl") && systemctl_success(["is-active", "--quiet", SERVICE_NAME]);
    let enabled =
        command_exists("systemctl") && systemctl_success(["is-enabled", "--quiet", SERVICE_NAME]);

    println!(
        "  Binaire installé      : {}",
        status_word(binary_installed)
    );
    println!(
        "  Service installé      : {}",
        status_word(service_installed)
    );
    println!("  Démarrage automatique : {}", status_word(enabled));
    println!("  Processus actif        : {}", status_word(active));
    println!();
    println!("  Binaire : {}", paths.binary_path.display());
    println!("  Service : {}", paths.service_path.display());

    if active {
        println!();
        println!("ClipH est prêt. Utilisez Windows + H.");
        Ok(())
    } else {
        Err(InstallerError::Message(String::from(
            "ClipH n’est pas actuellement actif",
        )))
    }
}

fn print_help() {
    print_small_header();
    println!("Gestionnaire de presse-papiers pour Linux");
    println!();
    println!("UTILISATION");
    println!("  cliph                  Ouvrir ClipH");
    println!("  cliph --background     Démarrer en arrière-plan");
    println!("  cliph install          Installer et activer ClipH");
    println!("  cliph status           Vérifier l’installation");
    println!("  cliph uninstall        Désinstaller ClipH");
    println!("  cliph --version        Afficher la version");
    println!();
    println!("Raccourci global : Windows + H");
}

fn install_current_executable(destination: &Path) -> Result<(), InstallerError> {
    let source = env::current_exe().map_err(InstallerError::Io)?;

    let source = source.canonicalize().map_err(InstallerError::Io)?;
    let destination_absolute = absolute_without_canonicalizing(destination)?;

    if source == destination_absolute {
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(destination, permissions).map_err(InstallerError::Io)?;
        return Ok(());
    }

    let temporary_path = destination.with_extension(format!("installing-{}", std::process::id()));

    if temporary_path.exists() {
        fs::remove_file(&temporary_path).map_err(InstallerError::Io)?;
    }

    fs::copy(&source, &temporary_path).map_err(|error| {
        InstallerError::Message(format!(
            "impossible de copier {} vers {} : {error}",
            source.display(),
            temporary_path.display()
        ))
    })?;

    fs::set_permissions(&temporary_path, fs::Permissions::from_mode(0o755))
        .map_err(InstallerError::Io)?;

    fs::rename(&temporary_path, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary_path);
        InstallerError::Message(format!(
            "impossible d’installer {} : {error}",
            destination.display()
        ))
    })?;

    Ok(())
}

fn absolute_without_canonicalizing(path: &Path) -> Result<PathBuf, InstallerError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(env::current_dir().map_err(InstallerError::Io)?.join(path))
}

fn desktop_entry(binary_path: &Path) -> String {
    let executable = binary_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    format!(
        r#"[Desktop Entry]
Version=1.0
Type=Application
Name=ClipH By Henry Gossou
GenericName=Gestionnaire de presse-papiers
Comment=Historique et insertion rapide du presse-papiers
Exec="{executable}"
Icon=com.cliph.ClipH
Terminal=false
Categories=Utility;
StartupNotify=false
DBusActivatable=false
StartupWMClass=com.cliph.ClipH
"#
    )
}

fn service_unit() -> String {
    String::from(
        r#"[Unit]
Description=ClipH By Henry Gossou - Clipboard Manager
Documentation=https://github.com/Hen17Ry
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/cliph --background
Restart=always
RestartSec=3
TimeoutStopSec=5

[Install]
WantedBy=graphical-session.target
"#,
    )
}

fn write_atomic(destination: &Path, content: &[u8], mode: u32) -> Result<(), InstallerError> {
    let temporary_path = destination.with_extension(format!("installing-{}", std::process::id()));

    if temporary_path.exists() {
        fs::remove_file(&temporary_path).map_err(InstallerError::Io)?;
    }

    fs::write(&temporary_path, content).map_err(InstallerError::Io)?;
    fs::set_permissions(&temporary_path, fs::Permissions::from_mode(mode))
        .map_err(InstallerError::Io)?;

    fs::rename(&temporary_path, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary_path);
        InstallerError::Message(format!(
            "impossible d’écrire {} : {error}",
            destination.display()
        ))
    })
}

fn remove_file_if_present(path: &Path) -> Result<(), InstallerError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallerError::Io(error)),
    }
}

fn ensure_systemctl_available() -> Result<(), InstallerError> {
    if command_exists("systemctl") {
        Ok(())
    } else {
        Err(InstallerError::Message(String::from(
            "systemctl est introuvable sur ce système",
        )))
    }
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn systemctl<I, S>(arguments: I) -> Result<ExitStatus, InstallerError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .status()
        .map_err(InstallerError::Io)
}

fn systemctl_checked<I, S>(arguments: I) -> Result<(), InstallerError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = systemctl(arguments)?;

    if status.success() {
        Ok(())
    } else {
        Err(InstallerError::Message(format!(
            "systemctl a terminé avec le code {}",
            status
                .code()
                .map_or_else(|| String::from("inconnu"), |code| { code.to_string() })
        )))
    }
}

fn systemctl_success<const N: usize>(arguments: [&str; N]) -> bool {
    systemctl(arguments).is_ok_and(|status| status.success())
}

fn status_word(value: bool) -> &'static str {
    if value { "oui" } else { "non" }
}

fn print_banner() {
    let color = TerminalColors::detect();

    let cliph_art = [
        "  ██████╗██╗     ██╗██████╗ ██╗  ██╗",
        " ██╔════╝██║     ██║██╔══██╗██║  ██║",
        " ██║     ██║     ██║██████╔╝███████║",
        " ██║     ██║     ██║██╔═══╝ ██╔══██║",
        " ╚██████╗███████╗██║██║     ██║  ██║",
        "  ╚═════╝╚══════╝╚═╝╚═╝     ╚═╝  ╚═╝",
    ];

    let by_art = [
        " ██████╗ ██╗   ██╗",
        " ██╔══██╗╚██╗ ██╔╝",
        " ██████╔╝ ╚████╔╝ ",
        " ██╔══██╗  ╚██╔╝  ",
        " ██████╔╝   ██║   ",
        " ╚═════╝    ╚═╝   ",
    ];

    let henry_art = [
        " ██╗  ██╗███████╗███╗   ██╗██████╗ ██╗   ██╗",
        " ██║  ██║██╔════╝████╗  ██║██╔══██╗╚██╗ ██╔╝",
        " ███████║█████╗  ██╔██╗ ██║██████╔╝ ╚████╔╝ ",
        " ██╔══██║██╔══╝  ██║╚██╗██║██╔══██╗  ╚██╔╝  ",
        " ██║  ██║███████╗██║ ╚████║██║  ██║   ██║   ",
        " ╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝   ╚═╝   ",
    ];

    let gossou_art = [
        "  ██████╗  ██████╗ ███████╗███████╗ ██████╗ ██╗   ██╗",
        " ██╔════╝ ██╔═══██╗██╔════╝██╔════╝██╔═══██╗██║   ██║",
        " ██║  ███╗██║   ██║███████╗███████╗██║   ██║██║   ██║",
        " ██║   ██║██║   ██║╚════██║╚════██║██║   ██║██║   ██║",
        " ╚██████╔╝╚██████╔╝███████║███████║╚██████╔╝╚██████╔╝",
        "  ╚═════╝  ╚═════╝ ╚══════╝╚══════╝ ╚═════╝  ╚═════╝ ",
    ];

    println!();

    for line in cliph_art {
        println!("{}{line}{}", color.cyan, color.reset);
    }

    println!();

    for line in by_art {
        println!("{}{line}{}", color.blue, color.reset);
    }

    println!();

    for line in henry_art {
        println!("{}{line}{}", color.magenta, color.reset);
    }

    println!();

    for line in gossou_art {
        println!("{}{line}{}", color.pink, color.reset);
    }

    println!();
    println!(
        "             {}CLIPH BY HENRY GOSSOU{}",
        color.bold, color.reset
    );
    println!(
        "          {}Votre presse-papiers, toujours prêt.{}",
        color.dim, color.reset
    );
    println!();
    println!(
        "  {}GitHub{}    {}",
        color.bold,
        color.reset,
        terminal_link(GITHUB_URL, GITHUB_URL, color.enabled)
    );
    println!(
        "  {}LinkedIn{}  {}",
        color.bold,
        color.reset,
        terminal_link(LINKEDIN_URL, LINKEDIN_URL, color.enabled)
    );
}

fn print_small_header() {
    let color = TerminalColors::detect();
    println!(
        "{}ClipH By Henry Gossou{} • {}",
        color.bold,
        color.reset,
        env!("CARGO_PKG_VERSION")
    );
    println!();
}

fn print_step(message: &str) {
    let color = TerminalColors::detect();
    println!("{}◆{} {message}", color.cyan, color.reset);
    let _ = io::stdout().flush();
}

fn print_success(message: &str) {
    let color = TerminalColors::detect();
    println!("  {}✓{} {message}", color.green, color.reset);
}

fn print_completion_box() {
    let color = TerminalColors::detect();

    println!(
        "{}╭────────────────────────────────────────────────────╮{}",
        color.green, color.reset
    );
    println!(
        "{}│{}  {}ClipH By Henry Gossou est maintenant installé.{}          {}│{}",
        color.green, color.reset, color.bold, color.reset, color.green, color.reset
    );
    println!(
        "{}│{}                                                    {}│{}",
        color.green, color.reset, color.green, color.reset
    );
    println!(
        "{}│{}  Il démarrera automatiquement à chaque session.   {}│{}",
        color.green, color.reset, color.green, color.reset
    );
    println!(
        "{}│{}  Utilisez {}Windows + H{} pour ouvrir ClipH.             {}│{}",
        color.green, color.reset, color.bold, color.reset, color.green, color.reset
    );
    println!(
        "{}╰────────────────────────────────────────────────────╯{}",
        color.green, color.reset
    );
}

fn terminal_link(label: &str, url: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
    } else {
        label.to_owned()
    }
}

#[derive(Debug)]
struct InstallPaths {
    binary_path: PathBuf,
    service_path: PathBuf,
    legacy_service_path: PathBuf,
    desktop_path: PathBuf,
}

impl InstallPaths {
    fn discover() -> Result<Self, InstallerError> {
        let home = env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            InstallerError::Message(String::from("la variable HOME est indisponible"))
        })?;

        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));

        let data_home = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));

        let systemd_user_directory = config_home.join("systemd").join("user");

        Ok(Self {
            binary_path: home.join(".local").join("bin").join(INSTALLED_BINARY_NAME),

            service_path: systemd_user_directory.join(SERVICE_NAME),

            legacy_service_path: systemd_user_directory.join(LEGACY_SERVICE_NAME),

            desktop_path: data_home.join("applications").join(DESKTOP_FILE_NAME),
        })
    }

    fn create_parent_directories(&self) -> Result<(), InstallerError> {
        for path in [
            &self.binary_path,
            &self.service_path,
            &self.legacy_service_path,
            &self.desktop_path,
        ] {
            let parent = path.parent().ok_or_else(|| {
                InstallerError::Message(format!("{} n’a pas de dossier parent", path.display()))
            })?;

            fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        }

        Ok(())
    }
}

struct TerminalColors {
    enabled: bool,
    bold: &'static str,
    dim: &'static str,
    cyan: &'static str,
    blue: &'static str,
    magenta: &'static str,
    pink: &'static str,
    green: &'static str,
    reset: &'static str,
}

impl TerminalColors {
    fn detect() -> Self {
        let enabled = io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();

        if enabled {
            Self {
                enabled,
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                cyan: "\x1b[38;5;51m",
                blue: "\x1b[38;5;39m",
                magenta: "\x1b[38;5;99m",
                pink: "\x1b[38;5;201m",
                green: "\x1b[38;5;82m",
                reset: "\x1b[0m",
            }
        } else {
            Self {
                enabled,
                bold: "",
                dim: "",
                cyan: "",
                blue: "",
                magenta: "",
                pink: "",
                green: "",
                reset: "",
            }
        }
    }
}

#[derive(Debug)]
enum InstallerError {
    Io(io::Error),
    Message(String),
}

impl std::fmt::Display for InstallerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for InstallerError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_starts_cliph_in_background() {
        let unit = service_unit();

        assert!(unit.contains("ExecStart=%h/.local/bin/cliph --background"));
    }

    #[test]
    fn service_is_bound_to_graphical_session() {
        let unit = service_unit();

        assert!(unit.contains("WantedBy=graphical-session.target"));
        assert!(unit.contains("PartOf=graphical-session.target"));
    }

    #[test]
    fn service_always_restarts() {
        assert!(service_unit().contains("Restart=always"));
    }

    #[test]
    fn branding_uses_expected_links() {
        assert_eq!(GITHUB_URL, "https://github.com/Hen17Ry");
        assert_eq!(LINKEDIN_URL, "https://www.linkedin.com/in/henrygossou/");
    }
}
