#!/usr/bin/env bash
# Contrôle de la clé SecMet virtuelle dans QEMU : la mise à jour au
# démarrage (`secmet-app-update`) choisit la version la plus récente du
# magasin app/, la vérifie et la déploie. Sans écran, sans privilège.
#
# 1. magasin de deux versions factices, 9.9.8 et 9.9.9 (le même .deb de
#    dist/ renommé), signé avec la clé de version si elle est disponible
#    (SECMET_RELEASE_KEY, défaut ~/.config/secmet/release-key.hex) ;
# 2. clé virtuelle fabriquée avec ce magasin (make_key_image.sh) ;
# 3. démarrage dans QEMU (qemu-test.sh --headless), attente de la copie du
#    journal sur la partition de données (minuterie secmet-journal-sync),
#    extinction ;
# 4. lecture du journal depuis l'image (partition extraite, e2fsck, debugfs)
#    et vérification :
#      avec la clé : « version 9.9.9 : signature et empreinte vérifiées »
#        puis « SecMet 9.9.9 déployée », et jamais 9.9.8 ;
#      sans la clé : 9.9.9 refusée (« signature ou empreinte invalide,
#        ignorée ») avant 9.9.8, rien de déployé.
#
# Prérequis : dist/live (make live), un .deb dans dist/ (make deb), podman ou
# docker, qemu-system-x86_64 avec KVM, OVMF, e2fsprogs (e2fsck, debugfs).
# Durée : 4 à 5 minutes. usage : make live-check
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE="$ROOT_DIR/packaging/live"
WORK="$ROOT_DIR/dist/live/check"
IMG="$ROOT_DIR/dist/live/secmet-key.img"
KEY="${SECMET_RELEASE_KEY:-$HOME/.config/secmet/release-key.hex}"
WAIT="${SECMET_CHECK_WAIT:-150}"
OLD=9.9.8
NEW=9.9.9
fail() { echo "ÉCHEC : $*" >&2; exit 1; }

[[ -f "$ROOT_DIR/dist/live/filesystem.squashfs" ]] || fail "dist/live absent : make live"
for tool in qemu-system-x86_64 e2fsck debugfs sfdisk; do
    command -v "$tool" >/dev/null || fail "$tool introuvable"
done
deb=$(ls "$ROOT_DIR"/dist/secmet_*~ubuntu24.04_amd64.deb 2>/dev/null | sort -V | tail -1 || true)
[[ -n "$deb" ]] || fail "aucun dist/secmet_*~ubuntu24.04_amd64.deb : make deb"

rm -rf "$WORK"
mkdir -p "$WORK/store/$OLD" "$WORK/store/$NEW"
signed=0
if [[ -f "$KEY" ]]; then
    signed=1
    ( cd "$ROOT_DIR" && cargo build -q --release -p secmet-attest --bin secmet-release )
    RELEASE="$ROOT_DIR/target/release/secmet-release"
fi
for v in "$OLD" "$NEW"; do
    cp "$deb" "$WORK/store/$v/secmet_${v}.ubuntu24.04_amd64.deb"
    if [[ $signed == 1 ]]; then
        "$RELEASE" sign "$WORK/store/$v" --key "$KEY" --version "$v" --date "$(date +%F)" \
            --sums-url "http://127.0.0.1/$v/SHA256SUMS" >/dev/null
    fi
done
echo "== magasin : $OLD et $NEW ($(basename "$deb"))$([[ $signed == 1 ]] && echo ', signé' || echo ', non signé : seul le refus est contrôlé')"

echo "== clé virtuelle"
SECMET_KEY_APP_DIR="$WORK/store" "$LIVE/make_key_image.sh" | tail -1

echo "== démarrage QEMU (sans affichage), attente ${WAIT} s"
export SECMET_QEMU_WORK="$WORK/qemu" SECMET_QEMU_MON="$WORK/qemu/mon.sock" SECMET_QEMU_QMP="$WORK/qemu/qmp.sock"
"$LIVE/qemu-test.sh" --headless &
QEMU=$!
trap 'kill "$QEMU" 2>/dev/null || true' EXIT
sleep "$WAIT"
kill -0 "$QEMU" 2>/dev/null || fail "QEMU s'est arrêté avant la fin de l'attente"
# Extinction propre (ACPI) si la session la laisse passer, sinon arrêt sec :
# le journal a déjà été copié par la minuterie, et e2fsck rejoue le journal
# ext4 de la partition extraite.
python3 "$LIVE/qemu-ctl.py" hmp system_powerdown >/dev/null 2>&1 || true
for _ in $(seq 1 30); do
    kill -0 "$QEMU" 2>/dev/null || break
    sleep 2
done
if kill -0 "$QEMU" 2>/dev/null; then
    python3 "$LIVE/qemu-ctl.py" hmp quit >/dev/null 2>&1 || kill "$QEMU"
    wait "$QEMU" 2>/dev/null || true
fi
trap - EXIT

echo "== lecture du journal sur la partition de données"
read -r start size < <(sfdisk -J "$IMG" | python3 -c '
import json, sys
p = json.load(sys.stdin)["partitiontable"]["partitions"]
d = [x for x in p if x.get("name") == "SECMET-DATA"][0]
print(d["start"], d["size"])')
P2="$WORK/data.img"
dd if="$IMG" of="$P2" bs=4M iflag=skip_bytes,count_bytes skip=$((start * 512)) count=$((size * 512)) conv=sparse status=none
e2fsck -fy "$P2" >/dev/null 2>&1 || true
logs=$(debugfs -R "ls -p /logs/system" "$P2" 2>/dev/null | awk -F/ '$6 ~ /^[0-9a-f]{32}\.log$/ {print $6}')
[[ -n "$logs" ]] || fail "aucun journal de démarrage dans logs/system (attente trop courte ?)"
JOURNAL="$WORK/journal.log"
: > "$JOURNAL"
for l in $logs; do debugfs -R "cat /logs/system/$l" "$P2" >> "$JOURNAL" 2>/dev/null; done
grep -h "secmet-app-update\[" "$JOURNAL" | sed 's/.*secmet-app-update\[[0-9]*\]: /  /' || true
lines=$(grep -c "secmet-app-update\[" "$JOURNAL" || true)
[[ "$lines" -gt 0 ]] || fail "secmet-app-update n'a rien journalisé (voir $JOURNAL)"

echo "== vérification"
first=$(grep "secmet-app-update\[" "$JOURNAL" | grep -o "version 9\.9\.[89]" | head -1)
[[ "$first" == "version $NEW" ]] || fail "la première version examinée est « $first », attendu « version $NEW » (ordre du magasin)"
if [[ $signed == 1 ]]; then
    grep -q "version $NEW : signature et empreinte vérifiées" "$JOURNAL" || fail "$NEW non vérifiée"
    grep -q "SecMet $NEW déployée depuis la partition de données" "$JOURNAL" || fail "$NEW non déployée"
    ! grep -q "version $OLD" "$JOURNAL" || fail "$OLD examinée alors que $NEW a été déployée"
    echo "clé virtuelle : $NEW vérifiée et déployée au démarrage, $OLD laissée en repli"
else
    grep -q "version $NEW : signature ou empreinte invalide, ignorée" "$JOURNAL" || fail "$NEW non refusée"
    grep -q "version $OLD : signature ou empreinte invalide, ignorée" "$JOURNAL" || fail "$OLD non refusée"
    ! grep -q "déployée" "$JOURNAL" || fail "une version non signée a été déployée"
    echo "clé virtuelle : $NEW puis $OLD refusées (non signées), rien de déployé"
fi
