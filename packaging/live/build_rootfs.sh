#!/usr/bin/env bash
# Dans le conteneur (build_live.sh) : fabrique le système de la clé SecMet.
#   1. .deb SecMet construit depuis /src
#   2. debootstrap Debian 13 (trixie) minimal, sections main et
#      non-free-firmware (micrologiciels, liste relue plus bas)
#   3. dans le chroot : noyau Debian signé, live-boot, XFCE dépouillé, LightDM en
#      connexion automatique, agent PolicyKit (mot de passe du helper),
#      NetworkManager, lecteur PDF, SecMet, opérateur
#      « secmet » (uid 1000, groupe secmet), locale et clavier français,
#      fichiers de packaging/live/overlay/
#   4. initramfs live-boot, squashfs, noyau, initrd, GRUB BIOS → /out ;
#      chaîne UEFI signée (shim, MokManager, GRUB) épinglée : versions et
#      empreintes de packaging/live/efi-pin.sha256 (fetch_efi.sh)
set -euo pipefail

SRC=/src
OUT=/out
ROOTFS=/rootfs
MIRROR="${SECMET_DEBIAN_MIRROR:-http://deb.debian.org/debian}"
SECURITY_MIRROR="${SECMET_DEBIAN_SECURITY_MIRROR:-http://security.debian.org/debian-security}"
PASSWORD="${SECMET_OPERATOR_PASSWORD:-secmet}"
export DEBIAN_FRONTEND=noninteractive

step() { printf '\n\033[1;33m== %s\033[0m (%s)\n' "$1" "$(date +%T)"; }

# --- 1. Paquet SecMet ---------------------------------------------------------
# Le .deb de l'image est construit ici, sur Debian 13 (bibliothèques GTK de
# trixie) : secmet_<version>~debian13_amd64.deb. Les mises à jour déployées
# ensuite sur la clé (bouton « Installer la mise à jour », magasin app/ de la
# reprise d'image) sont les .deb Ubuntu 24.04 publiés, choisis par la glibc
# (2.41 sur la clé, 2.39 requise) : ils s'installent sur Debian 13.
step "Paquet SecMet (.deb debian13)"
rm -rf /build && mkdir /build
tar -C "$SRC" --exclude=./target --exclude='./target-*' --exclude=./dist --exclude=./.git -cf - . \
    | tar -C /build -xf -
cd /build
export CARGO_TARGET_DIR=/target
SECMET_FORCE_BUILD=1 SECMET_DEB_DISTRO=debian13 ./packaging/build_deb.sh >/tmp/deb.log 2>&1 \
    || { tail -30 /tmp/deb.log; exit 1; }
VERSION="$(grep -m1 '^version' /build/Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
DEB="/build/dist/secmet_${VERSION}~debian13_amd64.deb"
[[ -f "$DEB" ]] || { echo "paquet attendu absent : $DEB"; ls /build/dist; exit 1; }
echo "$DEB"
# Le .deb de l'image est aussi livré dans dist/live/ (même binaire que la clé).
rm -f /out/secmet_*_amd64.deb
cp "$DEB" /out/
# secmet-release du livrable .tar (secmet-make-key.sh, sur la machine qui
# fabrique la clé : Ubuntu 22.04, Mint 21, Debian 12…) : lié en statique,
# pour ne pas dépendre de la glibc 2.41 du conteneur Debian 13. Ce binaire
# n'utilise ni NSS ni chargement dynamique.
step "secmet-release statique (livrable .tar)"
RUSTFLAGS="-C target-feature=+crt-static" cargo build -q --release -p secmet-attest --bin secmet-release \
    --target x86_64-unknown-linux-gnu >/tmp/static.log 2>&1 || { tail -20 /tmp/static.log; exit 1; }
STATIC_RELEASE=/target/x86_64-unknown-linux-gnu/release/secmet-release
if readelf -d "$STATIC_RELEASE" | grep -q NEEDED; then echo "secmet-release n'est pas statique" >&2; exit 1; fi
"$STATIC_RELEASE" --help | head -1

# --- 2. Système de base -------------------------------------------------------
step "debootstrap trixie (minbase)"
rm -rf "$ROOTFS"
debootstrap --variant=minbase --components=main,non-free-firmware \
    --include=gpgv,debian-archive-keyring,ca-certificates trixie "$ROOTFS" "$MIRROR" >/tmp/debootstrap.log 2>&1 \
    || { tail -20 /tmp/debootstrap.log; exit 1; }

# Sections main et non-free-firmware seulement (ni contrib ni non-free).
rm -f "$ROOTFS/etc/apt/sources.list"
cat > "$ROOTFS/etc/apt/sources.list.d/debian.sources" <<SRC_EOF
Types: deb
URIs: $MIRROR
Suites: trixie trixie-updates
Components: main non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: $SECURITY_MIRROR
Suites: trixie-security
Components: main non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
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
# Micrologiciels (section non-free-firmware), relus paquet par paquet ; tous
# viennent de la source firmware-nonfree (un même fichier copyright, licences
# « binary-redist » des fabricants : redistribution en binaire non modifié,
# aucune clause d'usage non commercial), sauf firmware-linux-free et
# firmware-ath9k-htc et firmware-carl9170 (main) et les microcodes (licences
# Intel et AMD : redistribution en binaire non modifié, avis reproduit).
# Retenus (postes x86 : affichage, Wi-Fi, Bluetooth, réseau, microcode) :
#   firmware-linux-free, firmware-misc-nonfree, firmware-amd-graphics,
#   firmware-intel-graphics, firmware-intel-misc, firmware-nvidia-graphics
#   (nouveau, GSP compris), firmware-iwlwifi, firmware-realtek,
#   firmware-atheros, firmware-mediatek, firmware-brcm80211, firmware-libertas
#   (Wi-Fi Marvell), firmware-ath9k-htc et firmware-carl9170 (clés Wi-Fi USB),
#   intel-microcode, amd64-microcode.
# Exclus :
#   firmware-ivtv, firmware-ipw2x00 : licence à accepter par debconf
#     (*/license/accepted) ; ipw2x00 ne sert qu'aux Intel PRO/Wireless
#     2100 à 2915 (2003-2005) ;
#   firmware-intel-sound, firmware-sof-signed, firmware-cirrus : DSP audio
#     (déjà retirés de l'image Ubuntu : sans objet pour effacer un disque) ;
#   firmware-bnx2, firmware-bnx2x, firmware-cavium, firmware-myricom,
#     firmware-netronome, firmware-netxen, firmware-qlogic,
#     firmware-marvell-prestera, firmware-ast, firmware-ti-connectivity :
#     cartes et commutateurs de serveurs (déjà retirés de l'image Ubuntu) ;
#   firmware-qcom-soc, firmware-qcom-media, firmware-samsung : SoC ARM ;
#   firmware-siano : tuners TV ; firmware-zd1211 : clés Wi-Fi de 2005 ;
#   firmware-nvidia-gsp, firmware-nvidia-tesla-*-gsp : pilote propriétaire
#     NVIDIA, absent de l'image (nouveau a son GSP dans nvidia-graphics).
# La construction échoue si un paquet installé demande d'accepter une
# licence, ou vient de contrib ou de non-free (contrôles après SecMet).
FIRMWARE=(
    firmware-linux-free firmware-misc-nonfree firmware-amd-graphics
    firmware-intel-graphics firmware-intel-misc firmware-nvidia-graphics
    firmware-iwlwifi firmware-realtek firmware-atheros firmware-mediatek
    firmware-brcm80211 firmware-libertas firmware-ath9k-htc firmware-carl9170
    intel-microcode amd64-microcode
)
step "Paquets : noyau, live-boot, bureau, outils"
in_chroot apt-get update -qq
# Équivalents Debian de la liste Ubuntu 24.04 : linux-image-amd64 (signé
# Debian) pour linux-image-generic-hwe-24.04 ; les firmware-* ci-dessus pour
# linux-firmware ; polkitd et pkexec pour policykit-1 ; mate-polkit (fork de
# polkit-gnome, GTK 3) pour policykit-1-gnome, absent de trixie ;
# network-manager-applet et nm-connection-editor pour network-manager-gnome ;
# plymouth-themes pour plymouth-theme-spinner ; xfce4-power-manager-plugins
# (greffon de la barre, inclus dans xfce4-power-manager chez Ubuntu) ;
# wireless-regdb (base réglementaire Wi-Fi, tirée d'office chez Ubuntu) ; fonts-cantarell (police du
# bureau, déjà celle de l'écran de démarrage) pour fonts-ubuntu, dans
# non-free chez Debian. language-pack-fr et language-pack-gnome-fr n'ont pas
# d'équivalent : chez Debian, les traductions sont dans chaque paquet.
in_chroot apt-get install -y -qq --no-install-recommends \
    linux-image-amd64 "${FIRMWARE[@]}" \
    live-boot live-boot-initramfs-tools initramfs-tools \
    systemd systemd-sysv systemd-timesyncd dbus dbus-user-session udev kmod \
    locales console-setup keyboard-configuration kbd tzdata \
    sudo polkitd pkexec udisks2 gvfs \
    network-manager network-manager-applet nm-connection-editor wpasupplicant wireless-regdb iproute2 iputils-ping ca-certificates \
    xserver-xorg xserver-xorg-video-all xserver-xorg-input-all xinit x11-xserver-utils \
    lightdm lightdm-gtk-greeter \
    xfce4-session xfwm4 xfce4-panel xfdesktop4 xfce4-settings xfce4-power-manager xfce4-power-manager-plugins \
    xterm thunar thunar-volman tumbler zathura zathura-pdf-poppler zenity xdg-utils \
    mate-polkit \
    xfce4-whiskermenu-plugin arc-theme elementary-xfce-icon-theme fonts-cantarell \
    plymouth plymouth-themes librsvg2-common \
    fonts-dejavu fonts-noto-color-emoji adwaita-icon-theme gnome-themes-extra \
    libgtk-4-1 exfatprogs dosfstools e2fsprogs fdisk parted \
    less nano time >/tmp/apt.log 2>&1 || { tail -40 /tmp/apt.log; exit 1; }

step "SecMet"
cp "$DEB" "$ROOTFS/tmp/secmet.deb"
in_chroot apt-get install -y -qq /tmp/secmet.deb >>/tmp/apt.log 2>&1 || { tail -40 /tmp/apt.log; exit 1; }
rm -f "$ROOTFS/tmp/secmet.deb"
in_chroot dpkg -s secmet | grep -E '^(Version|Depends)'
# Aucun paquet installé ne doit demander d'accepter une licence (debconf
# */license/accepted, comme firmware-ivtv et firmware-ipw2x00).
lic="$(grep -l '^Template: .*/license/accepted' "$ROOTFS"/var/lib/dpkg/info/*.templates 2>/dev/null || true)"
[[ -z "$lic" ]] || { echo "paquets à licence à accepter installés : $lic" >&2; exit 1; }
# Ni contrib ni non-free : seuls main et non-free-firmware sont permis.
nonfree="$(in_chroot dpkg-query -W -f '${binary:Package} ${Section}\n' | awk '$2 ~ /^(contrib|non-free)\// {print $1}')"
[[ -z "$nonfree" ]] || { echo "paquets contrib ou non-free installés : $nonfree" >&2; exit 1; }

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
# Bits d'exécution posés ici, quel que soit l'état du dépôt d'origine : un
# crochet initramfs ou de veille non exécutable est ignoré en silence
# (constaté le 2026-11-06 : initrd de 184 Mo au lieu de 75 sur GitHub).
chmod 0755 "$ROOTFS"/usr/local/bin/secmet-* "$ROOTFS"/usr/local/sbin/secmet-* "$ROOTFS"/etc/initramfs-tools/hooks/* \
    "$ROOTFS"/usr/lib/systemd/system-sleep/*
# Fond d'écran SecMet pour tout écran : les fonds XFCE par défaut sont remplacés
# (xfdesktop les prend pour un moniteur qu'il ne connaît pas encore).
for bg in "$ROOTFS"/usr/share/backgrounds/xfce/*; do cp "$ROOTFS/usr/share/backgrounds/secmet.png" "$bg"; done
# Écran de démarrage : thème Plymouth « secmet », dérivé du spinner (même
# roue), lion SecMet au centre et mention SecMet en bas (images de
# l'overlay), fond sombre assorti au bureau ; pas de messages.
PT="$ROOTFS/usr/share/plymouth/themes"
[[ -f "$PT/spinner/spinner.plymouth" ]] || { echo "thème Plymouth spinner absent (plymouth-themes)" >&2; exit 1; }
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
# Marque (B6, rapport 8) : le système s'appelle SecMet Live. « Basé sur
# Debian 13 » est une mention factuelle permise par la politique de marque
# de Debian, avec l'avertissement de non-affiliation ; ni nom ni logo Debian
# dans le produit. Restent, parce qu'ils sont techniques et qu'on ne peut
# pas les changer sans perdre la signature ou casser les outils :
# /etc/debian_version, EFI/debian (préfixe du GRUB signé), la section .sbat,
# la version du noyau et les noms de paquets.
DEBIAN_NOTICE="Debian est une marque déposée de Software in the Public Interest, Inc. ; SecMet n'est ni affilié au projet Debian, ni approuvé par lui."
cat > "$ROOTFS/usr/lib/os-release" <<OS_EOF
PRETTY_NAME="SecMet Live $VERSION"
NAME="SecMet Live"
VERSION_ID="$VERSION"
VERSION="$VERSION (basé sur Debian 13 trixie)"
VERSION_CODENAME=trixie
ID=secmet
ID_LIKE=debian
HOME_URL="https://github.com/hclcdev12/secmet"
SUPPORT_URL="https://github.com/hclcdev12/secmet"
BUG_REPORT_URL="https://github.com/hclcdev12/secmet/issues"
OS_EOF
ln -sf ../usr/lib/os-release "$ROOTFS/etc/os-release"
# Console texte (entrée « console de dépannage ») et connexion réseau.
printf 'SecMet Live %s \\n \\l\n\n' "$VERSION" > "$ROOTFS/etc/issue"
printf 'SecMet Live %s\n' "$VERSION" > "$ROOTFS/etc/issue.net"
# La notice de Debian (« The programs included with the Debian GNU/Linux
# system are free software… ABSOLUTELY NO WARRANTY ») est gardée en
# substance, au nom de SecMet Live.
cat > "$ROOTFS/etc/motd" <<MOTD_EOF

SecMet Live $VERSION, basé sur Debian 13 (trixie).

Les programmes de ce système sont des logiciels libres, sauf les
micrologiciels (firmware) redistribués sous la licence de leur fabricant ;
les conditions de chacun sont dans /usr/share/doc/*/copyright, le résumé
dans /usr/share/doc/secmet-live/ et dans LICENCES/ de la partition SECMET-SYS.
SecMet Live est fourni SANS AUCUNE GARANTIE, dans les limites permises par
la loi.

$DEBIAN_NOTICE
MOTD_EOF
# Logo Debian de Plymouth : le crochet initramfs de Plymouth le copie
# toujours (il échoue sans lui) et les thèmes qui affichent un logo de
# distribution le prennent ; remplacé par l'image SecMet du thème.
cp "$PT/secmet/watermark.png" "$ROOTFS/usr/share/plymouth/debian-logo.png"
# desktop-base apporte logos, fonds et thèmes Debian : jamais dans l'image.
if in_chroot dpkg-query -W -f '${Status}' desktop-base 2>/dev/null | grep -q 'ok installed'; then
    echo "desktop-base installé (logos et thèmes Debian) : à exclure" >&2; exit 1
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
in_chroot systemctl enable NetworkManager lightdm systemd-timesyncd secmet-journal-sync.timer secmet-journal-final secmet-first-boot secmet-app-update secmet-image-check secmet-key-watch >/dev/null 2>&1 || true
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
         kernel/fs/ocfs2 kernel/fs/smb/server; do
    rm -rf "$ROOTFS/usr/lib/modules/$KVER/$m"
done
rm -f "$ROOTFS/usr/lib/modules/$KVER"/kernel/kernel/kheaders.ko*
# Cohérence : un module qui dépendait d'un sous-système retiré (ksmbd et
# RDMA sans InfiniBand, capteurs ADC sans iio…) est retiré à son tour,
# jusqu'à ce que depmod -e ne signale plus aucun symbole non résolu.
# Le System.map de Debian est vide (le vrai est dans le paquet -dbg) : les
# symboles exportés par le noyau lui-même viennent du Module.symvers de
# linux-headers-$KVER, téléchargé et lu sans être installé ; seules les
# lignes « vmlinux » sont gardées, celles des modules viennent des modules
# encore présents.
( cd "$ROOTFS/tmp" && in_chroot sh -c "cd /tmp && apt-get download -qq linux-headers-$KVER" ) >/dev/null 2>&1 \
    || { echo "linux-headers-$KVER : téléchargement impossible" >&2; exit 1; }
dpkg-deb --fsys-tarfile "$ROOTFS"/tmp/linux-headers-"$KVER"_*.deb | tar -xO --wildcards '*/Module.symvers' \
    | awk -F'\t' '$3 == "vmlinux"' > "$ROOTFS/tmp/vmlinux.symvers"
rm -f "$ROOTFS"/tmp/linux-headers-*.deb
[[ "$(wc -l < "$ROOTFS/tmp/vmlinux.symvers")" -gt 5000 ]] || { echo "Module.symvers du noyau illisible" >&2; exit 1; }
for pass in 1 2 3 4 5; do
    in_chroot depmod -e -E /tmp/vmlinux.symvers "$KVER" >/tmp/depmod.log 2>&1 || true
    bad="$(grep -i 'needs unknown symbol' /tmp/depmod.log | awk '{print $3}' | sort -u || true)"
    [[ -n "$bad" ]] || break
    echo "modules dépendants retirés (passe $pass) : $(echo "$bad" | xargs -n1 basename | sed -E 's/\.ko(\.(zst|xz|gz))?$//' | tr '\n' ' ')"
    for f in $bad; do rm -f "$ROOTFS$f"; done
done
[[ -z "$bad" ]] || { echo "modules noyau : dépendances toujours non résolues" >&2; echo "$bad" >&2; exit 1; }
# Les pilotes qui comptent doivent être là (affichage, stockage USB, Wi-Fi).
# Extension selon la compression des modules du noyau (.ko.xz chez Debian).
for must in kernel/drivers/gpu/drm/radeon/radeon kernel/drivers/gpu/drm/amd/amdgpu/amdgpu \
            kernel/drivers/gpu/drm/i915/i915 kernel/drivers/gpu/drm/nouveau/nouveau \
            kernel/drivers/gpu/drm/display/drm_display_helper kernel/drivers/media/cec/core/cec \
            kernel/drivers/usb/storage/uas kernel/drivers/net/wireless/intel/iwlwifi/iwlwifi \
            kernel/drivers/net/wireless/realtek/rtw89/rtw89_core kernel/drivers/net/wireless/ath/ath11k/ath11k; do
    compgen -G "$ROOTFS/usr/lib/modules/$KVER/$must.ko*" >/dev/null || { echo "module requis absent après élagage : $must" >&2; exit 1; }
done
# live-boot (toram) : le tmpfs a pour nom de source « tmpfs » chez Debian (le
# « /dev/shm » d'Ubuntu faisait journaliser à systemd trente fois « Failed
# to open /dev/shm device » à chaque démarrage).
grep -q 'dev="tmpfs"' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh" || { echo "live-boot : tmpfs toram inattendu" >&2; exit 1; }
# Pendant la copie en mémoire (30 à 40 s sur une clé à 25 Mo/s), l'écran de
# démarrage ne montrait rien : message sous le logo, retiré à la fin.
sed -i 's|^\t\t\t\tcp ${MODULETORAMFILE} ${copyto}.*$|\t\t\t\t[ -x /bin/plymouth ] \&\& plymouth --ping \&\& plymouth message --text="Copie du système en mémoire (environ 30 s)…"\n\t\t\t\tcp ${MODULETORAMFILE} ${copyto}\n\t\t\t\t[ -x /bin/plymouth ] \&\& plymouth --ping \&\& plymouth hide-message --text="Copie du système en mémoire (environ 30 s)…"|' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh"
grep -q 'plymouth message' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh" || { echo "patch toram-todisk.sh non appliqué" >&2; exit 1; }
# Taille du module à copier : live-boot lit le 5e champ de « ls -la », mais
# le busybox de l'initramfs n'affiche ni propriétaire ni date (4 champs) :
# « expr: syntax error » puis « sh: bad number » à chaque démarrage toram.
sed -i 's|size=$( expr $(ls -la ${MODULETORAMFILE} \| awk .{print $5}.) / 1024 + 5000 )|size=$(( $(stat -c %s "${MODULETORAMFILE}") / 1024 + 5000 ))|' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh"
grep -q 'stat -c %s' "$ROOTFS/usr/lib/live/boot/9990-toram-todisk.sh" || { echo "patch taille toram non appliqué" >&2; exit 1; }
step "initramfs live-boot"
# Pilote nouveau et firmware NVIDIA (64 Mo, incompressibles) hors de
# l'initrd : chez Debian, initramfs-tools copie modules et firmware après
# les crochets (dracut-install), si bien que zz-secmet-trim ne les trouve
# pas encore. Ils sont mis de côté le temps de update-initramfs ; le système
# complet (squashfs) les garde et les charge au démarrage réel.
HOLD=/tmp/initrd-hold; rm -rf "$HOLD"; mkdir -p "$HOLD"
mv "$ROOTFS/usr/lib/firmware/nvidia" "$HOLD/firmware-nvidia"
mv "$ROOTFS/usr/lib/modules/$KVER/kernel/drivers/gpu/drm/nouveau" "$HOLD/nouveau"
in_chroot update-initramfs -u -k all >/tmp/initramfs.log 2>&1 || { tail -20 /tmp/initramfs.log; exit 1; }
mv "$HOLD/firmware-nvidia" "$ROOTFS/usr/lib/firmware/nvidia"
mv "$HOLD/nouveau" "$ROOTFS/usr/lib/modules/$KVER/kernel/drivers/gpu/drm/nouveau"
# Manifeste des paquets (B3) : chaque paquet installé du système, avec son
# paquet source et les sources intégrées à sa construction (Built-Using,
# Static-Built-Using des paquets Rust et Go). Complété plus bas par la
# chaîne de démarrage, qui n'est pas dans le squashfs.
in_chroot dpkg-query -W -f '${db:Status-Abbrev}|${binary:Package}\t${Version}\t${Architecture}\t${source:Package}\t${source:Version}\t${Built-Using}\t${Static-Built-Using}\n' \
    | sed -n 's/^ii *|//p' | LC_ALL=C sort > /tmp/rootfs.manifest
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
# Marque : contrôle bloquant de os-release, puis rapport des mentions de
# Debian qui restent dans /etc et les thèmes de démarrage (relu à chaque
# changement de la liste des paquets ; les fichiers de paquets qui citent
# Debian dans un commentaire ou une URL sont attendus).
grep -qx 'NAME="SecMet Live"' "$ROOTFS/etc/os-release" || { echo "os-release : NAME attendu SecMet Live" >&2; exit 1; }
echo "Mentions de Debian restantes (/etc, /usr/share/plymouth) :"
(cd "$ROOTFS" && grep -rIil debian etc usr/share/plymouth 2>/dev/null | sort | sed 's/^/  /') || true
unmount_all
trap - EXIT

step "Sortie : noyau, initrd, squashfs, EFI, BIOS"
mkdir -p "$OUT/efi" "$OUT/bios"
rm -f "$OUT"/vmlinuz "$OUT"/initrd.img "$OUT"/filesystem.squashfs "$OUT"/SYSTEM.sums "$OUT"/SYSTEM.sums.sig
cp "$(ls -1 "$ROOTFS"/boot/vmlinuz-* | sort -V | tail -1)" "$OUT/vmlinuz"
cp "$(ls -1 "$ROOTFS"/boot/initrd.img-* | sort -V | tail -1)" "$OUT/initrd.img"
# L'initrd est lu par GRUB depuis la clé avant tout affichage : au-delà de
# 120 Mo, le crochet zz-secmet-trim n'a pas tourné (190 Mo avec nouveau et
# le firmware NVIDIA, 75 à 85 Mo sans).
initrd_mb=$(( $(stat -c %s "$OUT/initrd.img") / 1048576 ))
echo "initrd : ${initrd_mb} Mo"
[[ "$initrd_mb" -le 120 ]] || { echo "initrd de ${initrd_mb} Mo : crochet zz-secmet-trim non appliqué ?" >&2; exit 1; }
# Chaîne UEFI : jamais « la version du jour » ; fetch_efi.sh échoue si une
# version, une empreinte ou un émetteur diffère de efi-pin.sha256.
step "Chaîne UEFI épinglée (shim, MokManager, GRUB)"
EFI_PKG=/tmp/efi-pkg
"$SRC/packaging/live/fetch_efi.sh" "$EFI_PKG"
rm -f "$OUT"/efi/*
cp "$EFI_PKG/shim-signed/usr/lib/shim/shimx64.efi.signed" "$OUT/efi/shimx64.efi"
# MokManager : paquet shim-helpers-amd64-signed chez Debian. Sur la FAT, il
# permet à l'utilisateur d'enrôler sa propre clé (MOK).
cp "$EFI_PKG/shim-helpers-amd64-signed/usr/lib/shim/mmx64.efi.signed" "$OUT/efi/mmx64.efi"
cp "$EFI_PKG/grub-efi-amd64-signed/usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed" "$OUT/efi/grubx64.efi"
cp "$EFI_PKG/versions.tsv" "$OUT/efi/versions.tsv"
[[ -f "$EFI_PKG/efi-pin.sha256.new" ]] && cp "$EFI_PKG/efi-pin.sha256.new" "$OUT/efi/"
# Manifeste complet : système (squashfs), puis partition SECMET-SYS (chaîne
# UEFI épinglée, GRUB BIOS du conteneur). Publié avec la release
# (secmet-key_<version>_x86_64.packages.tar, bundle_live.sh).
step "Manifeste des paquets"
{
    echo "# SecMet Live $VERSION : paquets de l'image de la clé USB, système basé sur Debian 13 (trixie)."
    echo "# Construit le $(date -u +%F) par packaging/live/build_rootfs.sh (sources : https://github.com/hclcdev12/secmet)."
    echo "# Colonnes (tabulations) : paquet, version, architecture, paquet source, version source, Built-Using, Static-Built-Using."
    echo "# Debian est une marque déposée de Software in the Public Interest, Inc. ; SecMet n'est ni affilié au projet Debian, ni approuvé par lui."
    echo "#"
    echo "# Système (live/filesystem.squashfs)"
    cat /tmp/rootfs.manifest
    echo "#"
    echo "# Partition de démarrage SECMET-SYS (EFI/BOOT, boot/grub), hors du système"
    cut -f1-6 "$EFI_PKG/versions.tsv" | sed 's/$/\t/'
    dpkg-query -W -f '${binary:Package}\t${Version}\t${Architecture}\t${source:Package}\t${source:Version}\t${Built-Using}\t\n' grub-pc-bin
} > "$OUT/packages.manifest"
n_manifest=$(grep -vc '^#' "$OUT/packages.manifest")
for must in linux-image-amd64 testdisk secmet live-boot shim-signed shim-helpers-amd64-signed grub-efi-amd64-signed grub-pc-bin; do
    grep -q "^$must"$'\t' "$OUT/packages.manifest" || { echo "manifeste : $must absent" >&2; exit 1; }
done
[[ "$n_manifest" -ge 500 ]] || { echo "manifeste : $n_manifest paquets seulement" >&2; exit 1; }
echo "packages.manifest : $n_manifest paquets"
# Licences et offre de sources (B4) : LICENCES/ de la partition de
# démarrage et du livrable, résumé dans /usr/share/doc/secmet-live/ du
# système (avant le squashfs).
step "Licences"
env SRC="$SRC" "$SRC/packaging/live/licences.sh" "$ROOTFS" "$EFI_PKG" "$OUT/packages.manifest" "$OUT" "$VERSION"
step "squashfs"
# /boot (noyau, initrd, System.map : 220 Mo incompressibles) n'est pas
# embarqué : la clé démarre sur les copies de la partition système.
# -wildcards : sans lui, les motifs -e sont pris au pied de la lettre.
# zstd 19 plutôt que xz : 19 Mo de plus à copier (725 contre 706), mais une
# décompression trois fois plus rapide, et c'est elle qui coûte sur les
# processeurs lents (session XFCE, base dpkg, services) : mesuré le 2026-11-05.
mksquashfs "$ROOTFS" "$OUT/filesystem.squashfs" -comp zstd -Xcompression-level 19 -b 1M -noappend -quiet \
    -wildcards -e 'boot/*' >/tmp/squash.log 2>&1 || { tail -10 /tmp/squash.log; exit 1; }
cp -a /usr/lib/grub/i386-pc/. "$OUT/bios/"
chmod 0644 "$OUT"/vmlinuz "$OUT"/initrd.img "$OUT"/filesystem.squashfs "$OUT"/efi/*
echo "secmet $(dpkg-deb -f "$DEB" Version) ; noyau $(basename "$(ls -1 "$ROOTFS"/boot/vmlinuz-* | sort -V | tail -1)")" > "$OUT/VERSION"
ls -la "$OUT"
