#!/usr/bin/env bash
set -Eeuo pipefail

PROJECT_ROOT="$(
    cd "$(dirname "${BASH_SOURCE[0]}")/../.." &&
    pwd
)"

IMAGE_NAME="cliph-builder:ubuntu-22.04"

cd "$PROJECT_ROOT"

if ! command -v docker >/dev/null 2>&1; then
    printf 'Docker est introuvable.\n' >&2
    exit 1
fi

printf '\n'
printf '◆ Construction de l’image Ubuntu 22.04\n'

docker build \
    --build-arg USER_ID="$(id -u)" \
    --build-arg GROUP_ID="$(id -g)" \
    --file packaging/containers/ubuntu-22.04.Dockerfile \
    --tag "$IMAGE_NAME" \
    .

printf '\n'
printf '◆ Compilation de ClipH dans Ubuntu 22.04\n'

docker run \
    --rm \
    --volume "$PROJECT_ROOT:/workspace" \
    --workdir /workspace \
    "$IMAGE_NAME" \
    bash -lc '
        set -Eeuo pipefail

        export CARGO_TARGET_DIR=/workspace/target/ubuntu22.04

        printf "Rust        : "
        rustc --version

        printf "GTK         : "
        pkg-config --modversion gtk4

        printf "Libadwaita  : "
        pkg-config --modversion libadwaita-1

        cargo check \
            -p cliph-ui \
            --no-default-features

        cargo clippy \
            -p cliph-ui \
            --all-targets \
            --no-default-features \
            -- \
            -D warnings

        cargo test \
            -p cliph-ui \
            --no-default-features

        CLIPH_BUILD_PROFILE=legacy \
        CLIPH_PACKAGE_VARIANT=ubuntu22.04 \
        bash packaging/debian/build-deb.sh

        PACKAGE="$(
            find dist \
                -maxdepth 1 \
                -type f \
                -name "cliph_*_ubuntu22.04_amd64.deb" \
                -print \
                -quit
        )"

        if [[ -z "$PACKAGE" ]]; then
            printf "Paquet Ubuntu 22.04 introuvable.\n" >&2
            exit 1
        fi

        printf "\n◆ Informations du paquet\n"
        dpkg-deb --info "$PACKAGE"

        printf "\n◆ Dépendances du paquet\n"
        dpkg-deb --field "$PACKAGE" Depends

        printf "\n◆ Analyse Lintian\n"
        lintian --tag-display-limit 0 "$PACKAGE"
    '

printf '\n'
printf 'Paquet Ubuntu 22.04 créé avec succès.\n'
