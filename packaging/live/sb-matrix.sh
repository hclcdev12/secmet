#!/usr/bin/env bash
# Matrice Secure Boot de l'image de la clé : démarre l'image sous OVMF avec
# Secure Boot ACTIF et quatre jeux de clés enrôlées, comme sur de vrais PC :
#
#   V1 ancien-pc     KEK 2011, db 2011 (Windows PCA + UEFI CA), dbx 2011 récente
#   V2 pc-neuf       KEK 2023, db 2023 seulement (UEFI CA + Option ROM), dbx 2023
#   V3 les-deux      jeu de référence de la distribution (2011 et 2023)
#   V4 secured-core  KEK 2011+2023, db Windows seulement (sans « UEFI CA » tierce)
#
# Le shim a démarré si le firmware a accepté sa signature : il écrit alors la
# variable SbatLevel, lue ensuite dans le magasin de variables (indépendant
# des journaux de débogage d'OVMF). Une capture d'écran est gardée par variante.
#
# Résultat attendu pour un shim signé seulement « Microsoft Corporation UEFI
# CA 2011 » (Ubuntu 24.04, shim 15.8) : V1 et V3 démarrent, V2 et V4 refusent.
# Shim à double signature 2011 + 2023 (Debian 13, shim 16.1) : V1 à V3.
# V4 refuse tout shim tiers : il faut activer dans le BIOS l'option « Allow
# Microsoft 3rd party UEFI CA » (PC Secured-core).
#
# usage : packaging/live/sb-matrix.sh [--require-2023] [image]
#   image          .img ou .img.xz (défaut : dist/live/secmet-key.img)
#   --require-2023 échoue aussi si V2 (PC neuf, 2023 seulement) refuse
# Code de sortie : 0 si V1 et V3 démarrent (et V2 avec --require-2023).
# Dépendances : qemu-system-x86_64, edk2-ovmf, virt-fw-vars
# (Fedora : dnf install python3-virt-firmware ; sinon pip install virt-firmware).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fail() { echo "ERREUR: $*" >&2; exit 1; }

REQUIRE_2023=0
if [[ "${1:-}" == "--require-2023" ]]; then REQUIRE_2023=1; shift; fi
IMG="${1:-$ROOT_DIR/dist/live/secmet-key.img}"
[[ -f "$IMG" ]] || fail "$IMG absent"
WAIT="${SECMET_SB_WAIT:-40}"
WORK="${SECMET_SB_WORK:-$ROOT_DIR/dist/live/sb-matrix}"
OVMF="${SECMET_OVMF_DIR:-/usr/share/edk2/ovmf}"
CODE="$OVMF/OVMF_CODE.secboot.fd"
for t in qemu-system-x86_64 qemu-img python3 virt-fw-vars; do
    command -v "$t" >/dev/null || fail "$t introuvable"
done
for f in "$CODE" "$OVMF/OVMF_VARS.fd" "$OVMF/OVMF_VARS.secboot.fd"; do
    [[ -f "$f" ]] || fail "$f introuvable (edk2-ovmf)"
done
DBX11="$(find "$OVMF" -maxdepth 1 -name 'DBXUpdate-*.2011.x64.bin' | sort | tail -n 1)"
DBX23="$(find "$OVMF" -maxdepth 1 -name 'DBXUpdate-*.2023.x64.bin' | sort | tail -n 1)"
[[ -n "$DBX11" && -n "$DBX23" ]] || fail "mises à jour dbx DBXUpdate-*.x64.bin introuvables dans $OVMF"

rm -rf "$WORK"; mkdir -p "$WORK/certs"

# Image : un .img.xz est décompressé jusqu'à la fin de la partition système
# (le démarrage jusqu'au noyau n'a besoin que d'elle).
DISK="$IMG"
if [[ "$IMG" == *.xz ]]; then
    echo "== décompression de l'en-tête de $IMG"
    xz -dc "$IMG" | head -c $((1024 * 1024)) > "$WORK/gpt.img" || true
    # Entrées GPT lues directement : sfdisk refuse une table dont les
    # partitions dépassent la fin du fichier tronqué.
    end=$(python3 - "$WORK/gpt.img" <<'EOF'
import struct, sys
d = open(sys.argv[1], "rb").read()
lba, n, size = struct.unpack_from("<QII", d, 512 + 72)
for i in range(n):
    e = d[lba * 512 + i * size:lba * 512 + (i + 1) * size]
    if len(e) < 128:
        break
    if e[56:128].decode("utf-16-le").rstrip("\0") == "SECMET-SYS":
        print(struct.unpack_from("<Q", e, 40)[0] + 1)
        break
EOF
)
    [[ -n "$end" ]] || fail "partition SECMET-SYS introuvable dans $IMG"
    echo "== décompression de la partition système ($((end / 2048)) Mio)"
    xz -dc "$IMG" | head -c $((end * 512)) > "$WORK/head.img" || true
    DISK="$WORK/head.img"
fi

# Jeux de clés.
( cd "$WORK/certs" && virt-fw-vars -i "$OVMF/OVMF_VARS.secboot.fd" --extract-certs >/dev/null 2>&1 )
MS=77fa9abd-0359-4d32-bd60-28f4e78f784b
pca11="$(ls "$WORK"/certs/db-*MicrosoftWindowsProductionPCA2011.pem)"
uefi11="$(ls "$WORK"/certs/db-*MicrosoftCorporationUEFICA2011.pem)"
mkvars() {
    local name=$1; shift
    virt-fw-vars -i "$OVMF/OVMF_VARS.fd" -o "$WORK/$name.fd" --enroll-redhat --sb "$@" >/dev/null 2>&1 \
        || fail "jeu de clés $name : virt-fw-vars a échoué"
}
mkvars V1-ancien-pc --microsoft-db none --microsoft-kek 2011 \
    --add-db "$MS" "$pca11" --add-db "$MS" "$uefi11" --set-dbx "$DBX11"
mkvars V2-pc-neuf --microsoft-db uefi+rom23 --microsoft-kek 2023 --set-dbx "$DBX23"
cp "$OVMF/OVMF_VARS.secboot.fd" "$WORK/V3-les-deux.fd"
mkvars V4-secured-core --microsoft-db win11 --microsoft-kek all --set-dbx "$DBX11"

ACCEL=tcg; [[ -w /dev/kvm ]] && ACCEL=kvm
boot() {
    local name=$1 r="$WORK/$1" pid
    mkdir -p "$r"
    cp "$WORK/$name.fd" "$r/vars.fd"
    qemu-img create -q -f qcow2 -F raw -b "$DISK" "$r/disk.qcow2"
    qemu-system-x86_64 -machine "q35,smm=on,accel=$ACCEL" -m 3072 \
        -global driver=cfi.pflash01,property=secure,value=on \
        -drive "if=pflash,format=raw,unit=0,readonly=on,file=$CODE" \
        -drive "if=pflash,format=raw,unit=1,file=$r/vars.fd" \
        -drive "if=none,id=k,format=qcow2,file=$r/disk.qcow2" \
        -device qemu-xhci -device usb-storage,drive=k,bootindex=1 \
        -display none -vga std -no-reboot \
        -monitor "unix:$r/mon,server,nowait" &
    pid=$!
    sleep "$WAIT"
    python3 - "$r/mon" "$r/ecran.ppm" <<'EOF' || true
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); time.sleep(0.3); s.recv(4096)
s.send(f"screendump {sys.argv[2]}\n".encode()); time.sleep(1.5)
s.send(b"quit\n"); time.sleep(0.5); s.close()
EOF
    wait "$pid" 2>/dev/null || true
    rm -f "$r/disk.qcow2"
    if virt-fw-vars -i "$r/vars.fd" --print 2>/dev/null | grep -q '^SbatLevel '; then
        echo ok
    else
        echo refus
    fi
}

echo "== Secure Boot, attente ${WAIT} s par variante ($ACCEL) ; image : $IMG"
declare -A R
for v in V1-ancien-pc V2-pc-neuf V3-les-deux V4-secured-core; do
    boot "$v" > "$WORK/$v.result" &
done
wait
status=0
for v in V1-ancien-pc V2-pc-neuf V3-les-deux V4-secured-core; do
    R[$v]="$(cat "$WORK/$v.result")"
    printf '  %-16s %s\n' "$v" "${R[$v]}"
done
[[ "${R[V1-ancien-pc]}" == ok ]] || { echo "ÉCHEC : refusé sur un PC à autorités 2011 seulement" >&2; status=1; }
[[ "${R[V3-les-deux]}" == ok ]] || { echo "ÉCHEC : refusé avec le jeu de clés de référence" >&2; status=1; }
if [[ "${R[V2-pc-neuf]}" != ok ]]; then
    if (( REQUIRE_2023 )); then
        echo "ÉCHEC : refusé sur un PC à autorités 2023 seulement" >&2; status=1
    else
        echo "  note : shim non signé par « Microsoft UEFI CA 2023 », refusé sur un PC qui n'a que les autorités 2023"
    fi
fi
echo "  captures d'écran : $WORK/<variante>/ecran.ppm"
exit "$status"
