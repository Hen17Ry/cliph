FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive
ARG USER_ID=1000
ARG GROUP_ID=1000

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        git \
        gzip \
        lintian \
        pkg-config \
        python3 \
        xz-utils \
        dpkg-dev \
        libgtk-4-dev \
        libadwaita-1-dev \
        libglib2.0-dev \
        libxkbcommon-dev \
        libwayland-dev \
        libx11-dev \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd \
        --gid "$GROUP_ID" \
        builder \
    && useradd \
        --uid "$USER_ID" \
        --gid "$GROUP_ID" \
        --create-home \
        --shell /bin/bash \
        builder

USER builder

ENV HOME=/home/builder
ENV CARGO_HOME=/home/builder/.cargo
ENV RUSTUP_HOME=/home/builder/.rustup
ENV PATH=/home/builder/.cargo/bin:$PATH

RUN curl \
        --proto '=https' \
        --tlsv1.2 \
        -sSf \
        https://sh.rustup.rs \
        | sh -s -- \
            -y \
            --profile minimal \
            --default-toolchain 1.97.0

RUN rustup component add \
        clippy \
        rustfmt

WORKDIR /workspace

CMD ["bash"]
