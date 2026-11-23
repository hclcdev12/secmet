#!/usr/bin/env bash
# Test du script de démarrage de la clé `secmet-image-check` (overlay), hors
# de la clé : support live et partition de données factices, commandes
# système remplacées par des doublures dans le PATH (mountpoint, blkid,
# mount, umount, logger, et secmet-release sauf avec SECMET_RELEASE_BIN).
# Vérifie le rapport écrit pour l'application (/run/secmet-image), sa copie
# sur la partition de données et la ligne du journal : système intact,
# altéré, partition introuvable, montage impossible, outil absent.
# Avec SECMET_RELEASE_BIN=<secmet-release compilé>, le vrai vérificateur est
# employé sur une partition système d'essai scellée par une clé de test :
# intacte, puis grub.cfg modifié (altérée), puis sans signature.
#
# usage : packaging/live/tests/image-check.sh        (code 0 si tout passe)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT="$ROOT_DIR/packaging/live/overlay/usr/local/bin/secmet-image-check"
T="$(mktemp -d "${TMPDIR:-/tmp}/secmet-image-check-XXXXXX")"
trap 'rm -rf "$T"' EXIT
BIN="$T/bin"
mkdir -p "$BIN"
failures=0

# --- doublures -------------------------------------------------------------
cat > "$BIN/mountpoint" <<'SH'
#!/bin/sh
# mountpoint -q <dossier> : vrai si <dossier> est listé dans $FAKE_MOUNTS.
grep -qx -- "$2" "$FAKE_MOUNTS" 2>/dev/null
SH
cat > "$BIN/blkid" <<'SH'
#!/bin/sh
# blkid -L SECMET-SYS : $FAKE_SYS_DEV s'il est renseigné.
[ -n "${FAKE_SYS_DEV:-}" ] && echo "$FAKE_SYS_DEV"
SH
cat > "$BIN/mount" <<'SH'
#!/bin/sh
# mount -o … <périphérique> <dossier> : échoue si FAKE_MOUNT_FAIL=1 ; sinon
# copie $FAKE_SYS_TREE dans <dossier> et le déclare monté.
[ "${FAKE_MOUNT_FAIL:-0}" = 1 ] && exit 32
cp -r "$FAKE_SYS_TREE/." "$4/" && echo "$4" >> "$FAKE_MOUNTS"
SH
cat > "$BIN/umount" <<'SH'
#!/bin/sh
grep -vx -- "$1" "$FAKE_MOUNTS" > "$FAKE_MOUNTS.new"; mv "$FAKE_MOUNTS.new" "$FAKE_MOUNTS"
SH
cat > "$BIN/logger" <<'SH'
#!/bin/sh
shift 2
echo "$*" >> "$FAKE_LOGGER"
SH
if [[ -z "${SECMET_RELEASE_BIN:-}" ]]; then
    cat > "$BIN/secmet-release" <<'SH'
#!/bin/sh
# verify-system <dossier> [--trust <clé>]... --json <fichier> : rapport
# $FAKE_STATUS si <dossier>/live/filesystem.squashfs existe.
echo "verify-system $2" >> "$FAKE_CALLS"
json=""; prev=""
for a in "$@"; do
    [ "$prev" = --json ] && json="$a"
    [ "$prev" = --trust ] && echo "$a" >> "$FAKE_CALLS.trust"
    prev="$a"
done
[ -f "$2/live/filesystem.squashfs" ] || exit 1
printf '{\n  "status": "%s",\n  "system": "secmet essai",\n  "files": 3,\n  "problems": []\n}\n' "$FAKE_STATUS" > "$json"
echo "système      : secmet essai"
echo "état         : $FAKE_STATUS"
[ "$FAKE_STATUS" = ok ] || echo "  boot/grub/grub.cfg : empreinte différente"
[ "$FAKE_STATUS" = ok ]
SH
else
    ln -s "$SECMET_RELEASE_BIN" "$BIN/secmet-release"
fi
chmod +x "$BIN"/*

# --- aide ------------------------------------------------------------------
check() { # <nom> <attendu> <obtenu>
    if [[ "$2" == "$3" ]]; then
        echo "ok    $1"
    else
        echo "ÉCHEC $1 : attendu « $2 », obtenu « $3 »"
        failures=$((failures + 1))
    fi
}
setup() {
    rm -rf "$T/run" "$T/medium" "$T/data" "$T/tree"
    mkdir -p "$T/run" "$T/medium/live" "$T/data" "$T/tree/live"
    : > "$T/mounts"; : > "$T/logger"; : > "$T/calls"; rm -f "$T/calls.trust"
    echo squashfs > "$T/medium/live/filesystem.squashfs"
    echo squashfs > "$T/tree/live/filesystem.squashfs"
    unset FAKE_SYS_DEV FAKE_MOUNT_FAIL
    export FAKE_STATUS=ok
}
run() {
    PATH="$BIN:$PATH" FAKE_MOUNTS="$T/mounts" FAKE_LOGGER="$T/logger" FAKE_CALLS="$T/calls" \
        FAKE_SYS_TREE="$T/tree" SECMET_IMAGE_CHECK_RUN="$T/run" \
        SECMET_IMAGE_CHECK_MEDIUM="$T/medium" SECMET_IMAGE_CHECK_DATA="$T/data" \
        sh "$SCRIPT"
}
status() { sed -n 's/^ *"status": *"\([a-z]*\)".*/\1/p' "$T/run/system-integrity.json" | head -1; }

if [[ -z "${SECMET_RELEASE_BIN:-}" ]]; then
    # 1. Support live monté (hors toram) : vérifié sur place ; rapport copié
    #    sur la partition de données montée.
    setup; echo "$T/medium" >> "$T/mounts"; echo "$T/data" >> "$T/mounts"
    run
    check "support live : rapport" "ok" "$(status)"
    check "support live : vérifié sur place" "verify-system $T/medium" "$(cat "$T/calls")"
    check "clés de version passées à secmet-release" "bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5" "$(cat "$T/calls.trust")"
    check "copie sur la partition de données" "ok" "$(sed -n 's/^ *"status": *"\([a-z]*\)".*/\1/p' "$T/data/logs/system/image-check.json")"
    check "journal" "1" "$(grep -c '^système de la clé : ok (.* s ; secmet essai)$' "$T/logger")"

    # 2. Image altérée : rapport « altered », écart au journal.
    setup; echo "$T/medium" >> "$T/mounts"; FAKE_STATUS=altered
    run
    check "altérée : rapport" "altered" "$(status)"
    check "altérée : écart au journal" "1" "$(grep -c 'altered .*boot/grub/grub.cfg : empreinte différente' "$T/logger")"
    check "données non montées : pas de copie" "no" "$([[ -e "$T/data/logs" ]] && echo yes || echo no)"

    # 3. toram : support non monté, partition retrouvée par étiquette, montée
    #    en lecture seule puis démontée.
    setup; export FAKE_SYS_DEV=/dev/null
    run
    check "toram : rapport" "ok" "$(status)"
    check "toram : vérifié sur le montage" "verify-system $T/run/sys" "$(cat "$T/calls")"
    check "toram : démonté" "" "$(cat "$T/mounts")"

    # 4. Clé retirée (toram) : indisponible.
    setup
    run
    check "clé retirée : rapport" "unavailable" "$(status)"
    check "clé retirée : rien vérifié" "" "$(cat "$T/calls")"

    # 5. Montage impossible : erreur.
    setup; export FAKE_SYS_DEV=/dev/null FAKE_MOUNT_FAIL=1
    run
    check "montage impossible : rapport" "error" "$(status)"

    # 6. Le vérificateur échoue sans écrire de rapport : erreur, jamais
    #    « pending » laissé derrière.
    #    (Partition montée sans squashfs : la doublure échoue en silence.)
    setup; export FAKE_SYS_DEV=/dev/null
    rm -f "$T/medium/live/filesystem.squashfs" "$T/tree/live/filesystem.squashfs"
    run
    check "vérificateur sans rapport : erreur" "error" "$(status)"
else
    # Vrai vérificateur : partition d'essai scellée par une clé de test.
    setup
    "$SECMET_RELEASE_BIN" keygen "$T/test-key.hex" > /dev/null
    trust="$(tr -d '[:space:]' < "$T/test-key.pub")"
    mkdir -p "$T/medium/EFI/BOOT" "$T/medium/boot/grub"
    echo shim > "$T/medium/EFI/BOOT/BOOTX64.EFI"
    echo "set timeout=0" > "$T/medium/boot/grub/grub.cfg"
    "$ROOT_DIR/packaging/live/system-sums.sh" "$T/medium" "secmet essai" > "$T/medium/SYSTEM.sums"
    "$SECMET_RELEASE_BIN" sign-system "$T/medium" --key "$T/test-key.hex" > /dev/null
    # Les clés de confiance du script sont les clés de version : remplacées
    # dans une copie par une autre clé (future clé de version, qui n'a rien
    # signé) suivie de celle de test : une signature par l'une suffit.
    "$SECMET_RELEASE_BIN" keygen "$T/other-key.hex" > /dev/null
    other="$(tr -d '[:space:]' < "$T/other-key.pub")"
    awk -v k="$trust" -v o="$other" '/^TRUST_KEYS="$/ { print; print o; print k; skip = 1; next } skip && /^"$/ { skip = 0 } !skip' "$SCRIPT" > "$T/script"; SCRIPT="$T/script"
    echo "$T/medium" >> "$T/mounts"
    run
    check "réel : système scellé intact" "ok" "$(status)"
    echo "set timeout=5" > "$T/medium/boot/grub/grub.cfg"
    run
    check "réel : grub.cfg modifié" "altered" "$(status)"
    check "réel : écart nommé" "1" "$(grep -c '"boot/grub/grub.cfg : empreinte différente"' "$T/run/system-integrity.json")"
    echo "set timeout=0" > "$T/medium/boot/grub/grub.cfg"; rm -f "$T/medium/SYSTEM.sums.sig"
    run
    check "réel : sans signature" "unsigned" "$(status)"
fi

if [[ $failures -ne 0 ]]; then
    echo "$failures échec(s)"
    exit 1
fi
echo "secmet-image-check : tous les cas passent"
