#!/usr/bin/env bash
# Scelle le système de la clé USB d'une version à publier (Annexe M §M.3) :
# signe avec la clé de version les empreintes SYSTEM.sums que
# make_key_image.sh --dist a écrites à la racine de la partition système, et
# ajoute la signature SYSTEM.sums.sig à l'image prête à écrire et au livrable
# de fabrication. À chaque démarrage, la clé vérifie l'une et l'autre
# (secmet-image-check). Appelé par le travail « publish » de package.yml
# (seul à lire la clé de version), avant la signature de SHA256SUMS ; jamais
# à la main. Image reprise d'une release précédente (« deux cadences »,
# refresh_live.sh) : rien à sceller, la partition système et sa signature
# sont reprises telles quelles.
#
# Sans montage ni privilège : la partition système est lue et complétée par
# mtools à son offset dans l'image décompressée.
#
# usage : packaging/live/seal_system.sh --dir <dossier> --version <version>
#             --key <clé de version .hex> --trust <clé publique hex>
#   <dossier> : secmet-key_<version>_x86_64.img.xz et secmet-live_<version>_x86_64.tar,
#               remplacés par leur version scellée.
# env   : SECMET_RELEASE_BIN  secmet-release (défaut target/release/, compilé au besoin)
#         SECMET_XZ_LEVEL     niveau xz de l'image recompressée (défaut 1 : le
#                             contenu est déjà compressé, voir refresh_live.sh)
# Requis : xz, sfdisk, python3, mtools, tar, cmp.
set -euo pipefail
umask 022
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE="$ROOT_DIR/packaging/live"
die() { echo "ERREUR : $*" >&2; exit 1; }
fail() { die "$@"; }
# shellcheck source=packaging/live/qemu-lib.sh
source "$LIVE/qemu-lib.sh"   # qemu_part, system_extract

DIR="" VERSION="" KEY="" TRUST=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dir) DIR="$2"; shift ;;
        --version) VERSION="$2"; shift ;;
        --key) KEY="$2"; shift ;;
        --trust) TRUST="$2"; shift ;;
        -h|--help) sed -n '2,24p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "option inconnue : $1" ;;
    esac
    shift
done
[[ -d "$DIR" && -n "$VERSION" && -f "$KEY" && -n "$TRUST" ]] || die "options --dir, --version, --key et --trust requises (voir --help)"
for t in xz sfdisk python3 mcopy tar cmp; do command -v "$t" >/dev/null || die "$t introuvable"; done
IMGXZ="$DIR/secmet-key_${VERSION}_x86_64.img.xz"
TAR="$DIR/secmet-live_${VERSION}_x86_64.tar"
NAME="secmet-live-$VERSION"
[[ -f "$IMGXZ" && -f "$TAR" ]] || die "$DIR : $(basename "$IMGXZ") et $(basename "$TAR") attendus"
RELEASE_BIN="${SECMET_RELEASE_BIN:-$ROOT_DIR/target/release/secmet-release}"
if [[ ! -x "$RELEASE_BIN" ]]; then
    ( cd "$ROOT_DIR" && cargo build -q --release -p secmet-attest --bin secmet-release )
    RELEASE_BIN="$ROOT_DIR/target/release/secmet-release"
fi
mkdir -p "$ROOT_DIR/target-live"
# Sur disque, pas dans /tmp (tmpfs : l'image décompressée y tiendrait en mémoire).
WORK="$(mktemp -d "$ROOT_DIR/target-live/seal-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "== image $(basename "$IMGXZ") décompressée"
IMG="$WORK/key.img"
xz -T0 -dc "$IMGXZ" | dd of="$IMG" bs=4M iflag=fullblock conv=sparse status=none
SYS="$WORK/sys"
off="$(system_extract "$IMG" "$SYS")"
[[ -f "$SYS/SYSTEM.sums" ]] || die "SYSTEM.sums absent de la partition système (image construite sans make_key_image.sh --dist ?)"
[[ ! -e "$SYS/SYSTEM.sums.sig" ]] || die "image déjà scellée (SYSTEM.sums.sig présent)"
echo "  partition système à l'octet $off ; $(grep -vc '^#' "$SYS/SYSTEM.sums") fichiers listés ($(sed -n '1s/^# *//p' "$SYS/SYSTEM.sums"))"

echo "== signature de SYSTEM.sums (clé de version)"
# sign-system refuse des empreintes qui ne décrivent pas exactement la
# partition extraite (fichier modifié, manquant ou en trop sous EFI/ et live/).
"$RELEASE_BIN" sign-system "$SYS" --key "$KEY" | sed 's/^/  /'
"$RELEASE_BIN" verify-system "$SYS" --trust "$TRUST" > "$WORK/verify.log" \
    || { cat "$WORK/verify.log" >&2; die "système non vérifiable avec --trust : clé de version et clé de confiance différentes ?"; }

echo "== image : SYSTEM.sums.sig ajouté à la partition système"
export MTOOLS_SKIP_CHECK=1
mcopy -i "$IMG@@$off" "$SYS/SYSTEM.sums.sig" ::SYSTEM.sums.sig
mcopy -n -i "$IMG@@$off" ::SYSTEM.sums.sig "$WORK/sig.check"
cmp -s "$WORK/sig.check" "$SYS/SYSTEM.sums.sig" || die "SYSTEM.sums.sig relu depuis l'image diffère"
# Contrôle complet sur la partition telle qu'elle sera publiée.
rm -rf "$SYS"; system_extract "$IMG" "$SYS" >/dev/null
"$RELEASE_BIN" verify-system "$SYS" --trust "$TRUST" > "$WORK/verify.log" \
    || { cat "$WORK/verify.log" >&2; die "partition système scellée invalide"; }
sed -n 's/^\(système\|fichiers\|état\) *:/  \1 :/p' "$WORK/verify.log"
rm -f "$IMGXZ"
xz -T0 "-${SECMET_XZ_LEVEL:-1}" -c "$IMG" > "$IMGXZ"
rm -f "$IMG"
echo "  $(basename "$IMGXZ") : $(stat -c %s "$IMGXZ") octets"

echo "== livrable $(basename "$TAR") : dist/live/SYSTEM.sums.sig ajouté"
tar -tf "$TAR" "$NAME/dist/live/SYSTEM.sums" >/dev/null 2>&1 || die "$(basename "$TAR") sans $NAME/dist/live/SYSTEM.sums (make_key_image.sh --dist avant bundle_live.sh)"
! tar -tf "$TAR" "$NAME/dist/live/SYSTEM.sums.sig" >/dev/null 2>&1 || die "$(basename "$TAR") déjà scellé"
tar -xOf "$TAR" "$NAME/dist/live/SYSTEM.sums" > "$WORK/tar.sums"
cmp -s "$WORK/tar.sums" "$SYS/SYSTEM.sums" || die "le livrable et l'image n'ont pas les mêmes empreintes du système"
mkdir -p "$WORK/tar/$NAME/dist/live"
cp "$SYS/SYSTEM.sums.sig" "$WORK/tar/$NAME/dist/live/"
tar -C "$WORK/tar" --owner=0 --group=0 --numeric-owner -rf "$TAR" "$NAME/dist/live/SYSTEM.sums.sig"
tar -xOf "$TAR" "$NAME/dist/live/SYSTEM.sums.sig" | cmp -s - "$SYS/SYSTEM.sums.sig" || die "SYSTEM.sums.sig relu depuis le livrable diffère"
echo "système de la clé $VERSION scellé : $(basename "$IMGXZ"), $(basename "$TAR")"
