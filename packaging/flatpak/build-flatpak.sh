#!/usr/bin/env bash
set -Eeuo pipefail

umask 022

APP_ID="com.cliph.ClipH"
APP_NAME="ClipH"
BINARY_CRATE="cliph-ui"
BINARY_SOURCE_NAME="cliph-ui"
BINARY_INSTALL_NAME="cliph"

GNOME_RUNTIME_VERSION="50"
FREEDESKTOP_VERSION="25.08"
FLATPAK_BRANCH="stable"

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
    flatpak \
    flatpak-builder \
    python3 \
    sha256sum \
    tar
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

ARCHITECTURE="$(flatpak --default-arch)"
RELEASE_DATE="$(date -u +%F)"

WORK_ROOT="$PROJECT_ROOT/target/flatpak-package"
SOURCE_DIR="$WORK_ROOT/source"
BUILD_DIR="$WORK_ROOT/build"
REPOSITORY_DIR="$WORK_ROOT/repository"
MANIFEST="$WORK_ROOT/${APP_ID}.yml"

OUTPUT_DIRECTORY="$PROJECT_ROOT/dist"
BUNDLE_FILE="$OUTPUT_DIRECTORY/ClipH-${VERSION}-${ARCHITECTURE}.flatpak"
CHECKSUM_FILE="${BUNDLE_FILE}.sha256"

printf '\n'
printf '╭──────────────────────────────────────────────╮\n'
printf '│       CLIPH BY HENRY GOSSOU — FLATPAK       │\n'
printf '╰──────────────────────────────────────────────╯\n'
printf '\n'
printf 'Version       : %s\n' "$VERSION"
printf 'Architecture  : %s\n' "$ARCHITECTURE"
printf 'Runtime GNOME : %s\n' "$GNOME_RUNTIME_VERSION"
printf 'Sortie        : %s\n' "$BUNDLE_FILE"
printf '\n'

printf '◆ Configuration de Flathub\n'

flatpak remote-add \
    --user \
    --if-not-exists \
    flathub \
    https://dl.flathub.org/repo/flathub.flatpakrepo

printf '◆ Installation des runtimes de construction\n'

flatpak install \
    --user \
    -y \
    flathub \
    "org.gnome.Platform//${GNOME_RUNTIME_VERSION}" \
    "org.gnome.Sdk//${GNOME_RUNTIME_VERSION}" \
    "org.freedesktop.Sdk.Extension.rust-stable//${FREEDESKTOP_VERSION}"

printf '◆ Synchronisation de Cargo.lock\n'

cargo metadata \
    --format-version 1 \
    >/dev/null

cargo metadata \
    --locked \
    --format-version 1 \
    >/dev/null

printf '◆ Préparation de l’espace de construction\n'

rm -rf "$WORK_ROOT"

mkdir -p \
    "$SOURCE_DIR" \
    "$BUILD_DIR" \
    "$REPOSITORY_DIR" \
    "$OUTPUT_DIRECTORY"

printf '◆ Copie propre des sources\n'

tar \
    --exclude='./.git' \
    --exclude='./.flatpak-builder' \
    --exclude='./target' \
    --exclude='./dist' \
    --exclude='./flatpak-build' \
    --exclude='./flatpak-repository' \
    -cf - \
    . |
tar \
    -xf - \
    -C "$SOURCE_DIR"

printf '◆ Préparation hors ligne des dépendances Rust\n'

cargo vendor \
    --locked \
    --versioned-dirs \
    "$SOURCE_DIR/vendor" \
    >/dev/null

mkdir -p "$SOURCE_DIR/.cargo"

cat > "$SOURCE_DIR/.cargo/config.toml" <<'CARGO_CONFIG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"

[net]
offline = true
CARGO_CONFIG

mkdir -p "$SOURCE_DIR/flatpak"

printf '◆ Création du lanceur Flatpak\n'

cat > "$SOURCE_DIR/flatpak/${APP_ID}.desktop" <<DESKTOP
[Desktop Entry]
Version=1.0
Type=Application
Name=ClipH By Henry Gossou
GenericName=Gestionnaire de presse-papiers
Comment=Historique et insertion rapide du presse-papiers
Exec=cliph
Icon=${APP_ID}
Terminal=false
Categories=Utility;
Keywords=clipboard;presse-papiers;history;historique;emoji;symbol;
StartupNotify=true
DBusActivatable=false
StartupWMClass=${APP_ID}
X-Flatpak=${APP_ID}
DESKTOP

printf '◆ Création des métadonnées AppStream\n'

cat > "$SOURCE_DIR/flatpak/${APP_ID}.metainfo.xml" <<METAINFO
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>${APP_ID}</id>

  <name>ClipH</name>
  <summary>Gestionnaire moderne et persistant de presse-papiers</summary>

  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-or-later</project_license>

  <developer id="com.cliph">
    <name>Henry Gossou</name>
  </developer>

  <description>
    <p>
      ClipH conserve localement les textes, contenus riches, images et
      références de fichiers copiés.
    </p>
    <p>
      Il fournit également une recherche rapide ainsi qu’un catalogue hors
      ligne d’emojis, de kaomojis et de symboles Unicode.
    </p>
    <p>
      Les données restent séparées pour chaque utilisateur et sont stockées
      localement.
    </p>
  </description>

  <launchable type="desktop-id">${APP_ID}.desktop</launchable>

  <provides>
    <binary>cliph</binary>
  </provides>

  <url type="homepage">https://github.com/Hen17Ry/cliph</url>
  <url type="bugtracker">https://github.com/Hen17Ry/cliph/issues</url>

  <content_rating type="oars-1.1"/>

  <releases>
    <release version="${VERSION}" date="${RELEASE_DATE}">
      <description>
        <p>Première construction Flatpak de ClipH.</p>
      </description>
    </release>
  </releases>
</component>
METAINFO

printf '◆ Création du manifeste Flatpak\n'

cat > "$MANIFEST" <<MANIFEST
app-id: ${APP_ID}

runtime: org.gnome.Platform
runtime-version: '${GNOME_RUNTIME_VERSION}'
sdk: org.gnome.Sdk

sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable

command: ${BINARY_INSTALL_NAME}

finish-args:
  - --share=ipc
  - --socket=wayland
  - --socket=fallback-x11
  - --device=dri

build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  strip: true
  env:
    CARGO_NET_OFFLINE: 'true'
    CARGO_TERM_COLOR: always

modules:
  - name: cliph
    buildsystem: simple

    build-commands:
      - cargo build --release --locked -p ${BINARY_CRATE}

      - >-
        install -Dm755
        target/release/${BINARY_SOURCE_NAME}
        /app/bin/${BINARY_INSTALL_NAME}

      - >-
        install -Dm644
        flatpak/${APP_ID}.desktop
        /app/share/applications/${APP_ID}.desktop

      - >-
        install -Dm644
        flatpak/${APP_ID}.metainfo.xml
        /app/share/metainfo/${APP_ID}.metainfo.xml

      - |
        for size in 16 24 32 48 64 128 256 512; do
          install -Dm644 \
            "assets/icons/hicolor/\${size}x\${size}/apps/${APP_ID}.png" \
            "/app/share/icons/hicolor/\${size}x\${size}/apps/${APP_ID}.png"
        done

    sources:
      - type: dir
        path: source
MANIFEST

printf '◆ Compilation du Flatpak\n'

flatpak-builder \
    --user \
    --force-clean \
    --ccache \
    --install-deps-from=flathub \
    --repo="$REPOSITORY_DIR" \
    --default-branch="$FLATPAK_BRANCH" \
    "$BUILD_DIR" \
    "$MANIFEST"

printf '◆ Création du bundle autonome\n'

rm -f \
    "$BUNDLE_FILE" \
    "$CHECKSUM_FILE"

flatpak build-bundle \
    "$REPOSITORY_DIR" \
    "$BUNDLE_FILE" \
    "$APP_ID" \
    "$FLATPAK_BRANCH" \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo

printf '◆ Création de l’empreinte SHA-256\n'

(
    cd "$OUTPUT_DIRECTORY"

    sha256sum \
        "$(basename "$BUNDLE_FILE")" \
        > "$(basename "$CHECKSUM_FILE")"
)

printf '◆ Vérification de l’empreinte\n'

(
    cd "$OUTPUT_DIRECTORY"

    sha256sum \
        -c \
        "$(basename "$CHECKSUM_FILE")"
)

printf '◆ Installation locale du bundle\n'

if flatpak info \
    --user \
    "$APP_ID" \
    >/dev/null 2>&1
then
    flatpak install \
        --user \
        -y \
        --reinstall \
        "$BUNDLE_FILE"
else
    flatpak install \
        --user \
        -y \
        "$BUNDLE_FILE"
fi

printf '◆ Vérification de la version installée\n'

flatpak run \
    --command=cliph \
    "$APP_ID" \
    --version

printf '\n'
printf '✓ Flatpak créé et installé avec succès\n'
printf '\n'
printf 'Bundle :\n'
printf '  %s\n' "$BUNDLE_FILE"
printf '\n'
printf 'Empreinte :\n'
printf '  %s\n' "$CHECKSUM_FILE"
printf '\n'
printf 'Commande de lancement :\n'
printf '  flatpak run %s\n' "$APP_ID"
printf '\n'
printf 'Commande de désinstallation :\n'
printf '  flatpak uninstall --user %s\n' "$APP_ID"
printf '\n'
