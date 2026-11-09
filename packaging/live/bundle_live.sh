#!/usr/bin/env bash
# Assemble le livrable « clé USB » d'une version, à publier avec les paquets :
# dist/secmet-live_<version>_x86_64.tar, qui contient un dossier
# secmet-live-<version>/ au schéma attendu par secmet-make-key.sh sur une
# machine sans le dépôt (packaging/live/README.md, « Fabrication d'une clé ») :
#   dist/live/           noyau, initrd, squashfs, efi/, bios/, VERSION
#   packaging/live/      secmet-make-key.sh, grub.cfg
#   target/release/      secmet-release compilé pour Ubuntu 24.04 (target-live/)
#   LISEZMOI.txt         la commande de fabrication
# Refuse une image qui n'est pas de la version de Cargo.toml (make live d'abord),
# sauf si dist/live/app/<version>/ porte le paquet de cette version signé
# (image reprise d'une release précédente, refresh_live.sh) : le magasin est
# alors livré, secmet-make-key.sh le copie sur la partition de données et
# secmet-app-update déploie la version au démarrage.
#
# usage : make live-bundle    (après make live)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE="${SECMET_LIVE_DIR:-$ROOT_DIR/dist/live}"
RELEASE_BIN="${SECMET_RELEASE_BIN:-$ROOT_DIR/target-live/release/secmet-release}"
die() { echo "ERREUR : $*" >&2; exit 1; }

VERSION="${SECMET_LIVE_VERSION:-$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
[[ -f "$LIVE/filesystem.squashfs" && -f "$LIVE/VERSION" ]] || die "image absente dans $LIVE : make live"
image_version="$(sed -E 's/^secmet ([^~ ]+).*/\1/' "$LIVE/VERSION")"
STORE=""
if [[ "$image_version" != "$VERSION" ]]; then
    [[ -f "$LIVE/app/$VERSION/SHA256SUMS.sig" && -n "$(ls "$LIVE/app/$VERSION"/secmet_*.deb 2>/dev/null)" ]] \
        || die "image en $image_version, Cargo.toml en $VERSION : make live"
    [[ "$(printf '%s\n%s\n' "$image_version" "$VERSION" | sort -V | tail -1)" == "$VERSION" ]] \
        || die "image en $image_version, plus récente que Cargo.toml ($VERSION)"
    STORE="$LIVE/app"
fi
[[ -x "$RELEASE_BIN" ]] || die "secmet-release introuvable : $RELEASE_BIN (produit par make live)"
for f in vmlinuz initrd.img efi/shimx64.efi efi/grubx64.efi; do
    [[ -f "$LIVE/$f" ]] || die "$LIVE/$f absent"
done

NAME="secmet-live-$VERSION"
OUT="$ROOT_DIR/dist/secmet-live_${VERSION}_x86_64.tar"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
D="$STAGE/$NAME"
mkdir -p "$D/dist/live" "$D/packaging/live" "$D/target/release"
cp "$LIVE/vmlinuz" "$LIVE/initrd.img" "$LIVE/filesystem.squashfs" "$LIVE/VERSION" "$D/dist/live/"
cp -r "$LIVE/efi" "$D/dist/live/"
[[ -d "$LIVE/bios" ]] && cp -r "$LIVE/bios" "$D/dist/live/"
[[ -n "$STORE" ]] && cp -r "$STORE" "$D/dist/live/app"
cp "$ROOT_DIR/packaging/live/secmet-make-key.sh" "$ROOT_DIR/packaging/live/grub.cfg" "$D/packaging/live/"
cp "$RELEASE_BIN" "$D/target/release/"
store_note=""
[[ -n "$STORE" ]] && store_note="
Système de l'image : SecMet $image_version ; SecMet $VERSION est dans dist/live/app/$VERSION/
(paquet signé), copié sur la partition de données par secmet-make-key.sh et déployé
par la clé à chaque démarrage (comme après « Installer la mise à jour »)."
cat > "$D/LISEZMOI.txt" <<TXT
SecMet $VERSION : image de la clé USB ($(cat "$LIVE/VERSION"))$store_note

Fabriquer une clé (root ; EFFACE la clé, 12 Go au moins) :
    sudo $NAME/packaging/live/secmet-make-key.sh /dev/sdX
Mettre à jour la partition système d'une clé existante (données conservées) :
    sudo $NAME/packaging/live/secmet-make-key.sh /dev/sdX --system-only

Empreinte et signature : SHA256SUMS et SHA256SUMS.sig de la release
(secmet-release verify <dossier> --trust <clé publique> --file $(basename "$OUT")).
Documentation : packaging/live/README.md et docs/Annexes/Annexe_M_cle_live_lots.md du dépôt.
TXT
mkdir -p "$ROOT_DIR/dist"
tar -C "$STAGE" --owner=0 --group=0 --numeric-owner -cf "$OUT" "$NAME"
echo "$OUT ($(du -h "$OUT" | cut -f1))$([[ -n "$STORE" ]] && echo " ; système $image_version, SecMet $VERSION dans app/")"
