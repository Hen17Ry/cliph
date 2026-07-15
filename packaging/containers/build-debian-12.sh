#!/usr/bin/env bash
set -Eeuo pipefail

PROJECT_ROOT="$(
    cd "$(dirname "${BASH_SOURCE[0]}")/../.." &&
    pwd
)"

IMAGE_NAME="cliph-builder:debian-12"

cd "$PROJECT_ROOT"

if ! command -v docker >/dev/null 2>&1; then
    printf 'Docker est introuvable.\n' >&2
    exit 1
fi

printf '\n'
printf '◆ Construction de l’image Debian 12\n'

docker build \
    --build-arg USER_ID="$(id -u)" \
    --build-arg GROUP_ID="$(id -g)" \
    --file packaging/containers/debian-12.Dockerfile \
    --tag "$IMAGE_NAME" \
    .

printf '\n'
printf '◆ Compilation de ClipH dans Debian 12\n'

docker run \
    --rm \
    --volume "$PROJECT_ROOT:/workspace" \
    --workdir /workspace \
    "$IMAGE_NAME" \
    bash -lc '
        set -Eeuo pipefail

        export CARGO_TARGET_DIR=/workspace/target/debian12

        printf "Rust        : "
        rustc --version

        printf "GTK         : "
        pkg-config --modversion gtk4

        printf "Libadwaita  : "
        pkg-config --modversion libadwaita-1

        printf "\n◆ Formatage\n"

        cargo fmt \
            --all \
            -- \
            --check

        printf "\n◆ Vérification du workspace\n"

        cargo check \
            --workspace \
            --no-default-features \
            --features cliph-ui/gtk-v4-8

        printf "\n◆ Analyse Clippy\n"

        cargo clippy \
            --workspace \
            --all-targets \
            --no-default-features \
            --features cliph-ui/gtk-v4-8 \
            -- \
            -D warnings

        printf "\n◆ Tests\n"

        cargo test \
            --workspace \
            --no-default-features \
            --features cliph-ui/gtk-v4-8

        printf "\n◆ Construction du paquet Debian 12\n"

        CLIPH_BUILD_PROFILE=modern \
        CLIPH_PACKAGE_VARIANT=debian12 \
        bash packaging/debian/build-deb.sh

        PACKAGE="$(
            find dist \
                -maxdepth 1 \
                -type f \
                -name "cliph_*_debian12_amd64.deb" \
                -print \
                -quit
        )"

        if [[ -z "$PACKAGE" ]]; then
            printf "Paquet Debian 12 introuvable.\n" >&2
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
printf 'Paquet Debian 12 créé avec succès.\n'
