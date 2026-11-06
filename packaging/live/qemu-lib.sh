#!/usr/bin/env bash
# Fonctions communes aux contrôles QEMU de la clé (qemu-check.sh,
# image-check.sh) : démarrage sans écran d'une image de clé, extinction,
# extraction de la partition de données et des journaux de démarrage
# (logs/system/<boot-id>.log, copiés par secmet-journal-sync). À sourcer ;
# attend LIVE (packaging/live) et fail().
# shellcheck shell=bash

qemu_tools_check() {
    local tool
    for tool in qemu-system-x86_64 e2fsck debugfs sfdisk python3; do
        command -v "$tool" >/dev/null || fail "$tool introuvable"
    done
}

# qemu_part <image> <nom GPT> → « début taille » en secteurs
qemu_part() {
    sfdisk -J "$1" | python3 -c '
import json, sys
p = json.load(sys.stdin)["partitiontable"]["partitions"]
d = [x for x in p if x.get("name") == sys.argv[1]][0]
print(d["start"], d["size"])' "$2"
}

# qemu_run_and_collect <image> <dossier de travail> <attente en s>
# Démarre l'image (qemu-test.sh --headless ; SECMET_QEMU_FIRMWARE=bios pour
# SeaBIOS), attend que la minuterie ait copié le journal, éteint (ACPI, puis
# arrêt sec : e2fsck rejoue le journal ext4), puis pose :
#   QEMU_DATA    partition de données extraite (fichier ext4)
#   QEMU_JOURNAL journaux de démarrage concaténés
qemu_run_and_collect() {
    local img=$1 work=$2 wait=$3 pid start size logs l
    echo "== démarrage QEMU (sans affichage, ${SECMET_QEMU_FIRMWARE:-uefi}), attente ${wait} s"
    export SECMET_QEMU_IMAGE="$img" SECMET_QEMU_WORK="$work/qemu" \
        SECMET_QEMU_MON="$work/qemu/mon.sock" SECMET_QEMU_QMP="$work/qemu/qmp.sock"
    "$LIVE/qemu-test.sh" --headless &
    pid=$!
    # shellcheck disable=SC2064
    trap "kill $pid 2>/dev/null || true" EXIT
    sleep "$wait"
    kill -0 "$pid" 2>/dev/null || fail "QEMU s'est arrêté avant la fin de l'attente"
    python3 "$LIVE/qemu-ctl.py" hmp system_powerdown >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 2
    done
    if kill -0 "$pid" 2>/dev/null; then
        python3 "$LIVE/qemu-ctl.py" hmp quit >/dev/null 2>&1 || kill "$pid"
        wait "$pid" 2>/dev/null || true
    fi
    trap - EXIT

    echo "== lecture du journal sur la partition de données"
    read -r start size < <(qemu_part "$img" SECMET-DATA)
    QEMU_DATA="$work/data.img"
    dd if="$img" of="$QEMU_DATA" bs=4M iflag=skip_bytes,count_bytes skip=$((start * 512)) count=$((size * 512)) conv=sparse status=none
    e2fsck -fy "$QEMU_DATA" >/dev/null 2>&1 || true
    logs=$(debugfs -R "ls -p /logs/system" "$QEMU_DATA" 2>/dev/null | awk -F/ '$6 ~ /^[0-9a-f]{32}\.log$/ {print $6}')
    [[ -n "$logs" ]] || fail "aucun journal de démarrage dans logs/system (attente trop courte ?)"
    QEMU_JOURNAL="$work/journal.log"
    : > "$QEMU_JOURNAL"
    for l in $logs; do debugfs -R "cat /logs/system/$l" "$QEMU_DATA" >> "$QEMU_JOURNAL" 2>/dev/null; done
}
