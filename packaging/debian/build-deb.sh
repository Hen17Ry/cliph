#!/usr/bin/env bash
set -Eeuo pipefail

PACKAGE_NAME="cliph"
APP_ID="com.cliph.ClipH"
SERVICE_NAME="app-${APP_ID}.service"
BINARY_CRATE="cliph-ui"
BINARY_SOURCE_NAME="cliph-ui"
BINARY_INSTALL_NAME="cliph"
MAINTAINER="Henry Gossou <Hen17Ry@users.noreply.github.com>"
HOMEPAGE="https://github.com/Hen17Ry"

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
BUILD_ROOT="$PROJECT_ROOT/target/debian-package"
PACKAGE_ROOT="$BUILD_ROOT/${PACKAGE_NAME}_${VERSION}_${ARCHITECTURE}"
OUTPUT_DIRECTORY="$PROJECT_ROOT/dist"
OUTPUT_FILE="$OUTPUT_DIRECTORY/${PACKAGE_NAME}_${VERSION}_${ARCHITECTURE}.deb"
RELEASE_BINARY="$PROJECT_ROOT/target/release/$BINARY_SOURCE_NAME"

printf '\n'
printf '╭──────────────────────────────────────────────╮\n'
printf '│       CLIPH BY HENRY GOSSOU — DEBIAN        │\n'
printf '╰──────────────────────────────────────────────╯\n'
printf '\n'
printf 'Version       : %s\n' "$VERSION"
printf 'Architecture  : %s\n' "$ARCHITECTURE"
printf 'Sortie        : %s\n' "$OUTPUT_FILE"
printf '\n'

printf '◆ Compilation release\n'
cargo build --release -p "$BINARY_CRATE"

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
    "$PACKAGE_ROOT/etc/systemd/user/graphical-session.target.wants" \
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

printf '◆ Création du lanceur de bureau\n'

cat > "$PACKAGE_ROOT/usr/share/applications/${APP_ID}.desktop" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=ClipH By Henry Gossou
GenericName=Gestionnaire de presse-papiers
Comment=Historique et insertion rapide du presse-papiers
Exec=/usr/bin/cliph
Icon=edit-paste-symbolic
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

ln -s \
    "/usr/lib/systemd/user/$SERVICE_NAME" \
    "$PACKAGE_ROOT/etc/systemd/user/graphical-session.target.wants/$SERVICE_NAME"

cat > "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/README.Debian" <<'EOF'
ClipH pour Debian et Ubuntu
===========================

ClipH démarre automatiquement à l'ouverture d'une session graphique.

Raccourci :
  Super/Windows + H

Commandes utiles :
  cliph --version
  systemctl --user status app-com.cliph.ClipH.service
  journalctl --user -u app-com.cliph.ClipH.service
EOF

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

DEPENDS="$SHLIBS_DEPENDS, xdg-desktop-portal"

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

for runtime_dir in /run/user/[0-9]*; do
    [ -d "$runtime_dir" ] || continue
    [ -S "$runtime_dir/bus" ] || continue

    uid="${runtime_dir##*/}"
    username="$(getent passwd "$uid" | cut -d: -f1)"

    [ -n "$username" ] || continue

    runuser -u "$username" -- \
        env \
        XDG_RUNTIME_DIR="$runtime_dir" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus" \
        systemctl --user daemon-reload ||
        true

    runuser -u "$username" -- \
        env \
        XDG_RUNTIME_DIR="$runtime_dir" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus" \
        systemctl --user restart "$SERVICE" ||
        true
done

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications || true
fi

exit 0
EOF

cat > "$PACKAGE_ROOT/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e

SERVICE="app-com.cliph.ClipH.service"

if [ "${1:-}" = "remove" ]; then
    for runtime_dir in /run/user/[0-9]*; do
        [ -d "$runtime_dir" ] || continue
        [ -S "$runtime_dir/bus" ] || continue

        uid="${runtime_dir##*/}"
        username="$(getent passwd "$uid" | cut -d: -f1)"

        [ -n "$username" ] || continue

        runuser -u "$username" -- \
            env \
            XDG_RUNTIME_DIR="$runtime_dir" \
            DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus" \
            systemctl --user stop "$SERVICE" ||
            true
    done
fi

exit 0
EOF

cat > "$PACKAGE_ROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

for runtime_dir in /run/user/[0-9]*; do
    [ -d "$runtime_dir" ] || continue
    [ -S "$runtime_dir/bus" ] || continue

    uid="${runtime_dir##*/}"
    username="$(getent passwd "$uid" | cut -d: -f1)"

    [ -n "$username" ] || continue

    runuser -u "$username" -- \
        env \
        XDG_RUNTIME_DIR="$runtime_dir" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus" \
        systemctl --user daemon-reload ||
        true
done

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications || true
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

    find usr etc \
        -type f \
        -print0 |
    sort -z |
    xargs -0 md5sum \
        > DEBIAN/md5sums
)

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
