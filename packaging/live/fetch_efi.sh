#!/usr/bin/env bash
# Récupère la chaîne de démarrage UEFI épinglée de la clé (shim, MokManager,
# GRUB signés par Debian) et la contrôle contre packaging/live/efi-pin.sha256 :
# version exacte de chaque paquet, empreinte du .deb, empreinte de chaque
# binaire EFI, émetteurs des signatures. Appelé par build_rootfs.sh dans le
# conteneur de construction (Debian 13).
#
# Chaque .deb est pris sur le miroir par sa version exacte
# (apt-get download paquet=version) ; si cette version a quitté le miroir
# (remplacée par une mise à jour), sur snapshot.debian.org, qui garde toutes
# les versions publiées (API mr/binary, puis /file/<sha1>).
#
# usage : packaging/live/fetch_efi.sh <dossier>
#   <dossier>/<paquet>/…        contenu de chaque paquet (dpkg-deb -x)
#   <dossier>/versions.tsv      paquet, version, architecture, paquet source,
#                               version source, Built-Using, sha256 du .deb
#                               (manifeste des paquets de l'image)
# env   : SECMET_EFI_UPDATE=1   prend les versions courantes du miroir au lieu
#                               des versions épinglées, écrit les lignes à
#                               recopier dans <dossier>/efi-pin.sha256.new ;
#                               les émetteurs restent exigés
#         SECMET_SNAPSHOT_URL   défaut https://snapshot.debian.org
#         SECMET_EFI_SNAPSHOT=1 passe directement par snapshot.debian.org
#                               (contrôle du repli)
set -euo pipefail
LIVE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PIN="$LIVE_DIR/efi-pin.sha256"
DEST="${1:?usage : fetch_efi.sh <dossier>}"
SNAPSHOT="${SECMET_SNAPSHOT_URL:-https://snapshot.debian.org}"
UPDATE="${SECMET_EFI_UPDATE:-0}"
die() { echo "ERREUR (chaîne EFI) : $*" >&2; exit 1; }
for t in apt-get dpkg-deb sha256sum curl sbverify; do command -v "$t" >/dev/null || die "$t introuvable"; done
[[ -f "$PIN" ]] || die "$PIN absent"

rm -rf "$DEST"; mkdir -p "$DEST/debs"
chmod 0777 "$DEST/debs"   # apt-get download écrit sous l'utilisateur _apt
apt-get update -qq >/dev/null 2>&1 || die "apt-get update"

fetch() { # <paquet> <version> → chemin du .deb
    local pkg=$1 ver=$2 f sha1
    if [[ "$ver" == "-" ]]; then
        ( cd "$DEST/debs" && apt-get download -qq "$pkg" ) >/dev/null 2>&1 || die "$pkg : absent du miroir"
    elif [[ "${SECMET_EFI_SNAPSHOT:-0}" != 1 ]] && ( cd "$DEST/debs" && apt-get download -qq "$pkg=$ver" ) >/dev/null 2>&1; then
        echo "  $pkg $ver : miroir Debian" >&2
    else
        echo "  $pkg $ver : absent du miroir, snapshot.debian.org" >&2
        sha1="$(curl -fsS --retry 3 "$SNAPSHOT/mr/binary/$pkg/$ver/binfiles" \
            | tr -d ' \n' | grep -o '"architecture":"amd64","hash":"[0-9a-f]\{40\}"' | grep -o '[0-9a-f]\{40\}' | head -1)" || true
        [[ -n "$sha1" ]] || die "$pkg $ver introuvable sur $SNAPSHOT"
        curl -fsS --retry 3 -o "$DEST/debs/${pkg}_snapshot_amd64.deb" "$SNAPSHOT/file/$sha1" || die "$pkg $ver : téléchargement snapshot"
        [[ "$(sha1sum "$DEST/debs/${pkg}_snapshot_amd64.deb" | cut -d' ' -f1)" == "$sha1" ]] || die "$pkg $ver : SHA-1 snapshot différent"
    fi
    f="$(ls -1t "$DEST/debs/${pkg}_"*.deb | head -1)"
    [[ "$(dpkg-deb -f "$f" Package)" == "$pkg" ]] || die "$f n'est pas $pkg"
    echo "$f"
}

: > "$DEST/versions.tsv"
new_pin=()
fail=0
while read -r kind pkg ver sha; do
    [[ "$kind" == deb ]] || continue
    deb="$(fetch "$pkg" "$([[ "$UPDATE" == 1 ]] && echo - || echo "$ver")")"
    got_ver="$(dpkg-deb -f "$deb" Version)"
    got_sha="$(sha256sum "$deb" | cut -d' ' -f1)"
    if [[ "$UPDATE" == 1 ]]; then
        new_pin+=("$(printf 'deb    %-26s %-30s %s' "$pkg" "$got_ver" "$got_sha")")
        [[ "$got_ver" == "$ver" ]] || echo "  $pkg : $ver → $got_ver (SECMET_EFI_UPDATE)" >&2
    else
        [[ "$got_ver" == "$ver" ]] || die "$pkg : version $got_ver, épinglée $ver"
        [[ "$got_sha" == "$sha" ]] || { echo "ERREUR (chaîne EFI) : $pkg $ver : sha256 du .deb $got_sha, épinglé $sha" >&2; fail=1; }
    fi
    mkdir -p "$DEST/$pkg"
    dpkg-deb -x "$deb" "$DEST/$pkg"
    printf '%s\t%s\n' "$(dpkg-deb --show --showformat '${Package}\t${Version}\t${Architecture}\t${source:Package}\t${source:Version}\t${Built-Using}' "$deb")" \
        "$got_sha" >> "$DEST/versions.tsv"
done < <(grep -v '^\s*#' "$PIN")

while read -r kind pkg path sha; do
    [[ "$kind" == efi ]] || continue
    f="$DEST/$pkg/$path"
    [[ -f "$f" ]] || die "$pkg : $path absent du paquet"
    got="$(sha256sum "$f" | cut -d' ' -f1)"
    if [[ "$UPDATE" == 1 ]]; then
        new_pin+=("$(printf 'efi    %-26s %-50s %s' "$pkg" "$path" "$got")")
    elif [[ "$got" != "$sha" ]]; then
        echo "ERREUR (chaîne EFI) : $path : sha256 $got, épinglé $sha" >&2; fail=1
    else
        echo "  $(basename "$path") : $got" >&2
    fi
done < <(grep -v '^\s*#' "$PIN")

# Émetteurs : chaque ligne « issuer » doit figurer dans sbverify --list.
while read -r kind pkg path issuer; do
    [[ "$kind" == issuer ]] || continue
    if sbverify --list "$DEST/$pkg/$path" 2>/dev/null | grep -q "issuer: .*CN=$issuer\$"; then
        echo "  $(basename "$path") : signé par une autorité « $issuer »" >&2
    else
        echo "ERREUR (chaîne EFI) : $path n'a pas de signature émise par « $issuer »" >&2
        sbverify --list "$DEST/$pkg/$path" 2>&1 | grep -E 'issuer|subject' >&2 || true
        fail=1
    fi
done < <(grep -v '^\s*#' "$PIN")
[[ "$fail" == 0 ]] || die "la chaîne de démarrage ne correspond pas à $PIN (SECMET_EFI_UPDATE=1 pour en changer, voir l'en-tête du fichier)"

if [[ "$UPDATE" == 1 ]]; then
    { grep -E '^\s*#' "$PIN"; printf '%s\n' "${new_pin[@]}"; grep '^issuer' "$PIN"; } > "$DEST/efi-pin.sha256.new"
    echo "SECMET_EFI_UPDATE=1 : versions courantes prises ; lignes à recopier dans packaging/live/efi-pin.sha256 :" >&2
    printf '%s\n' "${new_pin[@]}" >&2
fi
rm -rf "$DEST/debs"
