#!/usr/bin/env bash
# Licences et offre de sources de l'image de la clé (B4, rapport 8), appelé
# par build_rootfs.sh avant le squashfs. Produit <sortie>/licenses/, copié
# dans LICENCES/ de la partition de démarrage SECMET-SYS (make_key_image.sh,
# secmet-make-key.sh) et dans le livrable .tar (bundle_live.sh), et installe
# le résumé dans le système (/usr/share/doc/secmet-live/, cité par /etc/motd).
#
#   LISEZMOI-LICENCES.txt   agrégat, emplacements, modifications, sources,
#                           avertissement de marque Debian
#   OFFRE_SOURCES.txt       offre écrite de sources (titulaire : D1)
#   packages.manifest       liste des paquets (B3)
#   GPL-2, GPL-3, LGPL-2.1  textes de /usr/share/common-licenses
#   SecMet-LICENSE.txt, SecMet-content-LICENSE.txt
#   copyright/<paquet source>.txt  fichier copyright de Debian des
#                           composants de la partition de démarrage (shim,
#                           MokManager, GRUB EFI et BIOS, noyau), de live-boot,
#                           des microcodes et des micrologiciels firmware-*
#                           (un seul fichier pour la source firmware-nonfree)
#
# usage : licences.sh <rootfs> <paquets EFI (fetch_efi.sh)> <manifeste> <sortie> <version>
# env   : SECMET_SOURCE_OFFER_HOLDER  titulaire de l'offre de sources (raison
#                                     sociale, adresse, courriel ; décision D1)
set -euo pipefail
ROOTFS=$1 EFI_PKG=$2 MANIFEST=$3 OUT=$4 VERSION=$5
SRC="${SRC:-/src}"
REPO_URL="https://github.com/hclcdev12/secmet"
HOLDER="${SECMET_SOURCE_OFFER_HOLDER:-}"
die() { echo "ERREUR (licences) : $*" >&2; exit 1; }
L="$OUT/licenses"
rm -rf "$L"; mkdir -p "$L/copyright"

for f in GPL-2 GPL-3 LGPL-2.1; do
    cp "$ROOTFS/usr/share/common-licenses/$f" "$L/$f.txt" || die "$f absent de common-licenses"
done
cp "$SRC/LICENSE" "$L/SecMet-LICENSE.txt"
cp "$SRC/content/LICENSE" "$L/SecMet-content-LICENSE.txt"
cp "$MANIFEST" "$L/packages.manifest"

# copyright <paquet binaire> <fichier> : rangé sous le nom du paquet source
# (les firmware-* partagent celui de firmware-nonfree).
listed=()
copyright() {
    local pkg=$1 file=$2 src
    [[ -f "$file" ]] || die "copyright de $pkg absent ($file)"
    src="$(awk -F'\t' -v p="$pkg" '$1 == p {print $4; exit}' "$MANIFEST")"
    [[ -n "$src" ]] || src="$pkg"
    if [[ ! -f "$L/copyright/$src.txt" ]]; then cp "$file" "$L/copyright/$src.txt"; fi
    listed+=("$pkg ($src)")
}
for pkg in shim-signed shim-helpers-amd64-signed grub-efi-amd64-signed; do
    copyright "$pkg" "$EFI_PKG/$pkg/usr/share/doc/$pkg/copyright"
done
copyright grub-pc-bin /usr/share/doc/grub-pc-bin/copyright
kpkg="$(awk -F'\t' '$1 ~ /^linux-image-[0-9].*-amd64$/ {print $1}' "$MANIFEST" | sort -V | tail -1)"
[[ -n "$kpkg" ]] || die "paquet du noyau introuvable dans le manifeste"
fw=()
while read -r p; do fw+=("$p"); done < <(awk -F'\t' '$1 ~ /^firmware-/ {print $1}' "$MANIFEST")
for pkg in "$kpkg" live-boot intel-microcode amd64-microcode "${fw[@]}"; do
    copyright "$pkg" "$ROOTFS/usr/share/doc/$pkg/copyright"
done
kver_src="$(awk -F'\t' -v p="$kpkg" '$1 == p {print $2}' "$MANIFEST")"
n_pkg="$(grep -vc '^#' "$MANIFEST")"
date_build="$(date -u +%F)"

if [[ -n "$HOLDER" ]]; then
    holder_line="$HOLDER"
else
    holder_line="[titulaire de l'offre à désigner (raison sociale, adresse postale, courriel) avant toute vente]"
fi
cat > "$L/OFFRE_SOURCES.txt" <<EOF_OFFRE
OFFRE ÉCRITE DE SOURCES — SecMet Live $VERSION (image de la clé USB SecMet)

$holder_line s'engage, pendant au moins trois ans à compter de la dernière
distribution de cette version, et aussi longtemps qu'il en assure le support
ou fournit des pièces, à remettre à toute personne qui en fait la demande une
copie complète, lisible par machine, du code source correspondant des
composants distribués sous GNU GPL, LGPL ou AGPL dans l'image de la clé
SecMet $VERSION (liste : packages.manifest, colonnes « paquet source » et
« version source », plus Built-Using et Static-Built-Using), sur un support
usuel au prix coûtant de la copie et de l'envoi, ou gratuitement par
téléchargement. Indiquez le livrable (image de la clé USB) et la version
($VERSION) dans la demande.

Le code source de SecMet est publié à $REPO_URL (tag v$VERSION).
Les sources des paquets Debian de l'image sont aussi conservées, à titre de
commodité seulement, par https://snapshot.debian.org/ (paquet source et
version du manifeste).
EOF_OFFRE

cat > "$L/LISEZMOI-LICENCES.txt" <<TXT
SecMet Live $VERSION : licences et sources

SecMet Live est le système de la clé USB SecMet, basé sur Debian 13 (trixie).
Debian est une marque déposée de Software in the Public Interest, Inc. ;
SecMet n'est ni affilié au projet Debian, ni approuvé par lui.

Cette image est un agrégat de programmes distincts, chacun sous sa propre
licence :
- SecMet (application, secmet-helper, secmet-verify, secmet-release) :
  GNU GPL version 3 ou ultérieure (SecMet-LICENSE.txt, GPL-3.txt) ; logos et
  images SecMet : SecMet-content-LICENSE.txt. Sources : $REPO_URL ;
- le système, $n_pkg paquets Debian listés dans packages.manifest
  (paquet, version, architecture, paquet source, version source) ; la
  licence de chacun est dans /usr/share/doc/<paquet>/copyright du système
  de la clé ;
- des micrologiciels non libres (section non-free-firmware de Debian) et les
  microcodes Intel et AMD, redistribués en binaire non modifié sous la
  licence de leur fabricant (copyright/firmware-nonfree.txt,
  copyright/intel-microcode.txt, copyright/amd64-microcode.txt).

Partition de démarrage SECMET-SYS (où se trouve ce dossier LICENCES/) :
  EFI/BOOT/BOOTX64.EFI   shim signé (shim-signed, BSD-2-Clause)
  EFI/BOOT/mmx64.efi     MokManager (shim-helpers-amd64-signed, BSD-2-Clause)
  EFI/BOOT/grubx64.efi   GRUB signé (grub-efi-amd64-signed, GPL-3+)
  boot/grub/, « BIOS boot »  GRUB BIOS (grub-pc-bin, GPL-3+), image core.img
                         assemblée par packaging/live/make_key_image.sh
  live/vmlinuz           noyau Linux ($kpkg $kver_src, GPL-2)
  live/initrd.img, live/filesystem.squashfs   le système (packages.manifest)
Fichiers copyright de Debian de ces composants, de live-boot et des
micrologiciels : copyright/ (un fichier par paquet source) :
$(printf '  %s\n' "${listed[@]}")

Modification de logiciels sous GPL : live-boot (GPL-3+) est modifié à la
construction de l'image, le $date_build, par packaging/live/build_rootfs.sh
du dépôt SecMet ($REPO_URL, tag v$VERSION) : fichier
/usr/lib/live/boot/9990-toram-todisk.sh (message à l'écran pendant la copie
du système en mémoire, calcul de la taille du module copié).

Secure Boot : le système démarre avec Secure Boot actif (chaîne signée de
Debian). Vous pouvez désactiver Secure Boot dans le micrologiciel du PC, ou
enrôler votre propre clé avec MokManager (EFI/BOOT/mmx64.efi, lancé par
« mokutil --import » depuis un système Linux) pour démarrer un noyau ou un
GRUB que vous avez modifié.

Sources : OFFRE_SOURCES.txt (offre écrite). GPL-2.txt, GPL-3.txt, LGPL-2.1.txt :
textes des licences.

SecMet Live est fourni SANS AUCUNE GARANTIE, dans les limites permises par la loi.
TXT

# Dans le système : résumé, offre et manifeste (cités par /etc/motd).
D="$ROOTFS/usr/share/doc/secmet-live"
mkdir -p "$D"
cp "$L/LISEZMOI-LICENCES.txt" "$L/OFFRE_SOURCES.txt" "$L/packages.manifest" "$D/"
echo "licences : $(find "$L" -type f | wc -l) fichiers, $(du -sh "$L" | cut -f1) ; copyright de ${#listed[@]} paquets"
