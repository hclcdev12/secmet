# Image de construction de la clé live SecMet (packaging/live/build_live.sh).
# Ubuntu 24.04 : debootstrap d'un système Ubuntu 24.04 minimal + XFCE,
# construction du .deb SecMet, squashfs, chaîne UEFI signée (Secure Boot)
# et modules GRUB BIOS. À lancer avec podman/docker en --privileged (chroot
# avec /proc, /sys, /dev).
FROM docker.io/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        debootstrap ubuntu-keyring ca-certificates curl squashfs-tools xz-utils \
        mtools dosfstools e2fsprogs exfatprogs fdisk \
        grub-efi-amd64-bin grub-efi-amd64-signed shim-signed grub-pc-bin \
        build-essential pkg-config git libgtk-4-dev libglib2.0-dev \
        policykit-1 testdisk dpkg-dev fakeroot \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN curl -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path \
 && rustc --version

ENV SECMET_IN_CONTAINER=1
WORKDIR /src
