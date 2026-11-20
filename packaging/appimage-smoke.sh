#!/usr/bin/env bash
# Fumée de l'AppImage SecMet extraite (--appimage-extract, sans FUSE) :
# binaires, outils annexes secmet-verify et secmet-release et leur
# aiguillage par AppRun, copyright de SecMet et de chaque bibliothèque
# embarquée (liste usr/share/doc/secmet/bundled-libraries.txt écrite par
# bundle-tools.sh --copyrights). Appelé par package.yml (travail appimage)
# et par release.sh.
#
# usage : packaging/appimage-smoke.sh <squashfs-root>
set -euo pipefail

R="$(cd "${1:?usage: appimage-smoke.sh <squashfs-root>}" && pwd)"
fail() { echo "fumée AppImage : $*" >&2; exit 1; }

for b in secmet secmet-helper secmet-verify secmet-release; do
    [[ -x "$R/usr/bin/$b" ]] || fail "usr/bin/$b absent"
done

# --help : usage sur stdout, code 0. Sortie capturée avant grep (pipefail).
check_usage() { # <outil attendu> <commande…>
    local want="$1" out
    shift
    out="$("$@")" || return 1
    [[ "$(head -1 <<<"$out")" == "usage: $want "* ]]
}
check_usage secmet-verify "$R/usr/bin/secmet-verify" --help || fail "secmet-verify --help"
check_usage secmet-release "$R/usr/bin/secmet-release" --help || fail "secmet-release --help"
check_usage secmet-verify "$R/AppRun" verify --help || fail "AppRun verify --help"
check_usage secmet-release "$R/AppRun" release --help || fail "AppRun release --help"
empty="$(mktemp -d)"
if "$R/usr/bin/secmet-release" verify "$empty" 2>/dev/null; then fail "secmet-release verify accepte un dossier vide"; fi
rmdir "$empty"

DOC="$R/usr/share/doc/secmet"
[[ -s "$DOC/copyright" ]] || fail "usr/share/doc/secmet/copyright absent"
LIST="$DOC/bundled-libraries.txt"
[[ -s "$LIST" ]] || fail "usr/share/doc/secmet/bundled-libraries.txt absent"
listed="$(grep -v '^#' "$LIST" | cut -d' ' -f1 | sort -u)"
present="$(find "$R/usr/lib" -type f \( -name '*.so' -o -name '*.so.*' -o -path "$R/usr/lib/secmet/bin/*" \) \
    | sed "s|^$R/||" | sort -u)"
unlisted="$(comm -13 <(echo "$listed") <(echo "$present"))"
[[ -z "$unlisted" ]] || fail "fichiers embarqués sans paquet d'origine : $(echo "$unlisted" | tr '\n' ' ')"
n=0
while read -r pkg; do
    [[ -s "$R/usr/share/doc/$pkg/copyright" ]] || fail "usr/share/doc/$pkg/copyright absent"
    n=$((n + 1))
done < <(grep -v '^#' "$LIST" | cut -d' ' -f2 | sort -u)
echo "AppImage : 4 binaires, outils annexes aiguillés par AppRun, copyright de $n paquet(s) embarqué(s)"
