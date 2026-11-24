# Image de construction de l'AppImage SecMet.
#
# L'AppImage embarque GTK4 et TestDisk : ces bibliothèques sont liées à la
# glibc de la machine de construction, et une AppImage assemblée sur une
# distribution plus récente que le banc (ex. Fedora, glibc 2.43) refuse de
# démarrer sur Linux Mint 22 / Ubuntu 24.04 (glibc 2.39) avec
# « version `GLIBC_2.43' not found ». La construction se fait donc dans
# Ubuntu 24.04, plus ancienne glibc supportée par le banc.
#
# Utilisé par packaging/build_appimage.sh et build_key_writer_appimage.sh
# (podman ou docker) ; build_key_writer_exe.sh s'en sert aussi quand MinGW
# n'est pas installé sur la machine (gcc-mingw-w64 + cible
# x86_64-pc-windows-gnu).
FROM docker.io/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# ibus-gtk4 : module de saisie embarqué dans l'AppImage (accents, saisie
# asiatique). Sans lui, GTK_IM_MODULE=ibus (Mint, Ubuntu) ne trouve aucun
# module : « No IM module matching GTK_IM_MODULE=ibus found ».
#
# Miroir Ubuntu au choix (CI : celui d'Azure, voisin des runners GitHub ;
# archive.ubuntu.com a mis 40 min à servir les paquets de la 0.5.0-rc3) et
# nouvelles tentatives sur une connexion qui cale.
ARG APT_MIRROR=
RUN if [ -n "$APT_MIRROR" ]; then \
        sed -i "s|http://archive.ubuntu.com/ubuntu|$APT_MIRROR|g" /etc/apt/sources.list.d/ubuntu.sources; \
    fi \
 && printf 'Acquire::Retries "5";\nAcquire::http::Timeout "30";\n' > /etc/apt/apt.conf.d/80secmet-retries \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl file git pkg-config dpkg-dev \
        libgtk-4-dev libglib2.0-dev libglib2.0-bin libgdk-pixbuf2.0-bin \
        desktop-file-utils gtk-update-icon-cache squashfs-tools patchelf \
        librsvg2-common testdisk \
        ibus-gtk4 \
        gcc-mingw-w64-x86-64 \
 && rm -rf /var/lib/apt/lists/* \
 # Backends d'impression GTK : inutiles à SecMet, et ils tirent libcolord /
 # CUPS de l'hôte au lancement (symboles manquants, avertissements).
 && rm -rf /usr/lib/x86_64-linux-gnu/gtk-4.0/4.0.0/printbackends

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin

RUN curl -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path \
 && rustup target add x86_64-pc-windows-gnu \
 && rustc --version && cargo --version

ENV SECMET_IN_CONTAINER=1
WORKDIR /src
