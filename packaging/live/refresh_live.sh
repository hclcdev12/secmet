#!/usr/bin/env bash
# Reprend l'image de la clé USB de la release précédente pour la version à
# publier (Annexe N, §N.7, « deux cadences » ; décision : live-mode.sh).
# Le système de la clé (squashfs, noyau, initrd, GRUB) n'a pas changé :
# seule la partition de données est refaite, avec le nouveau paquet dans
# son magasin app/<version>/ signé par la clé de version ; au démarrage,
# secmet-app-update le vérifie et le déploie (Annexe N, §N.6), comme après
# « Installer la mise à jour ». Sans montage ni privilège : la partition est
# produite en fichier (mke2fs -d, propriétaires posés par debugfs) et écrite
# dans l'image à son offset.
#
# Produit :
#   <sortie>/secmet-key_<version>_x86_64.img.xz   image prête à écrire
#   <sortie>/secmet-live_<version>_x86_64.tar      livrable de fabrication :
#       dist/live/ de la release précédente + dist/live/app/<version>/, que
#       secmet-make-key.sh copie sur la partition de données (bundle_live.sh)
#
# usage : packaging/live/refresh_live.sh --previous <dossier> --deb <secmet_<version>…ubuntu24.04_amd64.deb>
#             --key <clé de version .hex> --trust <clé publique hex>
#             [--version <version>] [--out <dossier>]
#   <dossier> : secmet-key_*.img.xz, secmet-live_*.tar, SHA256SUMS, SHA256SUMS.sig
#               et latest.json de la release précédente (gh release download),
#               vérifiés avec --trust avant toute réutilisation.
# env   : SECMET_RELEASE_BIN   secmet-release (défaut target/release/, compilé au besoin ;
#                              jamais celui du livrable téléchargé, qui est ce qu'on vérifie)
#         SECMET_RELEASES_URL  base des téléchargements (sums_url du magasin ;
#                              défaut https://github.com/hclcdev12/secmet/releases/download)
#         SECMET_XZ_LEVEL      niveau xz (défaut 1 : le contenu est déjà compressé,
#                              seuls les zéros disparaissent, le niveau ne change pas la taille)
# Requis : xz, sfdisk, e2fsprogs (mke2fs, dumpe2fs, debugfs, e2fsck), tar, python3.
set -euo pipefail
umask 022   # modes de la partition de données et des sorties indépendants de l'appelant
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE="$ROOT_DIR/packaging/live"
BASE_URL="${SECMET_RELEASES_URL:-https://github.com/hclcdev12/secmet/releases/download}"
die() { echo "ERREUR : $*" >&2; exit 1; }
fail() { die "$@"; }
# shellcheck source=packaging/live/qemu-lib.sh
source "$LIVE/qemu-lib.sh"   # qemu_part

PREV="" DEB="" KEY="" TRUST="" VERSION="" OUT="$ROOT_DIR/dist"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --previous) PREV="$2"; shift ;;
        --deb) DEB="$2"; shift ;;
        --key) KEY="$2"; shift ;;
        --trust) TRUST="$2"; shift ;;
        --version) VERSION="$2"; shift ;;
        --out) OUT="$2"; shift ;;
        -h|--help) sed -n '2,32p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "option inconnue : $1" ;;
    esac
    shift
done
[[ -n "$VERSION" ]] || VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
[[ -d "$PREV" && -f "$DEB" && -f "$KEY" && -n "$TRUST" ]] || die "options --previous, --deb, --key et --trust requises (voir --help)"
for t in xz sfdisk mke2fs dumpe2fs debugfs e2fsck tar python3 cmp; do command -v "$t" >/dev/null || die "$t introuvable"; done
IMGXZ="$(ls "$PREV"/secmet-key_*_x86_64.img.xz 2>/dev/null | head -1 || true)"
TAR="$(ls "$PREV"/secmet-live_*_x86_64.tar 2>/dev/null | head -1 || true)"
[[ -n "$IMGXZ" && -n "$TAR" && -f "$PREV/SHA256SUMS" && -f "$PREV/SHA256SUMS.sig" && -f "$PREV/latest.json" ]] \
    || die "$PREV : image, livrable, SHA256SUMS(.sig) et latest.json de la release précédente attendus"
PREV_VERSION="$(basename "$TAR" | sed -E 's/^secmet-live_(.+)_x86_64\.tar$/\1/')"
[[ "$(printf '%s\n%s\n' "$PREV_VERSION" "$VERSION" | sort -V | tail -1)" == "$VERSION" && "$PREV_VERSION" != "$VERSION" ]] \
    || die "la version à publier ($VERSION) doit être plus récente que celle de l'image reprise ($PREV_VERSION)"
case "$(basename "$DEB")" in
    "secmet_${VERSION}~ubuntu24.04_amd64.deb"|"secmet_${VERSION}.ubuntu24.04_amd64.deb") ;;
    *) die "paquet attendu : secmet_${VERSION}~ubuntu24.04_amd64.deb (celui de l'image), reçu $(basename "$DEB")" ;;
esac
RELEASE_BIN="${SECMET_RELEASE_BIN:-$ROOT_DIR/target/release/secmet-release}"
if [[ ! -x "$RELEASE_BIN" ]]; then
    ( cd "$ROOT_DIR" && cargo build -q --release -p secmet-attest --bin secmet-release )
    RELEASE_BIN="$ROOT_DIR/target/release/secmet-release"
fi
mkdir -p "$OUT" "$ROOT_DIR/target-live"
# Sur disque, pas dans /tmp (tmpfs sur Fedora : l'image décompressée y
# tiendrait en mémoire).
WORK="$(mktemp -d "$ROOT_DIR/target-live/refresh-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "== release précédente $PREV_VERSION : signatures et empreintes"
"$RELEASE_BIN" verify "$PREV" --trust "$TRUST" --file "$(basename "$IMGXZ")" >/dev/null || die "$(basename "$IMGXZ") : vérification échouée"
"$RELEASE_BIN" verify "$PREV" --trust "$TRUST" --file "$(basename "$TAR")" >/dev/null || die "$(basename "$TAR") : vérification échouée"
echo "  $(basename "$IMGXZ") et $(basename "$TAR") : valides"

echo "== livrable précédent"
tar -C "$WORK" -xf "$TAR"
PREV_DIR="$WORK/secmet-live-$PREV_VERSION"
[[ -f "$PREV_DIR/dist/live/filesystem.squashfs" && -f "$PREV_DIR/dist/live/VERSION" ]] || die "livrable sans dist/live complet"
echo "  système : $(cat "$PREV_DIR/dist/live/VERSION")"

echo "== magasin app/$VERSION (signé par la clé de version)"
STORE="$WORK/app/$VERSION"
mkdir -p "$STORE"
cp "$DEB" "$STORE/secmet_${VERSION}.ubuntu24.04_amd64.deb"
"$RELEASE_BIN" sign "$STORE" --key "$KEY" --version "$VERSION" --date "$(date -u +%F)" \
    --sums-url "$BASE_URL/v$VERSION/SHA256SUMS" >/dev/null
"$RELEASE_BIN" verify "$STORE" --trust "$TRUST" --file "secmet_${VERSION}.ubuntu24.04_amd64.deb" >/dev/null \
    || die "le magasin signé n'est pas vérifiable avec --trust : clé de version et clé de confiance différentes ?"
echo "  $(ls "$STORE" | tr '\n' ' ')"

echo "== image : $(basename "$IMGXZ") décompressée"
IMG="$WORK/key.img"
# fullblock : sans lui, dd traite les lectures partielles du tube comme
# des blocs (des milliers de petites écritures : 5 min sur GitHub au lieu de 20 s).
xz -T0 -dc "$IMGXZ" | dd of="$IMG" bs=4M iflag=fullblock conv=sparse status=none
read -r start size < <(qemu_part "$IMG" SECMET-DATA)
# Options ext4 de la partition d origine (celles du mke2fs d Ubuntu 24.04
# dans le conteneur de construction), reprises à l identique : le format ne
# dépend pas de la machine qui refait la partition.
dd if="$IMG" of="$WORK/sb.img" bs=512 skip="$start" count=64 status=none
# dumpe2fs se plaint du fichier tronqué (code 1) mais lit le superbloc.
features="$(dumpe2fs -h "$WORK/sb.img" 2>/dev/null | sed -n 's/^Filesystem features: *//p' | tr -s ' ' ',' || true)"
inode_size="$(dumpe2fs -h "$WORK/sb.img" 2>/dev/null | sed -n 's/^Inode size: *//p' || true)"
[[ -n "$features" && -n "$inode_size" ]] || die "superbloc ext4 de la partition de données illisible"
echo "  partition de données : secteur $start, $((size / 2048)) Mio ; ext4 $features"

echo "== partition de données (ext4, mke2fs -d)"
# Même schéma que make_key_image.sh et secmet-make-key.sh : racine root
# 0755, dossiers de l'opérateur (uid 1000) en 0700, vault root 0700, pas
# de clé de signature (créée au premier démarrage par secmet-first-boot).
# Sans privilège, mke2fs -d reprend les propriétaires du dossier source :
# ils sont posés ensuite dans l'image par debugfs (set_inode_field).
D="$WORK/data"
for d in config logs lots archive app vault; do mkdir -p "$D/$d"; chmod 700 "$D/$d"; done
printf '{\n  "language": "fr",\n  "check_updates": true\n}\n' > "$D/config/settings.json"
cp -r "$WORK/app/." "$D/app/"
chmod 755 "$D"
P2="$WORK/p2.img"
truncate -s $((size * 512)) "$P2"
mke2fs -q -t ext4 -L SECMET-DATA -I "$inode_size" -O "none,$features" \
    -E lazy_itable_init=0,lazy_journal_init=0,root_owner=0:0 -d "$D" "$P2"
new_features="$(dumpe2fs -h "$P2" 2>/dev/null | sed -n 's/^Filesystem features: *//p' | tr -s ' ' ',')"
[[ "$new_features" == "$features" ]] || die "options ext4 obtenues ($new_features) différentes de l'origine ($features)"
{
    echo "sif / uid 0"; echo "sif / gid 0"
    (cd "$D" && find . -mindepth 1 | sed 's|^\./||' | sort) | while read -r p; do
        case "$p" in vault|vault/*) u=0 ;; *) u=1000 ;; esac
        echo "sif /$p uid $u"; echo "sif /$p gid $u"
    done
} > "$WORK/owners.cmd"
debugfs -w -f "$WORK/owners.cmd" "$P2" >/dev/null 2>&1
e2fsck -fn "$P2" >/dev/null 2>&1 || die "partition de données incohérente après debugfs"
# debugfs ls -l : inode, mode (octal), (type), uid, gid, taille, date, nom.
listing="$(debugfs -R "ls -l /" "$P2" 2>/dev/null)"
grep -Eq "40755 \(2\) +0 +0 .* \.$" <<<"$listing" || die "racine : propriétaire ou mode inattendu"
grep -Eq "40700 \(2\) +1000 +1000 .* app$" <<<"$listing" || die "app/ : propriétaire ou mode inattendu"
grep -Eq "40700 \(2\) +0 +0 .* vault$" <<<"$listing" || die "vault/ : propriétaire ou mode inattendu"
debugfs -R "ls -l /app/$VERSION" "$P2" 2>/dev/null | grep -Eq "100644 \(1\) +1000 +1000 .*SHA256SUMS\.sig$" || die "app/$VERSION : magasin incomplet"
# La zone est d abord vidée : avec conv=sparse seul, les blocs nuls de la
# nouvelle partition ne seraient pas écrits et l ancienne partition
# transparaîtrait dessous (système de fichiers incohérent, constaté le
# 2026-11-09 : la clé démarrait sans sa partition de données).
if ! fallocate -p -o $((start * 512)) -l $((size * 512)) "$IMG" 2>/dev/null; then
    dd if=/dev/zero of="$IMG" bs=1M seek=$((start / 2048)) count=$((size / 2048)) conv=notrunc status=none
fi
dd if="$P2" of="$IMG" bs=512 seek="$start" conv=notrunc,sparse status=none
dd if="$IMG" of="$WORK/check.img" bs=4M iflag=skip_bytes,count_bytes skip=$((start * 512)) count=$((size * 512)) conv=sparse status=none
e2fsck -fn "$WORK/check.img" >/dev/null 2>&1 || die "partition de données incohérente une fois écrite dans l'image"
cmp -s "$WORK/check.img" "$P2" || die "la partition relue depuis l'image diffère de celle produite"
rm -f "$WORK/check.img"
echo "  app/$VERSION, config/settings.json ; propriétaires et modes vérifiés ; partition relue depuis l'image : cohérente"

OUT_IMG="$OUT/secmet-key_${VERSION}_x86_64.img.xz"
echo "== compression xz → $(basename "$OUT_IMG")"
rm -f "$OUT_IMG"
xz -T0 "-${SECMET_XZ_LEVEL:-1}" -c "$IMG" > "$OUT_IMG"
rm -f "$IMG" "$P2"
echo "  $(stat -c %s "$OUT_IMG") octets"

echo "== livrable secmet-live_${VERSION}_x86_64.tar (système de $PREV_VERSION + app/$VERSION)"
mkdir -p "$PREV_DIR/dist/live/app"
cp -r "$WORK/app/." "$PREV_DIR/dist/live/app/"
SECMET_LIVE_DIR="$PREV_DIR/dist/live" SECMET_RELEASE_BIN="$PREV_DIR/target/release/secmet-release" SECMET_LIVE_VERSION="$VERSION" \
    "$LIVE/bundle_live.sh" | sed 's/^/  /'
[[ "$OUT" -ef "$ROOT_DIR/dist" ]] || mv -f "$ROOT_DIR/dist/secmet-live_${VERSION}_x86_64.tar" "$OUT/"
echo "image de la clé reprise de $PREV_VERSION pour $VERSION : $(basename "$OUT_IMG"), secmet-live_${VERSION}_x86_64.tar"
