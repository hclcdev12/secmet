# Image de construction de la clé live SecMet (packaging/live/build_live.sh).
# Debian 13 (trixie) : debootstrap d'un système Debian 13 minimal + XFCE,
# construction du .deb SecMet, squashfs, chaîne UEFI signée (Secure Boot)
# et modules GRUB BIOS. À lancer avec podman/docker en --privileged (chroot
# avec /proc, /sys, /dev).
FROM docker.io/library/debian:13

ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8

# pkexec et polkitd remplacent policykit-1 (absent de trixie) : le .deb
# construit ici installe la règle PolicyKit du helper. MokManager est dans
# shim-helpers-amd64-signed chez Debian.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        debootstrap debian-archive-keyring ca-certificates curl squashfs-tools xz-utils \
        mtools dosfstools e2fsprogs exfatprogs fdisk \
        grub-efi-amd64-bin grub-efi-amd64-signed shim-signed shim-helpers-amd64-signed grub-pc-bin binutils \
        build-essential pkg-config git libgtk-4-dev libglib2.0-dev \
        pkexec polkitd testdisk dpkg-dev fakeroot \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN curl -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path \
 && rustc --version

ENV SECMET_IN_CONTAINER=1
WORKDIR /src
