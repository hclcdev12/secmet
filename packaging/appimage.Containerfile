# Image de construction de l'AppImage SecMet.
#
# L'AppImage embarque GTK4 et TestDisk : ces bibliothèques sont liées à la
# glibc de la machine de construction, et une AppImage assemblée sur une
# distribution plus récente que le banc (ex. Fedora, glibc 2.43) refuse de
# démarrer sur Linux Mint 22 / Ubuntu 24.04 (glibc 2.39) avec
# « version `GLIBC_2.43' not found ». La construction se fait donc dans
# Ubuntu 24.04, plus ancienne glibc supportée par le banc.
#
# Utilisé par packaging/build_appimage.sh (podman ou docker).
FROM docker.io/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl file git pkg-config dpkg-dev \
        libgtk-4-dev libglib2.0-dev libglib2.0-bin libgdk-pixbuf2.0-bin \
        desktop-file-utils gtk-update-icon-cache squashfs-tools patchelf \
        librsvg2-common testdisk \
 && rm -rf /var/lib/apt/lists/* \
 # Backends d'impression GTK : inutiles à SecMet, et ils tirent libcolord /
 # CUPS de l'hôte au lancement (symboles manquants, avertissements).
 && rm -rf /usr/lib/x86_64-linux-gnu/gtk-4.0/4.0.0/printbackends

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin

RUN curl -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path \
 && rustc --version && cargo --version

ENV SECMET_IN_CONTAINER=1
WORKDIR /src
