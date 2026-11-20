# Image de construction de la clé live SecMet (packaging/live/build_live.sh).
# Debian 13 (trixie) : debootstrap d'un système Debian 13 minimal + XFCE,
# construction du .deb SecMet, squashfs et modules GRUB BIOS. La chaîne UEFI
# signée (shim, MokManager, GRUB) n'est pas prise ici mais récupérée par
# version exacte et contrôlée contre packaging/live/efi-pin.sha256
# (fetch_efi.sh, appelé par build_rootfs.sh). À lancer avec podman/docker en
# --privileged (chroot avec /proc, /sys, /dev).
FROM docker.io/library/debian:13

ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8

# pkexec et polkitd remplacent policykit-1 (absent de trixie) : le .deb
# construit ici installe la règle PolicyKit du helper. sbsigntool : émetteurs
# des signatures de shim et de GRUB affichés et contrôlés à la construction.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        debootstrap debian-archive-keyring ca-certificates curl squashfs-tools xz-utils \
        mtools dosfstools e2fsprogs exfatprogs fdisk \
        grub-efi-amd64-bin grub-pc-bin sbsigntool binutils \
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
