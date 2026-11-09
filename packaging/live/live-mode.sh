#!/usr/bin/env bash
# Cadence de l'image de la clé USB pour la version à publier (Annexe N,
# §N.7, « deux cadences ») : si aucun fichier qui entre dans le système de
# la clé (squashfs, noyau, initrd, GRUB, schéma de l'image) n'a changé
# depuis la release précédente, l'image de celle-ci est reprise telle
# quelle et seule sa partition de données est refaite, avec le nouveau
# paquet dans app/<version>/ (packaging/live/refresh_live.sh, quelques
# minutes) ; sinon, construction complète (build_live.sh, conteneur
# privilégié, 14 min sur GitHub).
#
# La release précédente a forcément été construite (ou reprise) depuis un
# arbre où ces fichiers étaient dans l'état de son tag : comparer les deux
# tags suffit, aucun registre n'est à tenir.
#
# usage : packaging/live/live-mode.sh [<version>]   (défaut : Cargo.toml)
#         packaging/live/live-mode.sh --inputs       (fichiers du système, un par ligne)
# env   : SECMET_LIVE_FULL=1     impose la construction complète
#         SECMET_RELEASES_REPO   dépôt des releases (défaut hclcdev12/secmet)
#         GH_TOKEN               jeton pour gh (lecture publique sinon)
# Sortie : une ligne « mode=reuse previous=vX.Y.Z » ou « mode=full reason=… ».
# Requis : gh, les tags du dépôt (git fetch --tags).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO="${SECMET_RELEASES_REPO:-hclcdev12/secmet}"
# Tout ce qui finit dans le système de la clé ou dans la forme de l'image.
# Le paquet SecMet lui-même n'en fait pas partie : la nouvelle version est
# déployée au démarrage depuis la partition de données (secmet-app-update).
# build_deb.sh et stage-tree.sh y sont parce qu'ils fixent les dépendances
# et l'arborescence installées dans le système (un paquet déployé par
# dessus n'apporte pas de nouvelle bibliothèque).
INPUTS=(
    packaging/live/build_live.sh
    packaging/live/build_rootfs.sh
    packaging/live/live.Containerfile
    packaging/live/grub.cfg
    packaging/live/make_key_image.sh
    packaging/live/overlay
    packaging/build_deb.sh
    packaging/stage-tree.sh
)
if [[ "${1:-}" == "--inputs" ]]; then printf '%s\n' "${INPUTS[@]}"; exit 0; fi
cd "$ROOT_DIR"
VERSION="${1:-$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')}"
TAG="v$VERSION"
full() { echo "mode=full reason=$*"; exit 0; }

[[ "${SECMET_LIVE_FULL:-0}" == 1 ]] && full "construction complète demandée (SECMET_LIVE_FULL)"
command -v gh >/dev/null || full "gh absent"
tags="$(gh release list --repo "$REPO" --limit 100 --exclude-drafts --exclude-pre-releases --json tagName --jq '.[].tagName' 2>/dev/null \
    | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true)"
[[ -n "$tags" ]] || full "aucune release sur $REPO"
# Le plus grand tag publié strictement inférieur à la version à publier.
prev="$(printf '%s\n%s\n' "$tags" "$TAG" | sort -uV | grep -B1 -x -- "$TAG" | head -1)"
[[ -n "$prev" && "$prev" != "$TAG" ]] || full "aucune release avant $TAG"
assets="$(gh release view "$prev" --repo "$REPO" --json assets --jq '.assets[].name' 2>/dev/null || true)"
pv="${prev#v}"
for a in "secmet-key_${pv}_x86_64.img.xz" "secmet-live_${pv}_x86_64.tar" SHA256SUMS SHA256SUMS.sig latest.json; do
    grep -qx -- "$a" <<<"$assets" || full "release $prev sans $a"
done
git rev-parse -q --verify "$prev^{commit}" >/dev/null 2>&1 || full "tag $prev absent du dépôt local (git fetch --tags)"
changed="$(git diff --name-only "$prev" HEAD -- "${INPUTS[@]}" | tr '\n' ' ')"
[[ -z "$changed" ]] || full "système modifié depuis $prev : ${changed% }"
echo "mode=reuse previous=$prev"
