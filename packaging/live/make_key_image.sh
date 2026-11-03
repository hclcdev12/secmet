#!/usr/bin/env bash
# Fabrique une clé SecMet virtuelle (fichier image GPT) sans montage ni
# privilège, dans le conteneur de construction : chaque partition est
# produite comme un fichier (FAT32 remplie par mtools, ext4 peuplée par
# mke2fs -d avec le schéma de propriétés, exFAT vide) puis écrite dans
# l'image à son offset. Pour QEMU (packaging/live/qemu-test.sh). GRUB BIOS
# n'est pas installé (UEFI seulement) ; la clé physique passe par
# secmet-make-key.sh. Sortie : dist/live/secmet-key.img.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENGINE="${SECMET_ENGINE:-$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)}"
IMAGE="secmet-live-builder:ubuntu24.04"
[[ -f "$ROOT_DIR/dist/live/filesystem.squashfs" ]] || { echo "dist/live absent : make live" >&2; exit 1; }
"$ENGINE" build -q -t "$IMAGE" -f "$ROOT_DIR/packaging/live/live.Containerfile" "$ROOT_DIR/packaging/live" >/dev/null
mkdir -p "$ROOT_DIR/target-live"
# SECMET_KERNEL_EXTRA : paramètres noyau ajoutés à l'entrée principale de la
# clé virtuelle (ex. « console=ttyS0,115200n8 console=tty0 » : shell série
# pour le débogage dans QEMU). Jamais sur une clé réelle.
# SECMET_KEY_APP_DIR : dossier app/ (paquets de mise à jour signés,
# app/<version>/) copié sur la partition de données de la clé virtuelle :
# essai du service secmet-app-update en VM.
APP_MOUNT=()
[[ -n "${SECMET_KEY_APP_DIR:-}" && -d "${SECMET_KEY_APP_DIR}" ]] && APP_MOUNT=(-v "${SECMET_KEY_APP_DIR}:/appdir:ro,Z")
exec "$ENGINE" run --rm -e "SECMET_KERNEL_EXTRA=${SECMET_KERNEL_EXTRA:-}" "${APP_MOUNT[@]}" \
    -v "$ROOT_DIR:/src:ro,Z" -v "$ROOT_DIR/dist/live:/out:Z" -v "$ROOT_DIR/target-live:/target:Z" \
    -v secmet-live-cargo-registry:/usr/local/cargo/registry:Z \
    "$IMAGE" bash -c '
set -euo pipefail
OUT=/out; SRC=/src; W=/tmp/key; rm -rf "$W"; mkdir -p "$W/data" "$W/sys"
# Tailles en MiB : système 6 GiB dès 2 MiB, données 4 GiB (comme la clé
# physique), export 1 GiB, agrandi au premier démarrage jusqu au bout de
# la clé (secmet-first-boot). Clé de 12 Go au moins ; 16 Go conseillés.
SYS_START=2; SYS_MIB=6144; DATA_START=$((SYS_START+SYS_MIB)); DATA_MIB=4096
EXP_START=$((DATA_START+DATA_MIB)); EXP_MIB=1024; TOTAL_MIB=$((EXP_START+EXP_MIB+1))

echo "== secmet-release (clé de signature)"
mkdir -p /build && tar -C "$SRC" --exclude=./target --exclude="./target-*" --exclude=./dist --exclude=./.git -cf - . | tar -C /build -xf -
( cd /build && CARGO_TARGET_DIR=/target cargo build --release -q -p secmet-attest --bin secmet-release )

echo "== partition système (FAT32, mtools)"
mkfs.vfat -F32 -n SECMET-SYS -C "$W/p1.img" $((SYS_MIB*1024)) >/dev/null
export MTOOLS_SKIP_CHECK=1
mmd -i "$W/p1.img" ::live ::EFI ::EFI/BOOT ::EFI/ubuntu ::boot ::boot/grub
mcopy -i "$W/p1.img" "$OUT/vmlinuz" "$OUT/initrd.img" "$OUT/filesystem.squashfs" ::live/
mcopy -i "$W/p1.img" "$OUT/efi/shimx64.efi" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "$W/p1.img" "$OUT/efi/grubx64.efi" ::EFI/BOOT/grubx64.efi
[ -f "$OUT/efi/mmx64.efi" ] && mcopy -i "$W/p1.img" "$OUT/efi/mmx64.efi" ::EFI/BOOT/mmx64.efi
sed "s|net.ifnames=0|net.ifnames=0 ${SECMET_KERNEL_EXTRA:-}|" "$SRC/packaging/live/grub.cfg" > "$W/grub.cfg"
mcopy -i "$W/p1.img" "$W/grub.cfg" ::EFI/BOOT/grub.cfg
mcopy -i "$W/p1.img" "$W/grub.cfg" ::EFI/ubuntu/grub.cfg
mcopy -i "$W/p1.img" "$W/grub.cfg" ::boot/grub/grub.cfg

echo "== partition de données (ext4, mke2fs -d)"
D="$W/data"
for d in config logs lots archive; do mkdir -p "$D/$d"; chown 1000:1000 "$D/$d"; chmod 700 "$D/$d"; done
mkdir -p "$D/vault"; chown 0:0 "$D/vault"; chmod 700 "$D/vault"
/target/release/secmet-release keygen "$D/config/attestation-key.hex" | sed "s/^/  /"
chown 1000:1000 "$D/config/attestation-key.hex" "$D/config/attestation-key.pub"
printf "{\n  \"language\": \"fr\",\n  \"check_updates\": true\n}\n" > "$D/config/settings.json"
chown 1000:1000 "$D/config/settings.json"
if [ -d /appdir ]; then mkdir -p "$D/app"; cp -r /appdir/. "$D/app/"; chown -R 1000:1000 "$D/app"; echo "  app/ : $(ls "$D/app" | tr "\n" " ")"; fi
chown 0:0 "$D"; chmod 755 "$D"
mke2fs -q -t ext4 -L SECMET-DATA -E lazy_itable_init=0,lazy_journal_init=0 -d "$D" "$W/p2.img" "${DATA_MIB}M"

echo "== partition d export (exFAT)"
truncate -s "${EXP_MIB}M" "$W/p3.img"
mkfs.exfat -L SECMET-EXP "$W/p3.img" >/dev/null

echo "== assemblage GPT"
IMG="$OUT/secmet-key.img"; rm -f "$IMG"; truncate -s "${TOTAL_MIB}M" "$IMG"
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
chmod 0644 "$IMG"; sfdisk -l "$IMG" | tail -4; du -h "$IMG"'
