#!/usr/bin/env bash
# Démarre la clé SecMet virtuelle (dist/live/secmet-key.img) dans QEMU en
# UEFI (OVMF), avec un disque de travail à effacer et une identité DMI de
# test. Secure Boot : SECMET_QEMU_SECBOOT=1 (firmware OVMF secboot, sans
# clés enrôlées : vérifie seulement que la chaîne signée démarre).
#
# usage : packaging/live/qemu-test.sh [--headless]   (--headless : VNC :9 + moniteur QMP)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KEY="$ROOT_DIR/dist/live/secmet-key.img"
WORK="${SECMET_QEMU_WORK:-$ROOT_DIR/dist/live/qemu}"
mkdir -p "$WORK"
[[ -f "$KEY" ]] || { echo "$KEY absent : packaging/live/make_key_image.sh" >&2; exit 1; }
CODE=/usr/share/edk2/ovmf/OVMF_CODE.fd
VARS_SRC=/usr/share/edk2/ovmf/OVMF_VARS.fd
[[ "${SECMET_QEMU_SECBOOT:-0}" == "1" ]] && CODE=/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd
[[ -f "$WORK/OVMF_VARS.fd" ]] || cp "$VARS_SRC" "$WORK/OVMF_VARS.fd"
# Disque de travail : 2 Gio, SATA (AHCI), comme un disque interne de laptop.
[[ -f "$WORK/laptop-disk.qcow2" ]] || qemu-img create -q -f qcow2 "$WORK/laptop-disk.qcow2" 2G
DISPLAY_ARGS=(-display gtk)
SERIAL="file:$WORK/serial.log"
if [[ "${1:-}" == "--headless" ]]; then
    DISPLAY_ARGS=(-display none -vnc :9 -qmp "unix:$WORK/qmp.sock,server,nowait" -monitor "unix:$WORK/mon.sock,server,nowait")
    # Port série sur socket : shell de débogage si l'image a été faite avec
    # SECMET_KERNEL_EXTRA="console=ttyS0,115200n8 console=tty0" (qemu-ctl.py serial).
    SERIAL="unix:$WORK/serial.sock,server,nowait"
fi
exec qemu-system-x86_64 -machine "${SECMET_QEMU_MACHINE:-q35}",accel=kvm -cpu host -m "${SECMET_QEMU_MEM:-4096}" -smp 2 \
    -drive "if=pflash,format=raw,readonly=on,file=$CODE" \
    -drive "if=pflash,format=raw,file=$WORK/OVMF_VARS.fd" \
    -smbios "type=1,manufacturer=Atelier Test,product=Laptop QEMU,serial=QEMU-LAPTOP-001" \
    -drive "if=none,id=usbkey,format=raw,file=$KEY" \
    -device qemu-xhci,id=xhci -device "usb-storage,bus=xhci.0,drive=usbkey,removable=on" \
    -device ahci,id=ahci -drive "if=none,id=lap,format=qcow2,file=$WORK/laptop-disk.qcow2" \
    -device "ide-hd,drive=lap,bus=ahci.0,serial=LAPTOPDISK001,model=QEMU LAPTOP SSD" \
    -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
    -vga "${SECMET_QEMU_VGA:-virtio}" -serial "$SERIAL" \
    -boot menu=off -usb -device usb-tablet "${DISPLAY_ARGS[@]}"
