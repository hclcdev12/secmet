#!/usr/bin/env bash
# Fabrique une image de clé SecMet (fichier GPT) sans montage ni privilège,
# dans le conteneur de construction : chaque partition est produite comme
# un fichier (FAT32 remplie par mtools, ext4 peuplée par mke2fs -d avec le
# schéma de propriétés, exFAT vide) puis écrite dans l'image à son offset.
# GRUB BIOS (i386-pc) est embarqué dans la partition « BIOS boot » sans
# grub-install (voir bios_embed) : l'image démarre en UEFI et en BIOS.
#
# Deux usages :
#   make live-image   → dist/live/secmet-key.img : clé virtuelle pour QEMU
#                       (clé de signature créée dans l'image, magasin app/
#                       et paramètres noyau d'essai possibles) ;
#   make live-dist    → dist/secmet-key_<version>_x86_64.img.xz : image à
#                       publier avec la release, à écrire telle quelle sur
#                       une clé (Raspberry Pi Imager, Rufus, dd). Pas de clé de
#                       signature dedans : chaque clé crée la sienne au
#                       premier démarrage (secmet-first-boot), qui agrandit
#                       aussi la partition d'export jusqu'au bout de la clé.
#
# usage : packaging/live/make_key_image.sh [--dist]
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENGINE="${SECMET_ENGINE:-$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)}"
IMAGE="secmet-live-builder:ubuntu24.04"
DIST=0
[[ "${1:-}" == "--dist" ]] && DIST=1
[[ -f "$ROOT_DIR/dist/live/filesystem.squashfs" ]] || { echo "dist/live absent : make live" >&2; exit 1; }
VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
if [[ $DIST -eq 1 ]]; then
    image_version="$(sed -E 's/^secmet ([^~ ]+).*/\1/' "$ROOT_DIR/dist/live/VERSION")"
    [[ "$image_version" == "$VERSION" ]] || { echo "image en $image_version, Cargo.toml en $VERSION : make live" >&2; exit 1; }
fi
"$ENGINE" build -q -t "$IMAGE" -f "$ROOT_DIR/packaging/live/live.Containerfile" "$ROOT_DIR/packaging/live" >/dev/null
mkdir -p "$ROOT_DIR/target-live" "$ROOT_DIR/dist"
# SECMET_KERNEL_EXTRA : paramètres noyau ajoutés à l'entrée principale de la
# clé virtuelle (ex. « console=ttyS0,115200n8 console=tty0 » : shell série
# pour le débogage dans QEMU). Jamais sur une clé réelle ni dans l'image publiée.
# SECMET_KEY_APP_DIR : dossier app/ (paquets de mise à jour signés,
# app/<version>/) copié sur la partition de données de la clé virtuelle :
# essai du service secmet-app-update en VM.
APP_MOUNT=()
[[ $DIST -eq 0 && -n "${SECMET_KEY_APP_DIR:-}" && -d "${SECMET_KEY_APP_DIR}" ]] && APP_MOUNT=(-v "${SECMET_KEY_APP_DIR}:/appdir:ro,Z")
exec "$ENGINE" run --rm -e "SECMET_KERNEL_EXTRA=$([[ $DIST -eq 0 ]] && echo "${SECMET_KERNEL_EXTRA:-}")" \
    -e "SECMET_KEY_DIST=$DIST" -e "SECMET_KEY_VERSION=$VERSION" "${APP_MOUNT[@]}" \
    -v "$ROOT_DIR:/src:ro,Z" -v "$ROOT_DIR/dist:/dist:Z" -v "$ROOT_DIR/dist/live:/out:Z" -v "$ROOT_DIR/target-live:/target:Z" \
    -v secmet-live-cargo-registry:/usr/local/cargo/registry:Z \
    "$IMAGE" bash -c '
set -euo pipefail
OUT=/out; SRC=/src; W=/tmp/key; rm -rf "$W"; mkdir -p "$W/data" "$W/sys"
# Tailles en MiB : système 6 GiB dès 2 MiB, données 4 GiB (comme la clé
# physique), export 1 GiB, agrandi au premier démarrage jusqu au bout de
# la clé (secmet-first-boot). Clé de 12 Go au moins ; 16 Go conseillés.
SYS_START=2; SYS_MIB=6144; DATA_START=$((SYS_START+SYS_MIB)); DATA_MIB=4096
EXP_START=$((DATA_START+DATA_MIB)); EXP_MIB=1024; TOTAL_MIB=$((EXP_START+EXP_MIB+1))
BIOS_SECTOR=2048   # partition « BIOS boot » : 1 MiB à 1 MiB

if [ "$SECMET_KEY_DIST" = 1 ]; then
    IMG="/dist/secmet-key_${SECMET_KEY_VERSION}_x86_64.img"
    echo "== image publiable $(basename "$IMG").xz (sans clé de signature : créée au premier démarrage)"
else
    IMG="$OUT/secmet-key.img"
    echo "== secmet-release (clé de signature)"
    mkdir -p /build && tar -C "$SRC" --exclude=./target --exclude="./target-*" --exclude=./dist --exclude=./.git -cf - . | tar -C /build -xf -
    ( cd /build && CARGO_TARGET_DIR=/target cargo build --release -q -p secmet-attest --bin secmet-release )
fi

echo "== partition système (FAT32, mtools)"
mkfs.vfat -F32 -n SECMET-SYS -C "$W/p1.img" $((SYS_MIB*1024)) >/dev/null
export MTOOLS_SKIP_CHECK=1
mmd -i "$W/p1.img" ::live ::EFI ::EFI/BOOT ::EFI/ubuntu ::boot ::boot/grub ::boot/grub/i386-pc ::boot/grub/fonts
mcopy -i "$W/p1.img" "$OUT/vmlinuz" "$OUT/initrd.img" "$OUT/filesystem.squashfs" ::live/
mcopy -i "$W/p1.img" "$OUT/efi/shimx64.efi" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "$W/p1.img" "$OUT/efi/grubx64.efi" ::EFI/BOOT/grubx64.efi
[ -f "$OUT/efi/mmx64.efi" ] && mcopy -i "$W/p1.img" "$OUT/efi/mmx64.efi" ::EFI/BOOT/mmx64.efi
sed "s|net.ifnames=0|net.ifnames=0 ${SECMET_KERNEL_EXTRA:-}|" "$SRC/packaging/live/grub.cfg" > "$W/grub.cfg"
mcopy -i "$W/p1.img" "$W/grub.cfg" ::EFI/BOOT/grub.cfg
mcopy -i "$W/p1.img" "$W/grub.cfg" ::EFI/ubuntu/grub.cfg
mcopy -i "$W/p1.img" "$W/grub.cfg" ::boot/grub/grub.cfg
# GRUB BIOS : modules et police dans boot/grub, comme grub-install les y met.
mcopy -i "$W/p1.img" "$OUT"/bios/*.mod "$OUT"/bios/*.lst ::boot/grub/i386-pc/
mcopy -i "$W/p1.img" /usr/share/grub/unicode.pf2 ::boot/grub/fonts/

echo "== partition de données (ext4, mke2fs -d)"
D="$W/data"
for d in config logs lots archive app; do mkdir -p "$D/$d"; chown 1000:1000 "$D/$d"; chmod 700 "$D/$d"; done
mkdir -p "$D/vault"; chown 0:0 "$D/vault"; chmod 700 "$D/vault"
if [ "$SECMET_KEY_DIST" = 0 ]; then
    /target/release/secmet-release keygen "$D/config/attestation-key.hex" | sed "s/^/  /"
    chown 1000:1000 "$D/config/attestation-key.hex" "$D/config/attestation-key.pub"
fi
printf "{\n  \"language\": \"fr\",\n  \"check_updates\": true\n}\n" > "$D/config/settings.json"
chown 1000:1000 "$D/config/settings.json"
if [ -d /appdir ]; then cp -r /appdir/. "$D/app/"; chown -R 1000:1000 "$D/app"; echo "  app/ : $(ls "$D/app" | tr "\n" " ")"; fi
chown 0:0 "$D"; chmod 755 "$D"
mke2fs -q -t ext4 -L SECMET-DATA -E lazy_itable_init=0,lazy_journal_init=0 -d "$D" "$W/p2.img" "${DATA_MIB}M"

echo "== partition d export (exFAT)"
truncate -s "${EXP_MIB}M" "$W/p3.img"
mkfs.exfat -L SECMET-EXP "$W/p3.img" >/dev/null

echo "== assemblage GPT"
rm -f "$IMG"; truncate -s "${TOTAL_MIB}M" "$IMG"
sfdisk -q "$IMG" <<PART
label: gpt
start=${SYS_START}MiB, size=${SYS_MIB}MiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="SECMET-SYS"
start=${DATA_START}MiB, size=${DATA_MIB}MiB, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="SECMET-DATA"
start=${EXP_START}MiB, size=${EXP_MIB}MiB, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="SECMET-EXPORT"
start=1MiB, size=1MiB, type=21686148-6449-6E6F-744E-656564454649, name="BIOS boot"
PART
dd if="$W/p1.img" of="$IMG" bs=1M seek=$SYS_START conv=notrunc,sparse status=none
dd if="$W/p2.img" of="$IMG" bs=1M seek=$DATA_START conv=notrunc,sparse status=none
dd if="$W/p3.img" of="$IMG" bs=1M seek=$EXP_START conv=notrunc,sparse status=none

echo "== GRUB BIOS (partition BIOS boot, secteur $BIOS_SECTOR)"
# Ce que fait grub-bios-setup, sans périphérique bloc (impossible dans le
# conteneur) : core.img (noyau GRUB + biosdisk, part_gpt, fat ; préfixe
# (,gpt1)/boot/grub, où sont normal.mod et grub.cfg) écrit entier dans la
# partition « BIOS boot », boot.img dans le secteur 0 avec, à 0x5c, le
# secteur du core, à 0x66 le contrôle du lecteur neutralisé (jmp → nop nop,
# BIOS qui passent un mauvais DL) et la table de partitions du MBR
# protecteur conservée (0x1b8..0x1fe). Dans core.img, la première entrée
# de la liste de blocs (12 derniers octets du secteur d amorçage : début
# u64, longueur u16, segment u16) reçoit le secteur qui suit ; longueur et
# segment sont déjà posés par grub-mkimage. Sans redondance Reed-Solomon
# (facultative). Vérifié dans QEMU avec SeaBIOS (packaging/live/image-check.sh).
grub-mkimage -O i386-pc -d "$OUT/bios" -p "(,gpt1)/boot/grub" -o "$W/core.img" biosdisk part_gpt part_msdos fat
core_size=$(stat -c %s "$W/core.img"); core_sectors=$(( (core_size + 511) / 512 ))
[ "$core_sectors" -le 2048 ] || { echo "core.img ($core_size octets) dépasse la partition BIOS boot" >&2; exit 1; }
le64() { v=$1; i=0; while [ $i -lt 8 ]; do printf "\\$(printf %03o $(( (v >> (8*i)) & 255 )))"; i=$((i+1)); done; }
cp "$OUT/bios/boot.img" "$W/boot.img"
le64 "$BIOS_SECTOR" | dd of="$W/boot.img" bs=1 seek=$((0x5c)) conv=notrunc status=none
printf "\220\220" | dd of="$W/boot.img" bs=1 seek=$((0x66)) conv=notrunc status=none
dd if="$IMG" of="$W/boot.img" bs=1 skip=$((0x1b8)) seek=$((0x1b8)) count=$((0x1fe - 0x1b8)) conv=notrunc status=none
le64 $((BIOS_SECTOR + 1)) | dd of="$W/core.img" bs=1 seek=$((512 - 12)) conv=notrunc status=none
dd if="$W/boot.img" of="$IMG" bs=512 count=1 conv=notrunc status=none
dd if="$W/core.img" of="$IMG" bs=512 seek="$BIOS_SECTOR" conv=notrunc status=none
echo "  core.img : $core_size octets ($core_sectors secteurs)"
chmod 0644 "$IMG"; sfdisk -l "$IMG" | tail -4; du -h "$IMG"

if [ "$SECMET_KEY_DIST" = 1 ]; then
    echo "== compression xz (blocs indépendants : décompression parallèle, Imager et Rufus l ouvrent tels quels)"
    rm -f "$IMG.xz"
    xz -T0 -6 -c "$IMG" > "$IMG.xz" && rm -f "$IMG"
    ls -l "$IMG.xz" | awk "{print \"  \" \$NF \" : \" \$5 \" octets\"}"
fi'
