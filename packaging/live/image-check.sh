#!/usr/bin/env bash
# Contrôle de l'image publiable de la clé (dist/secmet-key_<version>_x86_64.img.xz)
# dans QEMU : premier démarrage sur une clé de 16 Go, en BIOS (SeaBIOS) par
# défaut, l'UEFI étant déjà couvert par qemu-check.sh. Sans écran, sans
# privilège.
#
# 1. l'image est décompressée dans un fichier épars agrandi à la taille
#    d'une clé (SECMET_CHECK_KEY_SIZE, 16G) : la table GPT s'arrête bien
#    avant la fin, comme après Imager ou Rufus ;
# 2. démarrage (GRUB i386-pc embarqué sans grub-install, puis
#    secmet-first-boot), attente de la copie du journal, extinction ;
# 3. vérification depuis l'image : journal de secmet-first-boot (clé de
#    signature créée, partition d'export agrandie), attestation-key.hex en
#    0600 pour l'opérateur sur la partition de données, partition d'export
#    qui va jusqu'au bout de la clé ; si l'image porte un magasin
#    app/<version>/ (image reprise d'une release précédente,
#    refresh_live.sh), cette version doit avoir été déployée au démarrage.
#
# Prérequis : dist/secmet-key_*.img.xz (make live-dist), qemu-system-x86_64
# avec KVM, e2fsprogs (e2fsck, debugfs), xz. Durée : 4 minutes.
# usage : make live-dist-check   [SECMET_QEMU_FIRMWARE=uefi pour OVMF]
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE="$ROOT_DIR/packaging/live"
WORK="$ROOT_DIR/dist/live/check-image"
WAIT="${SECMET_CHECK_WAIT:-150}"
KEY_SIZE="${SECMET_CHECK_KEY_SIZE:-16G}"
fail() { echo "ÉCHEC : $*" >&2; exit 1; }
# shellcheck source=packaging/live/qemu-lib.sh
source "$LIVE/qemu-lib.sh"

IMGXZ=$(ls "$ROOT_DIR"/dist/secmet-key_*_x86_64.img.xz 2>/dev/null | sort -V | tail -1 || true)
[[ -n "$IMGXZ" ]] || fail "aucune image dist/secmet-key_*_x86_64.img.xz : make live-dist"
qemu_tools_check
command -v xz >/dev/null || fail "xz introuvable"

rm -rf "$WORK/qemu" "$WORK/key.img" "$WORK/data.img" "$WORK/journal.log"
mkdir -p "$WORK"
echo "== clé de $KEY_SIZE écrite depuis $(basename "$IMGXZ")"
IMG="$WORK/key.img"
xz -T0 -dc "$IMGXZ" | dd of="$IMG" bs=4M iflag=fullblock conv=sparse status=none
truncate -s "$KEY_SIZE" "$IMG"
read -r exp_start exp_before < <(qemu_part "$IMG" SECMET-EXPORT)
disk_sectors=$(( $(stat -c %s "$IMG") / 512 ))
echo "  export : $((exp_before / 2048)) Mio dans l'image, clé de $((disk_sectors / 2048)) Mio"

export SECMET_QEMU_FIRMWARE="${SECMET_QEMU_FIRMWARE:-bios}"
qemu_run_and_collect "$IMG" "$WORK" "$WAIT"
grep -h "secmet-first-boot\[" "$QEMU_JOURNAL" | sed 's/.*secmet-first-boot\[[0-9]*\]: /  /' || true

echo "== vérification"
key_line=$(grep "secmet-first-boot\[" "$QEMU_JOURNAL" | grep "clé de signature des certificats créée" | head -1 || true)
[[ -n "$key_line" ]] || fail "clé de signature non créée au premier démarrage (voir $QEMU_JOURNAL)"
key_id=$(printf '%s\n' "$key_line" | grep -o "key_id : [0-9a-f]*" | cut -d' ' -f3)
[[ ${#key_id} -eq 16 ]] || fail "key_id illisible dans « $key_line »"
# debugfs ls -l : inode, mode (octal), (type), uid, gid, taille, date, nom.
debugfs -R "ls -l /config" "$QEMU_DATA" 2>/dev/null | grep -Eq "100600 \(1\) +1000 +1000 .*attestation-key\.hex$" \
    || fail "config/attestation-key.hex absente ou pas en 0600 pour l'opérateur"
debugfs -R "ls -l /config" "$QEMU_DATA" 2>/dev/null | grep -q "attestation-key\.pub$" || fail "config/attestation-key.pub absente"
grep -q "secmet-first-boot\[.*partition d'export agrandie" "$QEMU_JOURNAL" || fail "partition d'export non agrandie (voir $QEMU_JOURNAL)"
read -r exp_start2 exp_after < <(qemu_part "$IMG" SECMET-EXPORT)
[[ "$exp_start2" == "$exp_start" ]] || fail "la partition d'export a bougé ($exp_start → $exp_start2)"
[[ "$exp_after" -gt "$exp_before" ]] || fail "export : $exp_before → $exp_after secteurs"
tail_gap=$(( disk_sectors - exp_start - exp_after ))
[[ "$tail_gap" -lt 131072 ]] || fail "l'export s'arrête à $((tail_gap / 2048)) Mio du bout de la clé"
# Magasin app/<version>/ dans l'image (reprise d'une release précédente) :
# secmet-app-update doit avoir déployé la version la plus récente.
store=$(debugfs -R "ls -p /app" "$QEMU_DATA" 2>/dev/null | awk -F/ '$6 ~ /^[0-9]+\.[0-9]+\.[0-9]+$/ {print $6}' | sort -V | tail -1)
deployed=""
if [[ -n "$store" ]]; then
    grep -h "secmet-app-update\[" "$QEMU_JOURNAL" | sed 's/.*secmet-app-update\[[0-9]*\]: /  /' || true
    grep -q "secmet-app-update\[.*SecMet $store déployée depuis la partition de données" "$QEMU_JOURNAL" \
        || fail "SecMet $store (magasin app/ de l'image) non déployée au démarrage (voir $QEMU_JOURNAL)"
    deployed=", SecMet $store déployée depuis app/"
fi
echo "image publiable : démarrage ${SECMET_QEMU_FIRMWARE}, clé de signature créée (key_id $key_id), export agrandi de $((exp_before / 2048)) à $((exp_after / 2048)) Mio$deployed"
