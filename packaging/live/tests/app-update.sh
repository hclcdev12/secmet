#!/usr/bin/env bash
# Test du script de démarrage de la clé `secmet-app-update` (overlay), hors
# de la clé : magasin app/<version> factice, commandes système remplacées
# par des doublures dans le PATH (mountpoint, dpkg-query, dpkg, secmet-release,
# dpkg-deb, logger). Vérifie l'ordre des versions (la plus récente d'abord,
# régression du 2026-11-06), le repli sur signature invalide ou installation
# échouée, les cas « rien à faire », la garde « lot ouvert » (décision D14 :
# lot ouvert sous la version déployée, sous l'ancienne, sans version lisible,
# clôturé, plusieurs lots) et le passage de toutes les clés de version.
#
# usage : packaging/live/tests/app-update.sh        (code 0 si tout passe)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT="$ROOT_DIR/packaging/live/overlay/usr/local/bin/secmet-app-update"
# Chemin sans point, comme /media/secmet-data : le tri fautif d'avant le
# 2026-11-06 dépendait des points du chemin.
T="$(mktemp -d "${TMPDIR:-/tmp}/secmet-app-update-XXXXXX")"
trap 'rm -rf "$T"' EXIT
BIN="$T/bin"
mkdir -p "$BIN"
failures=0

# --- doublures -------------------------------------------------------------
cat > "$BIN/mountpoint" <<'SH'
#!/bin/sh
[ "${FAKE_MOUNTED:-1}" = 1 ]
SH
cat > "$BIN/dpkg-query" <<'SH'
#!/bin/sh
printf '%s~ubuntu24.04' "${FAKE_INSTALLED:-0.3.7}"
SH
cat > "$BIN/dpkg" <<'SH'
#!/bin/sh
# dpkg --compare-versions A gt B : vrai si A > B (ordre des versions).
[ "$1" = "--compare-versions" ] || exit 2
[ "$3" = "gt" ] || exit 2
[ "$2" != "$4" ] || exit 1
[ "$(printf '%s\n%s\n' "$2" "$4" | sort -V | tail -1)" = "$2" ]
SH
cat > "$BIN/secmet-release" <<'SH'
#!/bin/sh
# verify <dir> [--trust <clé>]... --file <nom> : invalide si <dir>/INVALID
# existe. Les clés reçues vont dans $FAKE_TRUST, une par ligne.
[ "$1" = verify ] || exit 2
dir=$2; shift 2
file=
: > "$FAKE_TRUST"
while [ $# -gt 0 ]; do
    case "$1" in
        --trust) echo "$2" >> "$FAKE_TRUST"; shift 2 ;;
        --file) file=$2; shift 2 ;;
        *) exit 2 ;;
    esac
done
echo "verify $dir $file" >> "$FAKE_CALLS"
[ ! -f "$dir/INVALID" ]
SH
cat > "$BIN/dpkg-deb" <<'SH'
#!/bin/sh
# -x <deb> / : note le paquet « déployé » ; échoue si <dir>/FAIL existe.
echo "$2" >> "$FAKE_DEPLOYED"
[ ! -f "$(dirname "$2")/FAIL" ]
SH
cat > "$BIN/logger" <<'SH'
#!/bin/sh
shift 2
echo "$*" >> "$FAKE_LOGGER"
SH
chmod +x "$BIN"/*

# --- aide ------------------------------------------------------------------
setup() {
    # setup <installée> <versions…> : magasin neuf avec un .deb par version.
    rm -rf "$T/data" "$T/run"
    mkdir -p "$T/data/app" "$T/data/logs/system" "$T/run"
    export FAKE_INSTALLED="$1"; shift
    for v in "$@"; do
        mkdir -p "$T/data/app/$v"
        printf 'deb' > "$T/data/app/$v/secmet_${v}.ubuntu24.04_amd64.deb"
    done
    export FAKE_CALLS="$T/calls" FAKE_DEPLOYED="$T/deployed" FAKE_LOGGER="$T/logger" FAKE_MOUNTED=1
    export FAKE_TRUST="$T/trust"
    : > "$FAKE_CALLS"; : > "$FAKE_DEPLOYED"; : > "$FAKE_LOGGER"; : > "$FAKE_TRUST"
}
lot() {
    # lot <identifiant> <statut> <version|-> : lot.json au format de
    # serde_json::to_string_pretty (src/lot.rs), avec une entrée machine
    # dont le statut, plus indenté, ne doit pas être pris pour celui du lot.
    # Version « - » : champ software absent.
    local d="$T/data/lots/$1" sw=""
    mkdir -p "$d"
    [ "$3" = - ] || sw="  \"software\": \"secmet $3\",
"
    cat > "$d/lot.json" <<JSON
{
  "version": 1,
  "id": "$1",
  "operator": "Jean",
  "reference": "",
${sw}  "opened_at": "2026-11-20 09:00:00",
  "closed_at": null,
  "status": "$2",
  "scope": "machines",
  "entries": [
    {
      "id": 1,
      "status": "finished",
      "certificates": []
    }
  ],
  "next_entry_id": 2
}
JSON
}
run() {
    PATH="$BIN:$PATH" SECMET_APP_UPDATE_DATA="$T/data" SECMET_APP_UPDATE_RUN="$T/run" sh "${1:-$SCRIPT}"
}
deployed() { sed 's#.*/app/\([^/]*\)/.*#\1#' "$FAKE_DEPLOYED" | tr '\n' ' ' | sed 's/ $//'; }
check() {
    # check <nom du cas> <attendu> <obtenu>
    if [ "$2" = "$3" ]; then
        echo "ok   $1"
    else
        echo "ÉCHEC $1 : attendu « $2 », obtenu « $3 »"
        failures=$((failures + 1))
    fi
}
has() {
    # has <nom du cas> <texte attendu dans le journal>
    check "$1" oui "$(grep -qF -- "$2" "$FAKE_LOGGER" && echo oui || echo non)"
}

# --- cas -------------------------------------------------------------------
echo "# la plus récente d'abord (0.3.10 devant 0.3.9 et 0.3.8)"
setup 0.3.7 0.3.8 0.3.10 0.3.9
run
check "déploie 0.3.10 seulement" "0.3.10" "$(deployed)"
check "vérifie 0.3.10 seulement" "verify $T/data/app/0.3.10 secmet_0.3.10.ubuntu24.04_amd64.deb" "$(cat "$FAKE_CALLS")"
check "marqueur de version" "0.3.10" "$(cat "$T/run/secmet-app-update.version")"
has "journal : déployée" "SecMet 0.3.10 déployée depuis la partition de données"
check "détail sur la clé" "1" "$(ls "$T/data/logs/system"/app-update-*.log | wc -l)"
check "détail : version" "oui" "$(grep -q 'version 0.3.10 :' "$T/data/logs/system"/app-update-*.log && echo oui || echo non)"

echo "# signature invalide sur la plus récente : repli sur la précédente"
setup 0.3.7 0.3.8 0.3.9 0.3.10
touch "$T/data/app/0.3.10/INVALID"
run
check "déploie 0.3.9" "0.3.9" "$(deployed)"
has "journal : 0.3.10 ignorée" "version 0.3.10 : signature ou empreinte invalide, ignorée"
has "journal : 0.3.9 déployée" "SecMet 0.3.9 déployée"

echo "# installation échouée sur la plus récente : repli sur la précédente"
setup 0.3.7 0.3.9 0.3.10
touch "$T/data/app/0.3.10/FAIL"
run
check "tente 0.3.10 puis 0.3.9" "0.3.10 0.3.9" "$(deployed)"
has "journal : échec puis repli" "version 0.3.10 : installation échouée"
check "marqueur = version réellement déployée" "0.3.9" "$(cat "$T/run/secmet-app-update.version")"

echo "# rien de plus récent que l'image : rien à faire"
setup 0.3.10 0.3.9 0.3.10
run
check "rien déployé" "" "$(deployed)"
has "journal : rien à faire" "version 0.3.10 pas plus récente que 0.3.10 : rien à faire"

echo "# dossier sans paquet ignoré"
setup 0.3.7 0.3.10
mkdir -p "$T/data/app/0.3.11"
run
check "déploie 0.3.10 malgré 0.3.11 vide" "0.3.10" "$(deployed)"

echo "# ordre des versions à deux chiffres (0.3.10 > 0.3.9, régression du 2026-11-06)"
setup 0.3.8 0.3.9 0.3.10
run
check "0.3.10 choisie" "0.3.10" "$(deployed)"

echo "# magasin absent : créé, rien d'autre"
setup 0.3.7
rm -rf "$T/data/app"
run
check "app/ recréé" "oui" "$([ -d "$T/data/app" ] && echo oui || echo non)"
check "rien déployé" "" "$(deployed)"

echo "# lot ouvert sous la version du magasin : déployée"
setup 0.3.9 0.3.10
lot L-20261120-AAAA open 0.3.10
run
check "déploie 0.3.10" "0.3.10" "$(deployed)"

echo "# lot ouvert sous la version installée (ancienne) : reportée"
setup 0.3.9 0.3.10
lot L-20261120-AAAA open 0.3.9
run
check "rien déployé" "" "$(deployed)"
check "aucune vérification" "" "$(cat "$FAKE_CALLS")"
has "journal : reportée" "version 0.3.10 reportée : lot L-20261120-AAAA ouvert sous secmet 0.3.9 ; déploiement au démarrage qui suivra sa clôture"
check "pas de marqueur" "non" "$([ -e "$T/run/secmet-app-update.version" ] && echo oui || echo non)"

echo "# lot ouvert sous une version intermédiaire du magasin : celle du lot"
setup 0.3.8 0.3.9 0.3.10
lot L-20261120-AAAA open 0.3.9
run
check "déploie 0.3.9" "0.3.9" "$(deployed)"
has "journal : 0.3.10 reportée" "version 0.3.10 reportée : lot L-20261120-AAAA"

echo "# lot ouvert sans version lisible (version inconnue) : reportée"
setup 0.3.9 0.3.10
lot L-20261120-AAAA open -
run
check "rien déployé" "" "$(deployed)"
has "journal : version inconnue" "version 0.3.10 reportée : lot L-20261120-AAAA ouvert sous secmet ?"

echo "# lot sans statut lisible : compté comme ouvert"
setup 0.3.9 0.3.10
lot L-20261120-AAAA open 0.3.9
sed -i '/^  "status"/d' "$T/data/lots/L-20261120-AAAA/lot.json"
run
check "rien déployé" "" "$(deployed)"

echo "# pas de lot (dossier lots/ vide ou absent) : déployée"
setup 0.3.9 0.3.10
mkdir -p "$T/data/lots"
run
check "déploie 0.3.10" "0.3.10" "$(deployed)"

echo "# lot clôturé ou abandonné sous l'ancienne version : déployée"
setup 0.3.9 0.3.10
lot L-20261120-AAAA closed 0.3.9
lot L-20261120-BBBB abandoned 0.3.9
run
check "déploie 0.3.10" "0.3.10" "$(deployed)"

echo "# plusieurs lots ouverts : tous doivent être sous la version"
setup 0.3.9 0.3.10
lot L-20261120-AAAA open 0.3.10
lot L-20261120-BBBB open 0.3.9
run
check "rien déployé" "" "$(deployed)"
has "journal : lot bloquant nommé" "lot L-20261120-BBBB ouvert sous secmet 0.3.9"

echo "# clés de version : chacune passée à secmet-release"
setup 0.3.9 0.3.10
run
check "clés du script" "$(sed -n '/^TRUST_KEYS="/,/^"/p' "$SCRIPT" | grep -E '^[0-9a-f]{64}$')" "$(cat "$FAKE_TRUST")"
check "une clé au moins" "oui" "$([ -s "$FAKE_TRUST" ] && echo oui || echo non)"
sed '/^TRUST_KEYS="/a\
0000000000000000000000000000000000000000000000000000000000000001' "$SCRIPT" > "$T/two-keys"
setup 0.3.9 0.3.10
run "$T/two-keys"
check "deux clés passées" "2" "$(wc -l < "$FAKE_TRUST" | tr -d ' ')"
check "déploie 0.3.10 avec deux clés" "0.3.10" "$(deployed)"

echo "# partition non montée : sortie silencieuse"
setup 0.3.7 0.3.10
FAKE_MOUNTED=0 run
check "rien déployé" "" "$(deployed)"
check "rien journalisé" "" "$(cat "$FAKE_LOGGER")"

echo
if [ "$failures" -eq 0 ]; then
    echo "secmet-app-update : tous les cas passent"
else
    echo "secmet-app-update : $failures échec(s)"
    exit 1
fi
