#!/usr/bin/env bash
set -Eeuo pipefail

umask 022

PACKAGE_NAME="cliph"
APP_ID="com.cliph.ClipH"
SERVICE_NAME="app-${APP_ID}.service"
BINARY_CRATE="cliph-ui"
BINARY_SOURCE_NAME="cliph-ui"
BINARY_INSTALL_NAME="cliph"
MAINTAINER="Henry Gossou <Hen17Ry@users.noreply.github.com>"
HOMEPAGE="https://github.com/Hen17Ry"

BUILD_PROFILE="${CLIPH_BUILD_PROFILE:-modern}"
PACKAGE_VARIANT="${CLIPH_PACKAGE_VARIANT:-}"

case "$BUILD_PROFILE" in
    modern)
        CARGO_BUILD_ARGS=(
            --release
            -p "$BINARY_CRATE"
            --no-default-features
            --features "$BINARY_CRATE/gtk-v4-8"
        )
        ;;
    legacy)
        CARGO_BUILD_ARGS=(
            --release
            -p "$BINARY_CRATE"
            --no-default-features
        )
        ;;
    *)
        printf 'Profil de compilation inconnu : %s\n'             "$BUILD_PROFILE" >&2
        printf 'Profils acceptés : modern, legacy\n' >&2
        exit 1
        ;;
esac

if [[ -n "$PACKAGE_VARIANT" ]] &&
   [[ ! "$PACKAGE_VARIANT" =~ ^[A-Za-z0-9._+-]+$ ]]
then
    printf 'Variante de paquet invalide : %s\n'         "$PACKAGE_VARIANT" >&2
    exit 1
fi

PROJECT_ROOT="$(
    cd "$(dirname "${BASH_SOURCE[0]}")/../.." &&
    pwd
)"

cd "$PROJECT_ROOT"

need_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'Commande requise introuvable : %s\n' "$1" >&2
        exit 1
    fi
}

for command in \
    cargo \
    dpkg \
    dpkg-deb \
    dpkg-shlibdeps \
    dpkg-query \
    gzip \
    md5sum \
    find \
    sed \
    awk \
    python3
do
    need_command "$command"
done

VERSION="$(
    cargo metadata \
        --no-deps \
        --format-version 1 |
    python3 -c '
import json
import sys

metadata = json.load(sys.stdin)

for package in metadata["packages"]:
    if package["name"] == "cliph-ui":
        print(package["version"])
        break
else:
    raise SystemExit("Version de cliph-ui introuvable.")
'
)"

ARCHITECTURE="$(dpkg --print-architecture)"

CARGO_TARGET_DIRECTORY="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"

BUILD_ROOT="$CARGO_TARGET_DIRECTORY/debian-package"
OUTPUT_DIRECTORY="$PROJECT_ROOT/dist"

VARIANT_SUFFIX=""

if [[ -n "$PACKAGE_VARIANT" ]]; then
    VARIANT_SUFFIX="_$PACKAGE_VARIANT"
fi

PACKAGE_ROOT="${BUILD_ROOT}/${PACKAGE_NAME}_${VERSION}${VARIANT_SUFFIX}_${ARCHITECTURE}"

OUTPUT_FILE="${OUTPUT_DIRECTORY}/${PACKAGE_NAME}_${VERSION}${VARIANT_SUFFIX}_${ARCHITECTURE}.deb"

RELEASE_BINARY="$CARGO_TARGET_DIRECTORY/release/$BINARY_SOURCE_NAME"

printf '\n'
printf '╭──────────────────────────────────────────────╮\n'
printf '│       CLIPH BY HENRY GOSSOU — DEBIAN        │\n'
printf '╰──────────────────────────────────────────────╯\n'
printf '\n'
printf 'Version       : %s\n' "$VERSION"
printf 'Architecture  : %s\n' "$ARCHITECTURE"
printf 'Profil GTK    : %s\n' "$BUILD_PROFILE"
printf 'Variante      : %s\n' "${PACKAGE_VARIANT:-générique}"
printf 'Sortie        : %s\n' "$OUTPUT_FILE"
printf '\n'

printf '◆ Compilation release\n'
cargo build "${CARGO_BUILD_ARGS[@]}"

if [[ ! -x "$RELEASE_BINARY" ]]; then
    printf 'Binaire release introuvable : %s\n' "$RELEASE_BINARY" >&2
    exit 1
fi

rm -rf "$PACKAGE_ROOT"
mkdir -p \
    "$PACKAGE_ROOT/DEBIAN" \
    "$PACKAGE_ROOT/usr/bin" \
    "$PACKAGE_ROOT/usr/lib/systemd/user" \
    "$PACKAGE_ROOT/usr/share/applications" \
    "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME" \
    "$OUTPUT_DIRECTORY" \
    "$BUILD_ROOT/debian"

install -Dm755 \
    "$RELEASE_BINARY" \
    "$PACKAGE_ROOT/usr/bin/$BINARY_INSTALL_NAME"

if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded \
        "$PACKAGE_ROOT/usr/bin/$BINARY_INSTALL_NAME" ||
        true
fi

printf '◆ Installation des icônes officielles\n'

for size in 16 24 32 48 64 128 256 512; do
    icon_source="$PROJECT_ROOT/assets/icons/hicolor/${size}x${size}/apps/${APP_ID}.png"
    icon_destination="$PACKAGE_ROOT/usr/share/icons/hicolor/${size}x${size}/apps/${APP_ID}.png"

    if [[ ! -f "$icon_source" ]]; then
        printf 'Icône manquante : %s\n' "$icon_source" >&2
        exit 1
    fi

    install -Dm644 \
        "$icon_source" \
        "$icon_destination"
done

printf '◆ Création du lanceur de bureau\n'

cat > "$PACKAGE_ROOT/usr/share/applications/${APP_ID}.desktop" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=ClipH By Henry Gossou
GenericName=Gestionnaire de presse-papiers
Comment=Historique et insertion rapide du presse-papiers
Exec=/usr/bin/cliph
Icon=com.cliph.ClipH
Terminal=false
Categories=Utility;
Keywords=clipboard;presse-papiers;history;historique;emoji;symbol;
StartupNotify=false
DBusActivatable=false
StartupWMClass=${APP_ID}
EOF

printf '◆ Création du service utilisateur\n'

cat > "$PACKAGE_ROOT/usr/lib/systemd/user/$SERVICE_NAME" <<EOF
[Unit]
Description=ClipH By Henry Gossou - Clipboard Manager
Documentation=$HOMEPAGE
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/cliph --background
Restart=always
RestartSec=3
TimeoutStopSec=5

[Install]
WantedBy=graphical-session.target
EOF

cat > "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/README.Debian" <<'EOF'
ClipH pour Debian et Ubuntu
===========================

ClipH démarre automatiquement à l'ouverture d'une session graphique.

Raccourcis :
  Super/Windows + H
  Ctrl + Super/Windows + H lorsque le raccourci principal est occupé

Commandes utiles :
  cliph --version
  systemctl --user status app-com.cliph.ClipH.service
  journalctl --user -u app-com.cliph.ClipH.service
EOF

printf '◆ Installation de la documentation Debian\n'

install -Dm644 \
    "$PROJECT_ROOT/packaging/debian/copyright" \
    "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/copyright"

gzip -9 -n -c \
    "$PROJECT_ROOT/packaging/debian/changelog" \
    > "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/changelog.gz"

mkdir -p "$PACKAGE_ROOT/usr/share/man/man1"

gzip -9 -n -c \
    "$PROJECT_ROOT/packaging/debian/cliph.1" \
    > "$PACKAGE_ROOT/usr/share/man/man1/cliph.1.gz"

chmod 0644 \
    "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/copyright" \
    "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/changelog.gz" \
    "$PACKAGE_ROOT/usr/share/man/man1/cliph.1.gz"

printf '◆ Détection des dépendances système\n'

cat > "$BUILD_ROOT/debian/control" <<EOF
Source: $PACKAGE_NAME
Section: utils
Priority: optional
Maintainer: $MAINTAINER
Standards-Version: 4.7.0

Package: $PACKAGE_NAME
Architecture: any
Description: Gestionnaire de presse-papiers pour Linux
EOF

SHLIBS_OUTPUT="$(
    cd "$BUILD_ROOT"
    dpkg-shlibdeps \
        -O \
        -e"$PACKAGE_ROOT/usr/bin/$BINARY_INSTALL_NAME"
)"

SHLIBS_DEPENDS="$(
    printf '%s\n' "$SHLIBS_OUTPUT" |
    sed -n 's/^shlibs:Depends=//p'
)"

if [[ -z "$SHLIBS_DEPENDS" ]]; then
    printf 'Impossible de déterminer les dépendances partagées.\n' >&2
    exit 1
fi

DEPENDS="$SHLIBS_DEPENDS, xdg-desktop-portal, hicolor-icon-theme"

INSTALLED_SIZE="$(
    du -sk "$PACKAGE_ROOT" |
    awk '{print $1}'
)"

cat > "$PACKAGE_ROOT/DEBIAN/control" <<EOF
Package: $PACKAGE_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCHITECTURE
Maintainer: $MAINTAINER
Installed-Size: $INSTALLED_SIZE
Depends: $DEPENDS
Homepage: $HOMEPAGE
Description: Gestionnaire de presse-papiers rapide pour Linux
 ClipH conserve l'historique des textes, contenus riches, images et
 fichiers copiés. Il fournit également un panneau hors ligne pour les
 emojis, kaomojis et symboles Unicode, accessible avec Super+H.
EOF

cat > "$PACKAGE_ROOT/DEBIAN/preinst" <<'EOF'
#!/bin/sh
set -e

# Active les couleurs uniquement dans un vrai terminal.
if [ -t 1 ] &&
   [ "${TERM:-dumb}" != "dumb" ] &&
   [ -z "${NO_COLOR:-}" ]; then
    BOLD="$(printf '\033[1m')"
    CYAN="$(printf '\033[38;5;51m')"
    BLUE="$(printf '\033[38;5;39m')"
    MAGENTA="$(printf '\033[38;5;135m')"
    PINK="$(printf '\033[38;5;201m')"
    GREEN="$(printf '\033[38;5;82m')"
    DIM="$(printf '\033[2m')"
    RESET="$(printf '\033[0m')"
else
    BOLD=""
    CYAN=""
    BLUE=""
    MAGENTA=""
    PINK=""
    GREEN=""
    DIM=""
    RESET=""
fi

cat <<BANNER

${CYAN}  ██████╗██╗     ██╗██████╗ ██╗  ██╗
 ██╔════╝██║     ██║██╔══██╗██║  ██║
 ██║     ██║     ██║██████╔╝███████║
 ██║     ██║     ██║██╔═══╝ ██╔══██║
 ╚██████╗███████╗██║██║     ██║  ██║
  ╚═════╝╚══════╝╚═╝╚═╝     ╚═╝  ╚═╝${RESET}

${BLUE} ██████╗ ██╗   ██╗
 ██╔══██╗╚██╗ ██╔╝
 ██████╔╝ ╚████╔╝
 ██╔══██╗  ╚██╔╝
 ██████╔╝   ██║
 ╚═════╝    ╚═╝${RESET}

${MAGENTA} ██╗  ██╗███████╗███╗   ██╗██████╗ ██╗   ██╗
 ██║  ██║██╔════╝████╗  ██║██╔══██╗╚██╗ ██╔╝
 ███████║█████╗  ██╔██╗ ██║██████╔╝ ╚████╔╝
 ██╔══██║██╔══╝  ██║╚██╗██║██╔══██╗  ╚██╔╝
 ██║  ██║███████╗██║ ╚████║██║  ██║   ██║
 ╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝   ╚═╝${RESET}

${PINK}  ██████╗  ██████╗ ███████╗███████╗ ██████╗ ██╗   ██╗
 ██╔════╝ ██╔═══██╗██╔════╝██╔════╝██╔═══██╗██║   ██║
 ██║  ███╗██║   ██║███████╗███████╗██║   ██║██║   ██║
 ██║   ██║██║   ██║╚════██║╚════██║██║   ██║██║   ██║
 ╚██████╔╝╚██████╔╝███████║███████║╚██████╔╝╚██████╔╝
  ╚═════╝  ╚═════╝ ╚══════╝╚══════╝ ╚═════╝  ╚═════╝${RESET}

              ${BOLD}${GREEN}CLIPH BY HENRY GOSSOU${RESET}

         ${DIM}Votre presse-papiers, toujours prêt.${RESET}

 ${BOLD}GitHub${RESET}   : ${BLUE}https://github.com/Hen17Ry${RESET}
 ${BOLD}LinkedIn${RESET} : ${BLUE}https://www.linkedin.com/in/henrygossou/${RESET}

BANNER

exit 0
EOF

cat > "$PACKAGE_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

SERVICE="app-com.cliph.ClipH.service"

case "${1:-}" in
    configure|abort-upgrade|abort-deconfigure|abort-remove)
        if [ -z "${DPKG_ROOT:-}" ] &&
           command -v deb-systemd-helper >/dev/null 2>&1; then

            deb-systemd-helper \
                --user \
                unmask "$SERVICE" \
                >/dev/null ||
                true

            # Sur une première installation, was-enabled vaut vrai.
            # Sur une mise à jour, le choix de l'utilisateur est conservé.
            if deb-systemd-helper \
                --quiet \
                --user \
                was-enabled "$SERVICE"
            then
                deb-systemd-helper \
                    --user \
                    enable "$SERVICE" \
                    >/dev/null ||
                    true
            else
                deb-systemd-helper \
                    --user \
                    update-state "$SERVICE" \
                    >/dev/null ||
                    true
            fi
        fi
        ;;
esac

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache \
        --force \
        --ignore-theme-index \
        /usr/share/icons/hicolor ||
        true
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database \
        /usr/share/applications ||
        true
fi

if command -v deb-systemd-invoke >/dev/null 2>&1; then
    deb-systemd-invoke \
        --user \
        daemon-reload ||
        true

    deb-systemd-invoke \
        --user \
        restart "$SERVICE" ||
        true
fi

exit 0
EOF

cat > "$PACKAGE_ROOT/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e

SERVICE="app-com.cliph.ClipH.service"

case "${1:-}" in
    remove|deconfigure)
        if command -v deb-systemd-invoke >/dev/null 2>&1; then
            deb-systemd-invoke --user stop "$SERVICE" || true
        fi
        ;;
esac

exit 0
EOF

cat > "$PACKAGE_ROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

SERVICE="app-com.cliph.ClipH.service"

if [ "${1:-}" = "purge" ] &&
   [ -z "${DPKG_ROOT:-}" ] &&
   command -v deb-systemd-helper >/dev/null 2>&1; then

    deb-systemd-helper \
        --user \
        purge "$SERVICE" \
        >/dev/null ||
        true
fi

if command -v deb-systemd-invoke >/dev/null 2>&1; then
    deb-systemd-invoke \
        --user \
        daemon-reload ||
        true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache \
        --force \
        --ignore-theme-index \
        /usr/share/icons/hicolor ||
        true
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database \
        /usr/share/applications ||
        true
fi

exit 0
EOF

chmod 0755 \
    "$PACKAGE_ROOT/DEBIAN/preinst" \
    "$PACKAGE_ROOT/DEBIAN/postinst" \
    "$PACKAGE_ROOT/DEBIAN/prerm" \
    "$PACKAGE_ROOT/DEBIAN/postrm"

(
    cd "$PACKAGE_ROOT"

    find usr \
        -type f \
        -print0 |
    sort -z |
    xargs -0 md5sum \
        > DEBIAN/md5sums
)

printf '◆ Normalisation des permissions Debian\n'

find "$PACKAGE_ROOT" \
    -type d \
    -exec chmod 0755 {} +

find "$PACKAGE_ROOT" \
    -type f \
    -exec chmod 0644 {} +

chmod 0755 \
    "$PACKAGE_ROOT/usr/bin/$BINARY_INSTALL_NAME" \
    "$PACKAGE_ROOT/DEBIAN/preinst" \
    "$PACKAGE_ROOT/DEBIAN/postinst" \
    "$PACKAGE_ROOT/DEBIAN/prerm" \
    "$PACKAGE_ROOT/DEBIAN/postrm"

printf '◆ Construction du paquet .deb\n'

rm -f "$OUTPUT_FILE"

dpkg-deb \
    --root-owner-group \
    --build \
    "$PACKAGE_ROOT" \
    "$OUTPUT_FILE"

printf '◆ Vérification du paquet\n'

dpkg-deb --info "$OUTPUT_FILE" >/dev/null
dpkg-deb --contents "$OUTPUT_FILE" >/dev/null

printf '\n'
printf '✓ Paquet créé avec succès\n'
printf '  %s\n' "$OUTPUT_FILE"
printf '\n'
printf 'Dépendances :\n'
printf '  %s\n' "$DEPENDS"
printf '\n'

if command -v lintian >/dev/null 2>&1; then
    printf '◆ Analyse Lintian\n'
    lintian "$OUTPUT_FILE" || true
else
    printf 'Lintian non installé : contrôle avancé ignoré.\n'
fi
