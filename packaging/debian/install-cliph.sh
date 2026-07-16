#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(
    cd "$(dirname "${BASH_SOURCE[0]}")" &&
    pwd
)"

PACKAGE="${1:-}"

if [[ -z "$PACKAGE" ]]; then
    PACKAGE="$(
        find "$SCRIPT_DIR" \
            -maxdepth 1 \
            -type f \
            -name 'cliph_*_*.deb' \
            -print |
        sort -V |
        tail -n 1
    )"
fi

if [[ -z "$PACKAGE" || ! -f "$PACKAGE" ]]; then
    printf 'Paquet ClipH .deb introuvable.\n' >&2
    printf 'Placez install-cliph.sh dans le même dossier que le paquet.\n' >&2
    exit 1
fi

VERSION="$(
    dpkg-deb \
        --field "$PACKAGE" \
        Version
)"

INSTALLED_VERSION="$(
    dpkg-query \
        --show \
        --showformat='${Version}' \
        cliph \
        2>/dev/null ||
    true
)"

TEMP_PACKAGE="$(
    mktemp \
        --tmpdir \
        "cliph-${VERSION}-XXXXXX.deb"
)"

cleanup() {
    rm -f "$TEMP_PACKAGE"
}

trap cleanup EXIT

install \
    -m 0644 \
    "$PACKAGE" \
    "$TEMP_PACKAGE"

APT_ARGUMENTS=(install)

if [[ "$INSTALLED_VERSION" == "$VERSION" ]]; then
    APT_ARGUMENTS+=(--reinstall)
fi

printf 'Installation de ClipH %s...\n' "$VERSION"

sudo apt \
    "${APT_ARGUMENTS[@]}" \
    "$TEMP_PACKAGE"

systemctl --user daemon-reload || true

systemctl --user restart \
    app-com.cliph.ClipH.service \
    2>/dev/null ||
true

printf '\nClipH %s est installé.\n' "$VERSION"
printf 'Raccourci : Super + P\n'
