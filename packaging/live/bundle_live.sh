#!/usr/bin/env bash
# Assemble le livrable « clé USB » d'une version, à publier avec les paquets :
# dist/secmet-live_<version>_x86_64.tar, qui contient un dossier
# secmet-live-<version>/ au schéma attendu par secmet-make-key.sh sur une
# machine sans le dépôt (packaging/live/README.md, « Fabrication d'une clé ») :
#   dist/live/           noyau, initrd, squashfs, efi/, bios/, VERSION, et
#                        SYSTEM.sums (empreintes du système, make live-dist)
#                        si elles décrivent bien ce noyau et ce squashfs ;
#                        SYSTEM.sums.sig est ajouté à la publication
#                        (seal_system.sh)
#   packaging/live/      secmet-make-key.sh, grub.cfg
#   target/release/      secmet-release lié en statique (target-live/, make live) :
#                        tourne sur toute distribution x86_64
#   LISEZMOI.txt         la commande de fabrication
# et le manifeste des paquets de l'image, publié à part avec la release :
# dist/secmet-key_<version>_x86_64.packages.tar (un seul fichier,
# secmet-key_<version>_x86_64.packages.txt ; en .tar pour être listé dans
# SHA256SUMS signé comme les autres livrables).
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
RELEASE_BIN="${SECMET_RELEASE_BIN:-$ROOT_DIR/target-live/x86_64-unknown-linux-gnu/release/secmet-release}"
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
for f in vmlinuz initrd.img efi/shimx64.efi efi/grubx64.efi efi/mmx64.efi; do
    [[ -f "$LIVE/$f" ]] || die "$LIVE/$f absent"
done
[[ -f "$LIVE/licenses/LISEZMOI-LICENCES.txt" ]] || die "$LIVE/licenses absent (image antérieure aux licences sur la clé : construction complète, make live ou --full-live)"
[[ -f "$LIVE/packages.manifest" ]] || die "$LIVE/packages.manifest absent (image antérieure au manifeste des paquets : construction complète, make live ou --full-live)"

NAME="secmet-live-$VERSION"
OUT="$ROOT_DIR/dist/secmet-live_${VERSION}_x86_64.tar"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
D="$STAGE/$NAME"
mkdir -p "$D/dist/live" "$D/packaging/live" "$D/target/release"
cp "$LIVE/vmlinuz" "$LIVE/initrd.img" "$LIVE/filesystem.squashfs" "$LIVE/VERSION" "$LIVE/packages.manifest" "$D/dist/live/"
cp -r "$LIVE/efi" "$LIVE/licenses" "$D/dist/live/"
[[ -d "$LIVE/bios" ]] && cp -r "$LIVE/bios" "$D/dist/live/"
[[ -n "$STORE" ]] && cp -r "$STORE" "$D/dist/live/app"
# Empreintes du système (Annexe M §M.3) : livrées seulement si leurs entrées
# live/ correspondent à ce dist/live (sinon : make live-dist après make live).
if [[ -f "$LIVE/SYSTEM.sums" ]]; then
    if (cd "$LIVE" && grep -E '^[0-9a-f]{64}  live/' SYSTEM.sums | sed 's|  live/|  |' | sha256sum -c --quiet --strict >/dev/null 2>&1); then
        cp "$LIVE/SYSTEM.sums" "$D/dist/live/"
        [[ -f "$LIVE/SYSTEM.sums.sig" ]] && cp "$LIVE/SYSTEM.sums.sig" "$D/dist/live/"
    else
        echo "avertissement : $LIVE/SYSTEM.sums ne décrit pas ce système (make live-dist), non livré" >&2
    fi
fi
cp "$ROOT_DIR/packaging/live/secmet-make-key.sh" "$ROOT_DIR/packaging/live/grub.cfg" "$D/packaging/live/"
cp "$RELEASE_BIN" "$D/target/release/"
# Licence de SecMet à la racine du livrable ; celles de l'image dans LICENCES/.
cp "$ROOT_DIR/LICENSE" "$D/LICENSE"
cp -r "$LIVE/licenses" "$D/LICENCES"
store_note=""
[[ -n "$STORE" ]] && store_note="
Système de l'image : SecMet $image_version ; SecMet $VERSION est dans dist/live/app/$VERSION/
(paquet signé), copié sur la partition de données par secmet-make-key.sh et déployé
par la clé à chaque démarrage (comme après « Installer la mise à jour »)."
cat > "$D/LISEZMOI.txt" <<TXT
SecMet $VERSION : image de la clé USB ($(cat "$LIVE/VERSION"))$store_note

Système de la clé : SecMet Live, basé sur Debian 13 (trixie). Debian est une
marque déposée de Software in the Public Interest, Inc. ; SecMet n'est ni
affilié au projet Debian, ni approuvé par lui.

Fabriquer une clé (root ; EFFACE la clé, 12 Go au moins) :
    sudo $NAME/packaging/live/secmet-make-key.sh /dev/sdX
Mettre à jour la partition système d'une clé existante (données conservées) :
    sudo $NAME/packaging/live/secmet-make-key.sh /dev/sdX --system-only

Empreinte et signature : SHA256SUMS et SHA256SUMS.sig de la release
(secmet-release verify <dossier> --trust <clé publique> --file $(basename "$OUT")).
Documentation : packaging/live/README.md et docs/Annexes/Annexe_M_cle_live_lots.md du dépôt.

Licences et sources :
SecMet est sous GNU GPL version 3 ou ultérieure (LICENSE). L'image est un
agrégat de programmes distincts, chacun sous sa licence : LICENCES/ (aussi
sur la partition SECMET-SYS de chaque clé) donne le résumé
(LISEZMOI-LICENCES.txt), la liste des paquets (packages.manifest), les
fichiers copyright de la chaîne de démarrage, du noyau et des micrologiciels,
les textes GPL-2, GPL-3 et LGPL-2.1, et l'OFFRE écrite de sources
(OFFRE_SOURCES.txt). Code source de SecMet : https://github.com/hclcdev12/secmet
TXT
mkdir -p "$ROOT_DIR/dist"
tar -C "$STAGE" --owner=0 --group=0 --numeric-owner -cf "$OUT" "$NAME"
echo "$OUT ($(du -h "$OUT" | cut -f1))$([[ -n "$STORE" ]] && echo " ; système $image_version, SecMet $VERSION dans app/")"

# Manifeste publié. Image reprise d'une release précédente : même système,
# en-tête qui le dit (la version de SecMet déployée au démarrage est celle
# du magasin app/, pas celle du paquet secmet listé).
PKG_NAME="secmet-key_${VERSION}_x86_64.packages"
mkdir -p "$STAGE/manifest"
{
    if [[ -n "$STORE" ]]; then
        echo "# Image de la clé de SecMet $VERSION : système repris tel quel de l'image $image_version (release v$image_version),"
        echo "# SecMet $VERSION installé au démarrage depuis app/$VERSION de la partition de données."
    fi
    cat "$LIVE/packages.manifest"
} > "$STAGE/manifest/$PKG_NAME.txt"
tar -C "$STAGE/manifest" --owner=0 --group=0 --numeric-owner -cf "$ROOT_DIR/dist/$PKG_NAME.tar" "$PKG_NAME.txt"
echo "$ROOT_DIR/dist/$PKG_NAME.tar ($(grep -vc '^#' "$STAGE/manifest/$PKG_NAME.txt") paquets)"
