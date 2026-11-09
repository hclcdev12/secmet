#!/usr/bin/env bash
# Fabrique une clé USB SecMet (Annexe M) à partir de l'image produite par
# build_live.sh : trois partitions GPT (système, données, export), GRUB
# UEFI (chaîne signée Ubuntu, Secure Boot) et BIOS, données initialisées
# avec le schéma de propriétés attendu par SecMet et son helper, clé de
# signature des certificats créée tout de suite (son key_id est affiché :
# à enregistrer chez le client).
#
# usage : sudo packaging/live/secmet-make-key.sh /dev/sdX [--data-size 4G]
#                [--system-only] [--yes]
#        sudo packaging/live/secmet-make-key.sh image.img [--size 16G] …
#   --system-only : réécrit seulement la partition système (mise à jour de
#                   l'image) ; données, lots, clé de signature conservés.
#   --yes         : pas de confirmation interactive.
#   image.img     : fichier image (créé à --size, défaut 16G) attaché à un
#                   périphérique boucle : clé virtuelle pour QEMU.
#
# Source : dist/live/ (vmlinuz, initrd.img, filesystem.squashfs, efi/, bios/).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE_DIR="${SECMET_LIVE_DIR:-$ROOT_DIR/dist/live}"
DEV=""
DATA_SIZE="4G"
IMAGE_SIZE="16G"
SYSTEM_ONLY=0
ASSUME_YES=0
LOOP=""
IMAGE_FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --data-size) DATA_SIZE="$2"; shift ;;
        --system-only) SYSTEM_ONLY=1 ;;
        --yes) ASSUME_YES=1 ;;
        --size) IMAGE_SIZE="$2"; shift ;;
        -h|--help) sed -n '2,19p' "${BASH_SOURCE[0]}"; exit 0 ;;
        -*) echo "argument inconnu : $1" >&2; exit 2 ;;
        *) DEV="$1" ;;
    esac
    shift
done

die() { echo "ERREUR : $*" >&2; exit 1; }
[[ $EUID -eq 0 ]] || die "à lancer en root (sudo)"
[[ -n "$DEV" ]] || die "périphérique bloc (ex. /dev/sdc) ou fichier image attendu"
if [[ ! -b "$DEV" ]]; then
    # Fichier image : clé virtuelle (QEMU, essais), via un périphérique boucle.
    [[ "$SYSTEM_ONLY" -eq 1 && ! -f "$DEV" ]] && die "$DEV absent"
    [[ -f "$DEV" ]] || truncate -s "$IMAGE_SIZE" "$DEV"
    LOOP="$(losetup -fP --show "$DEV")" || die "losetup impossible (conteneur privilégié requis)"
    IMAGE_FILE="$DEV"; DEV="$LOOP"
    ASSUME_YES=1
fi
for f in vmlinuz initrd.img filesystem.squashfs; do
    [[ -f "$LIVE_DIR/$f" ]] || die "$LIVE_DIR/$f absent : lancez make live"
done
for t in sfdisk mkfs.vfat mkfs.ext4 mkfs.exfat partprobe xxd sha256sum; do
    command -v "$t" >/dev/null 2>&1 || die "outil requis : $t"
done
if findmnt -no SOURCE / | grep -q "^$DEV"; then die "$DEV porte le système en cours"; fi
part() { # <n> → chemin de la partition n
    if [[ "$DEV" =~ [0-9]$ ]]; then echo "${DEV}p$1"; else echo "${DEV}$1"; fi
}

# Réécriture complète : tout démonter. Système seul : seule la partition 1
# (les données et l'export, peut-être en usage, restent montés).
if [[ "$SYSTEM_ONLY" -eq 1 ]]; then
    to_unmount="$(lsblk -no MOUNTPOINT "$(part 1)" 2>/dev/null | grep . || true)"
else
    to_unmount="$(lsblk -no MOUNTPOINT "$DEV" | grep . || true)"
fi
if [[ -n "$to_unmount" ]]; then
    echo "Démontage : $to_unmount"
    for mp in $to_unmount; do umount "$mp"; done
fi

if [[ "$SYSTEM_ONLY" -eq 0 ]]; then
    echo "Partitionnement de $DEV (toutes les données seront perdues)…"
    if [[ "$ASSUME_YES" -eq 0 ]]; then
        read -r -p "Confirmer la réécriture complète de $DEV ? [oui/N] " answer
        [[ "$answer" == "oui" ]] || { echo "annulé"; exit 1; }
    fi
    wipefs -aq "$DEV"
    # 1 : système (EFI System, GRUB + noyau + squashfs) ; 2 : données ext4 ;
    # 3 : export exFAT (reste) ; 4 : partition d'amorçage BIOS (1 Mio, avant
    # la première partition, qui commence à 2 Mio) où GRUB i386-pc loge son code pour les postes
    # sans UEFI.
    sfdisk -q "$DEV" <<PART_EOF
label: gpt
start=2MiB, size=6GiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="SECMET-SYS"
size=$DATA_SIZE, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="SECMET-DATA"
type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="SECMET-EXPORT"
start=1MiB, size=1MiB, type=21686148-6449-6E6F-744E-656564454649, name="BIOS boot"
PART_EOF
    partprobe "$DEV" 2>/dev/null || true; sleep 2
    if [[ -n "$LOOP" ]]; then
        losetup -d "$LOOP"; LOOP="$(losetup -fP --show "$IMAGE_FILE")"; DEV="$LOOP"
    fi
    mkfs.vfat -F32 -n SECMET-SYS "$(part 1)" >/dev/null
    # Pas d'initialisation différée : elle écrirait les tables d'inodes en
    # arrière-plan pendant les premiers démarrages (clé lente, veille et
    # extinction ralenties).
    mkfs.ext4 -q -L SECMET-DATA -E lazy_itable_init=0,lazy_journal_init=0 "$(part 2)"
    # exFAT : étiquette limitée à 11 caractères.
    mkfs.exfat -L SECMET-EXP "$(part 3)" >/dev/null
fi

SYS="$(part 1)"; DATA="$(part 2)"
MNT="$(mktemp -d)"
cleanup() {
    umount "$MNT/data" 2>/dev/null || true; umount "$MNT/sys" 2>/dev/null || true
    rmdir "$MNT/data" "$MNT/sys" "$MNT" 2>/dev/null || true
    [[ -n "$LOOP" ]] && losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT
mkdir -p "$MNT/sys" "$MNT/data"

# --- Partition système ---------------------------------------------------------
echo "Système : noyau, initrd, squashfs, GRUB…"
mount "$SYS" "$MNT/sys"
rm -rf "$MNT/sys/live" "$MNT/sys/EFI" "$MNT/sys/boot"
mkdir -p "$MNT/sys/live" "$MNT/sys/EFI/BOOT" "$MNT/sys/boot/grub"
cp "$LIVE_DIR/vmlinuz" "$LIVE_DIR/initrd.img" "$LIVE_DIR/filesystem.squashfs" "$MNT/sys/live/"
cp "$ROOT_DIR/packaging/live/grub.cfg" "$MNT/sys/boot/grub/grub.cfg"
# UEFI : chaîne signée Ubuntu (shim + grub signé) copiée telle quelle, lisible
# avec Secure Boot ; grub signé cherche /EFI/BOOT/grub.cfg puis /boot/grub.
if [[ -d "$LIVE_DIR/efi" ]]; then
    cp "$LIVE_DIR/efi/shimx64.efi" "$MNT/sys/EFI/BOOT/BOOTX64.EFI"
    cp "$LIVE_DIR/efi/grubx64.efi" "$MNT/sys/EFI/BOOT/grubx64.efi"
    cp "$ROOT_DIR/packaging/live/grub.cfg" "$MNT/sys/EFI/BOOT/grub.cfg"
    # Le grub signé d'Ubuntu lit $prefix/grub.cfg avec prefix=/EFI/ubuntu.
    mkdir -p "$MNT/sys/EFI/ubuntu"
    cp "$ROOT_DIR/packaging/live/grub.cfg" "$MNT/sys/EFI/ubuntu/grub.cfg"
    [[ -f "$LIVE_DIR/efi/mmx64.efi" ]] && cp "$LIVE_DIR/efi/mmx64.efi" "$MNT/sys/EFI/BOOT/"
fi
# BIOS : GRUB i386-pc dans le secteur d'amorçage et la partition d'amorçage
# BIOS (postes anciens sans UEFI). Une clé plus ancienne sans cette
# partition reste UEFI seule.
if [[ -d "$LIVE_DIR/bios" ]] && command -v grub-install >/dev/null 2>&1; then
    if ! grub-install --target=i386-pc --boot-directory="$MNT/sys/boot" \
        --directory="$LIVE_DIR/bios" "$DEV" >/tmp/secmet-grub-bios.log 2>&1; then
        echo "avertissement : GRUB BIOS non installé (UEFI seul) : $(tail -1 /tmp/secmet-grub-bios.log)" >&2
    fi
fi
sync
umount "$MNT/sys"

# --- Partition de données ------------------------------------------------------
echo "Données : dossiers et clé de signature…"
mount "$DATA" "$MNT/data"
# Schéma de propriétés (Annexe M) : racine root 0755, dossiers de l'opérateur
# (uid 1000 dans l'image, dont app/ pour les mises à jour conservées),
# vault réservé au helper (root 0700).
chown root:root "$MNT/data"; chmod 755 "$MNT/data"
for d in config logs lots archive app; do
    mkdir -p "$MNT/data/$d"; chown 1000:1000 "$MNT/data/$d"; chmod 700 "$MNT/data/$d"
done
mkdir -p "$MNT/data/vault"; chown root:root "$MNT/data/vault"; chmod 700 "$MNT/data/vault"
# Magasin livré avec l'image (livrable d'une version dont le système est
# repris d'une release précédente, bundle_live.sh) : versions signées que
# secmet-app-update déploie au démarrage.
if [[ -d "$LIVE_DIR/app" ]]; then
    cp -r "$LIVE_DIR/app/." "$MNT/data/app/"
    chown -R 1000:1000 "$MNT/data/app"
    echo "  magasin app/ : $(ls "$LIVE_DIR/app" | tr '\n' ' ')"
fi
KEY="$MNT/data/config/attestation-key.hex"
if [[ ! -f "$KEY" ]]; then
    RELEASE_BIN="$ROOT_DIR/target/release/secmet-release"
    [[ -x "$RELEASE_BIN" ]] || die "$RELEASE_BIN absent : cargo build --release -p secmet-attest"
    # Même format que la clé créée par l'application au premier certificat.
    "$RELEASE_BIN" keygen "$KEY" | sed 's/^/  /'
    chown 1000:1000 "$KEY" "${KEY%.hex}.pub"
fi
PUBKEY="$(tr -d '[:space:]' < "${KEY%.hex}.pub")"
KEY_ID="$(printf '%s' "$PUBKEY" | xxd -r -p | sha256sum | cut -c1-16)"
if [[ ! -f "$MNT/data/config/settings.json" ]]; then
    printf '{\n  "language": "fr",\n  "check_updates": true\n}\n' > "$MNT/data/config/settings.json"
    chown 1000:1000 "$MNT/data/config/settings.json"
fi
sync
umount "$MNT/data"

echo
echo "Clé SecMet prête sur ${IMAGE_FILE:-$DEV}."
echo "  Clé publique de signature des certificats : $PUBKEY"
echo "  key_id : $KEY_ID — à enregistrer chez le client (Annexe M §M.6)."
echo "  Mot de passe opérateur : celui de l'image (build_live.sh, SECMET_OPERATOR_PASSWORD)."
