#!/usr/bin/env bash
# Dans le conteneur (build_live.sh) : fabrique le système de la clé SecMet.
#   1. .deb SecMet construit depuis /src
#   2. debootstrap Ubuntu 24.04 (noble) minimal
#   3. dans le chroot : noyau HWE, live-boot, XFCE dépouillé, LightDM en
#      connexion automatique, agent PolicyKit (mot de passe du helper),
#      NetworkManager, lecteur PDF, SecMet, opérateur
#      « secmet » (uid 1000, groupe secmet), locale et clavier français,
#      fichiers de packaging/live/overlay/
#   4. initramfs live-boot, squashfs, noyau, initrd, EFI signé, GRUB BIOS → /out
set -euo pipefail

SRC=/src
OUT=/out
ROOTFS=/rootfs
MIRROR="${SECMET_UBUNTU_MIRROR:-http://archive.ubuntu.com/ubuntu}"
PASSWORD="${SECMET_OPERATOR_PASSWORD:-secmet}"
export DEBIAN_FRONTEND=noninteractive

step() { printf '\n\033[1;33m== %s\033[0m (%s)\n' "$1" "$(date +%T)"; }

# --- 1. Paquet SecMet ---------------------------------------------------------
step "Paquet SecMet (.deb ubuntu24.04)"
rm -rf /build && mkdir /build
tar -C "$SRC" --exclude=./target --exclude='./target-*' --exclude=./dist --exclude=./.git -cf - . \
    | tar -C /build -xf -
cd /build
export CARGO_TARGET_DIR=/target
SECMET_FORCE_BUILD=1 SECMET_DEB_DISTRO=ubuntu24.04 ./packaging/build_deb.sh >/tmp/deb.log 2>&1 \
    || { tail -30 /tmp/deb.log; exit 1; }
VERSION="$(grep -m1 '^version' /build/Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
DEB="/build/dist/secmet_${VERSION}~ubuntu24.04_amd64.deb"
[[ -f "$DEB" ]] || { echo "paquet attendu absent : $DEB"; ls /build/dist; exit 1; }
echo "$DEB"
# Le .deb de l'image est aussi livré dans dist/live/ (même binaire que la clé).
cp "$DEB" /out/

# --- 2. Système de base -------------------------------------------------------
step "debootstrap noble (minbase)"
rm -rf "$ROOTFS"
debootstrap --variant=minbase --components=main,universe --include=gpgv,ubuntu-keyring,ca-certificates noble "$ROOTFS" "$MIRROR" >/tmp/debootstrap.log 2>&1 \
    || { tail -20 /tmp/debootstrap.log; exit 1; }

cat > "$ROOTFS/etc/apt/sources.list" <<SRC_EOF
deb $MIRROR noble main universe
deb $MIRROR noble-updates main universe
deb http://security.ubuntu.com/ubuntu noble-security main universe
SRC_EOF
echo "secmet-live" > "$ROOTFS/etc/hostname"
printf '127.0.0.1 localhost\n127.0.1.1 secmet-live\n' > "$ROOTFS/etc/hosts"

mount -t proc proc "$ROOTFS/proc"
mount -t sysfs sys "$ROOTFS/sys"
# Récursif : dans un conteneur podman sans privilège, /dev/null, /dev/pts…
# sont des sous-montages ; un bind simple les perdrait.
mount --rbind /dev "$ROOTFS/dev"
for d in null zero random urandom tty; do
    if [[ ! -c "$ROOTFS/dev/$d" ]]; then
        [[ -e "$ROOTFS/dev/$d" ]] || touch "$ROOTFS/dev/$d"
        mount --bind "/dev/$d" "$ROOTFS/dev/$d"
    fi
done
mkdir -p "$ROOTFS/dev/pts"
mount -t devpts devpts "$ROOTFS/dev/pts" || true
unmount_all() {
    umount -l "$ROOTFS/dev/pts" 2>/dev/null || true
    for d in null zero random urandom tty; do umount -l "$ROOTFS/dev/$d" 2>/dev/null || true; done
    umount -R -l "$ROOTFS/dev" 2>/dev/null || umount -l "$ROOTFS/dev" 2>/dev/null || true
    umount -l "$ROOTFS/sys" "$ROOTFS/proc" 2>/dev/null || true
}
trap unmount_all EXIT
# Pas de démarrage de services dans le chroot.
printf '#!/bin/sh\nexit 101\n' > "$ROOTFS/usr/sbin/policy-rc.d"; chmod +x "$ROOTFS/usr/sbin/policy-rc.d"
in_chroot() { chroot "$ROOTFS" /usr/bin/env DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 "$@"; }

# --- 3. Paquets ---------------------------------------------------------------
step "Paquets : noyau, live-boot, bureau, outils"
in_chroot apt-get update -qq
in_chroot apt-get install -y -qq --no-install-recommends \
    linux-image-generic-hwe-24.04 linux-firmware \
    live-boot live-boot-initramfs-tools initramfs-tools \
    systemd systemd-sysv systemd-timesyncd dbus dbus-user-session udev kmod \
    locales console-setup keyboard-configuration kbd tzdata \
    sudo policykit-1 udisks2 gvfs \
    network-manager network-manager-gnome wpasupplicant iproute2 iputils-ping ca-certificates \
    xserver-xorg xserver-xorg-video-all xserver-xorg-input-all xinit x11-xserver-utils \
    lightdm lightdm-gtk-greeter \
    xfce4-session xfwm4 xfce4-panel xfdesktop4 xfce4-settings xfce4-power-manager \
    xterm thunar thunar-volman tumbler zathura zathura-pdf-poppler zenity xdg-utils \
    policykit-1-gnome language-pack-fr language-pack-gnome-fr \
    xfce4-whiskermenu-plugin arc-theme elementary-xfce-icon-theme fonts-ubuntu \
    plymouth plymouth-theme-spinner librsvg2-common \
    fonts-dejavu fonts-noto-color-emoji adwaita-icon-theme gnome-themes-extra \
    libgtk-4-1 exfatprogs dosfstools e2fsprogs fdisk parted \
    less nano time >/tmp/apt.log 2>&1 || { tail -40 /tmp/apt.log; exit 1; }

step "SecMet"
cp "$DEB" "$ROOTFS/tmp/secmet.deb"
in_chroot apt-get install -y -qq /tmp/secmet.deb >>/tmp/apt.log 2>&1 || { tail -40 /tmp/apt.log; exit 1; }
rm -f "$ROOTFS/tmp/secmet.deb"
in_chroot dpkg -s secmet | grep -E '^(Version|Depends)'

# --- 4. Configuration -------------------------------------------------------
step "Locale, clavier, opérateur, overlay"
echo "fr_FR.UTF-8 UTF-8" > "$ROOTFS/etc/locale.gen"
in_chroot locale-gen >/dev/null
echo "LANG=fr_FR.UTF-8" > "$ROOTFS/etc/default/locale"
cat > "$ROOTFS/etc/default/keyboard" <<KB_EOF
XKBMODEL="pc105"
XKBLAYOUT="fr"
XKBVARIANT=""
XKBOPTIONS=""
BACKSPACE="guess"
KB_EOF
ln -sf /usr/share/zoneinfo/Europe/Paris "$ROOTFS/etc/localtime"
echo "Europe/Paris" > "$ROOTFS/etc/timezone"

for g in secmet sudo plugdev netdev audio video cdrom dialout; do in_chroot groupadd -f "$g"; done
in_chroot useradd -m -u 1000 -g secmet -s /bin/bash -G sudo,plugdev,netdev,audio,video,cdrom,dialout secmet
echo "secmet:$PASSWORD" | in_chroot chpasswd
# Dossiers de montage de la clé (fstab dans l'overlay).
mkdir -p "$ROOTFS/media/secmet-data" "$ROOTFS/media/secmet-export"

cp -a "$SRC/packaging/live/overlay/." "$ROOTFS/"
# Fond d'écran SecMet pour tout écran : les fonds XFCE par défaut sont remplacés
# (xfdesktop les prend pour un moniteur qu'il ne connaît pas encore).
for bg in "$ROOTFS"/usr/share/backgrounds/xfce/*; do cp "$ROOTFS/usr/share/backgrounds/secmet.png" "$bg"; done
# Écran de démarrage : thème Plymouth « secmet », dérivé du spinner (même
# roue), lion SecMet au centre et mention SecMet en bas (images de
# l'overlay), fond sombre assorti au bureau ; pas de messages.
PT="$ROOTFS/usr/share/plymouth/themes"
if [[ -d "$PT/spinner" ]]; then
    cp -n "$PT"/spinner/*.png "$PT/secmet/" 2>/dev/null || true
    sed -e 's|^ImageDir=.*|ImageDir=/usr/share/plymouth/themes/secmet|' \
        -e 's|^BackgroundStartColor=.*|BackgroundStartColor=0x0e1117|' \
        -e 's|^BackgroundEndColor=.*|BackgroundEndColor=0x1b212e|' \
        -e 's|^WatermarkVerticalAlignment=.*|WatermarkVerticalAlignment=.40|' \
        -e 's|^Name=Spinner|Name=SecMet|' -e '/^Name\[/d' \
        -e 's|^Description=.*|Description=SecMet : roue du spinner, lion SecMet, fond sombre.|' \
        "$PT/spinner/spinner.plymouth" > "$PT/secmet/secmet.plymouth"
    # Thème par défaut : plymouthd.conf (prioritaire) et alternative système,
    # lus par le crochet initramfs de Plymouth. Erreur visible.
    mkdir -p "$ROOTFS/etc/plymouth"
    printf '[Daemon]\nTheme=secmet\n' > "$ROOTFS/etc/plymouth/plymouthd.conf"
    in_chroot update-alternatives --install /usr/share/plymouth/themes/default.plymouth \
        default.plymouth /usr/share/plymouth/themes/secmet/secmet.plymouth 200 >/dev/null
    in_chroot update-alternatives --set default.plymouth \
        /usr/share/plymouth/themes/secmet/secmet.plymouth >/dev/null
    echo "Plymouth : thème $(in_chroot /usr/sbin/plymouth-set-default-theme)"
fi
# Icônes : cache hicolor (icône SecMet du .deb, menu et barre).
in_chroot gtk-update-icon-cache -f -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
# Le panneau par défaut de xfce4-panel est remplacé par celui de l'overlay.
mkdir -p "$ROOTFS/etc/xdg/xfce4/xfconf/xfce-perchannel-xml"
cp "$ROOTFS/etc/xdg/xfce4/panel/default.xml" "$ROOTFS/etc/xdg/xfce4/xfconf/xfce-perchannel-xml/xfce4-panel.xml"
chmod +x "$ROOTFS/usr/local/bin/secmet-live" "$ROOTFS/usr/local/bin/secmet-journal-save" "$ROOTFS/usr/local/bin/secmet-first-boot" "$ROOTFS/usr/local/bin/secmet-app-update" "$ROOTFS/usr/lib/systemd/system-sleep/secmet-display"
# Lanceur sur le bureau pour relancer SecMet si la fenêtre a été fermée.
mkdir -p "$ROOTFS/home/secmet/Bureau"
cp "$ROOTFS/etc/xdg/autostart/secmet.desktop" "$ROOTFS/home/secmet/Bureau/secmet.desktop"
chmod +x "$ROOTFS/home/secmet/Bureau/secmet.desktop"
in_chroot chown -R secmet:secmet /home/secmet
# NetworkManager gère les interfaces (pas de netplan).
mkdir -p "$ROOTFS/etc/NetworkManager/conf.d"
printf '[keyfile]\nunmanaged-devices=none\n' > "$ROOTFS/etc/NetworkManager/conf.d/10-globally-managed-devices.conf"
in_chroot systemctl enable NetworkManager lightdm systemd-timesyncd secmet-journal-sync.timer secmet-journal-final secmet-first-boot secmet-app-update >/dev/null 2>&1 || true
in_chroot systemctl set-default graphical.target >/dev/null 2>&1 || true
# Police de la console texte : 9 s au démarrage (setfont échoue cinq fois
# avec ce noyau) pour un poste graphique ; la disposition clavier des
# consoles reste assurée par keyboard-setup.
in_chroot systemctl mask console-setup.service >/dev/null 2>&1 || true
# Minuteries sans objet sur un système en mémoire (apt, man-db, fstrim,
# e2scrub, motd) : du travail disque et réseau au démarrage pour rien.
in_chroot systemctl mask apt-daily.timer apt-daily-upgrade.timer dpkg-db-backup.timer \
    man-db.timer fstrim.timer e2scrub_all.timer motd-news.timer >/dev/null 2>&1 || true

# --- 5. initramfs, nettoyage, sortie ----------------------------------------
# Micrologiciels sans objet sur un poste x86 (commutateurs et cartes réseau de
# centre de données, SoC ARM) : retirés avant l'initramfs, qui en embarque une
# partie. Environ 240 Mo de moins à lire sur la clé à chaque démarrage.
# Firmware sans objet sur un poste à effacer (copié en mémoire à chaque
# démarrage, non compressible) : cartes réseau de serveurs, commutateurs,
# points d'accès et plateformes routeurs, caméras et capteurs, DSP audio, NPU.
for fw in mellanox mrvl/prestera qcom netronome dpaa2 qed cxgb4 cxgb3 liquidio ti-connectivity \
          bnx2x bnx2 qlogic tehuti acenic tigon slicoss 3com advansys intel/ice \
          mediatek/mt7996 ath11k/IPQ8074 ath11k/IPQ6018 ath11k/IPQ5018 ath11k/QCN9074 ath12k/QCN9274 \
          intel/ipu intel/vsc intel/ish intel/vpu LENOVO HP dell \
          intel/avs intel/catpt cirrus asihpi; do
    rm -rf "$ROOTFS/usr/lib/firmware/$fw"
done
rm -rf "$ROOTFS"/usr/lib/firmware/intel/sof*
# Modules noyau sans objet (webcams, cartes TV, InfiniBand, acquisition de
# mesures, ZFS, en-têtes du noyau) : copiés en mémoire à chaque démarrage.
# Jamais drivers/media en entier : media/cec est requis par
# drm_display_helper, donc par tous les pilotes graphiques (constaté le
# 2026-11-05 : sans lui, simpledrm seul et écran noir au réveil). Ni sound :
# thinkpad_acpi et toshiba_acpi (touches, capot) en dépendent.
KVER="$(ls -1 "$ROOTFS/usr/lib/modules" | sort -V | tail -1)"
for m in kernel/drivers/media/usb kernel/drivers/media/pci kernel/drivers/media/dvb-frontends \
         kernel/drivers/media/tuners kernel/drivers/infiniband kernel/drivers/comedi \
         kernel/drivers/iio kernel/drivers/usb/gadget kernel/drivers/net/can kernel/drivers/net/dsa \
         kernel/fs/ocfs2 kernel/fs/smb/server kernel/kernel/kheaders.ko.zst ubuntu/dkms/zfs; do
    rm -rf "$ROOTFS/usr/lib/modules/$KVER/$m"
done
# Cohérence : un module qui dépendait d'un sous-système retiré (ksmbd et
# RDMA sans InfiniBand, capteurs ADC sans iio…) est retiré à son tour,
# jusqu'à ce que depmod -e ne signale plus aucun symbole non résolu.
for pass in 1 2 3 4 5; do
    in_chroot depmod -e -F "/boot/System.map-$KVER" "$KVER" >/tmp/depmod.log 2>&1 || true
    bad="$(grep -i 'needs unknown symbol' /tmp/depmod.log | awk '{print $3}' | sort -u || true)"
    [[ -n "$bad" ]] || break
    echo "modules dépendants retirés (passe $pass) : $(echo "$bad" | xargs -n1 basename | sed 's/\.ko\.zst$//' | tr '\n' ' ')"
    for f in $bad; do rm -f "$ROOTFS$f"; done
done
[[ -z "$bad" ]] || { echo "modules noyau : dépendances toujours non résolues" >&2; echo "$bad" >&2; exit 1; }
# Les pilotes qui comptent doivent être là (affichage, stockage USB, Wi-Fi).
for must in kernel/drivers/gpu/drm/radeon/radeon.ko.zst kernel/drivers/gpu/drm/amd/amdgpu/amdgpu.ko.zst \
            kernel/drivers/gpu/drm/i915/i915.ko.zst kernel/drivers/gpu/drm/nouveau/nouveau.ko.zst \
            kernel/drivers/gpu/drm/display/drm_display_helper.ko.zst kernel/drivers/media/cec/core/cec.ko.zst \
            kernel/drivers/usb/storage/uas.ko.zst kernel/drivers/net/wireless/intel/iwlwifi/iwlwifi.ko.zst \
            kernel/drivers/net/wireless/realtek/rtw89/rtw89_core.ko.zst kernel/drivers/net/wireless/ath/ath11k/ath11k.ko.zst; do
    [[ -f "$ROOTFS/usr/lib/modules/$KVER/$must" ]] || { echo "module requis absent après élagage : $must" >&2; exit 1; }
done
# live-boot (toram) monte son tmpfs avec « /dev/shm » pour nom de source :
# systemd y voit un périphérique bloc et journalise trente fois « Failed to
# open /dev/shm device » à chaque démarrage. Nom de source neutre.
sed -i 's|dev="/dev/shm"|dev="tmpfs"|' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh"
# Pendant la copie en mémoire (30 à 40 s sur une clé à 25 Mo/s), l'écran de
# démarrage ne montrait rien : message sous le logo, retiré à la fin.
sed -i 's|^\t\t\t\tcp ${MODULETORAMFILE} ${copyto}|\t\t\t\t[ -x /bin/plymouth ] \&\& plymouth --ping \&\& plymouth message --text="Copie du système en mémoire (environ 30 s)…"\n\t\t\t\tcp ${MODULETORAMFILE} ${copyto}\n\t\t\t\t[ -x /bin/plymouth ] \&\& plymouth --ping \&\& plymouth hide-message --text="Copie du système en mémoire (environ 30 s)…"|' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh"
grep -q 'plymouth message' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh" || { echo "patch toram-todisk.sh non appliqué" >&2; exit 1; }
# Taille du module à copier : live-boot lit le 5e champ de « ls -la », mais
# le busybox de l'initramfs n'affiche ni propriétaire ni date (4 champs) :
# « expr: syntax error » puis « sh: bad number » à chaque démarrage toram.
sed -i 's|size=$( expr $(ls -la ${MODULETORAMFILE} \| awk .{print $5}.) / 1024 + 5000 )|size=$(( $(stat -c %s "${MODULETORAMFILE}") / 1024 + 5000 ))|' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh"
grep -q 'stat -c %s' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh" || { echo "patch taille toram non appliqué" >&2; exit 1; }
step "initramfs live-boot"
in_chroot update-initramfs -u -k all >/tmp/initramfs.log 2>&1 || { tail -20 /tmp/initramfs.log; exit 1; }
in_chroot apt-get clean
rm -rf "$ROOTFS/var/lib/apt/lists/"* "$ROOTFS/tmp/"* "$ROOTFS/usr/sbin/policy-rc.d"
# debootstrap copie le resolv.conf de la machine de construction dans le
# chroot (serveurs DNS et domaine de recherche internes) : retiré, c'est
# NetworkManager qui écrit /etc/resolv.conf au démarrage de la clé
# (overlay etc/NetworkManager/conf.d/20-secmet-dns.conf : dns=default).
rm -f "$ROOTFS/etc/resolv.conf"
# Marqueurs « système à jour » (ConditionNeedsUpdate) : sans eux, chaque
# démarrage relance ldconfig (10 s sur le chemin critique), la reconstruction
# du catalogue du journal et systemd-update-done.
touch "$ROOTFS/etc/.updated" "$ROOTFS/var/.updated"
unmount_all
trap - EXIT

step "Sortie : noyau, initrd, squashfs, EFI, BIOS"
mkdir -p "$OUT/efi" "$OUT/bios"
rm -f "$OUT"/vmlinuz "$OUT"/initrd.img "$OUT"/filesystem.squashfs
cp "$(ls -1 "$ROOTFS"/boot/vmlinuz-* | sort -V | tail -1)" "$OUT/vmlinuz"
cp "$(ls -1 "$ROOTFS"/boot/initrd.img-* | sort -V | tail -1)" "$OUT/initrd.img"
# /boot (noyau, initrd, System.map : 220 Mo incompressibles) n'est pas
# embarqué : la clé démarre sur les copies de la partition système.
# -wildcards : sans lui, les motifs -e sont pris au pied de la lettre.
# zstd 19 plutôt que xz : 19 Mo de plus à copier (725 contre 706), mais une
# décompression trois fois plus rapide, et c'est elle qui coûte sur les
# processeurs lents (session XFCE, base dpkg, services) : mesuré le 2026-11-05.
mksquashfs "$ROOTFS" "$OUT/filesystem.squashfs" -comp zstd -Xcompression-level 19 -b 1M -noappend -quiet \
    -wildcards -e 'boot/*' >/tmp/squash.log 2>&1 || { tail -10 /tmp/squash.log; exit 1; }
cp /usr/lib/shim/shimx64.efi.signed "$OUT/efi/shimx64.efi"
# MokManager : selon la version de shim-signed, signé ou non, ou absent.
for mm in /usr/lib/shim/mmx64.efi.signed /usr/lib/shim/mmx64.efi; do
    [[ -f "$mm" ]] && { cp "$mm" "$OUT/efi/mmx64.efi"; break; }
done
cp /usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed "$OUT/efi/grubx64.efi"
cp -a /usr/lib/grub/i386-pc/. "$OUT/bios/"
chmod 0644 "$OUT"/vmlinuz "$OUT"/initrd.img "$OUT"/filesystem.squashfs "$OUT"/efi/*
echo "secmet $(dpkg-deb -f "$DEB" Version) ; noyau $(basename "$(ls -1 "$ROOTFS"/boot/vmlinuz-* | sort -V | tail -1)")" > "$OUT/VERSION"
ls -la "$OUT"
