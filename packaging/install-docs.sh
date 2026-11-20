#!/usr/bin/env bash
# Documentation installée de SecMet, la même pour le .deb, le .rpm
# (stage-tree.sh) et l'AppImage (build_appimage.sh) : une seule liste, un
# seul nom par fichier (décision D18, annexe J §11).
#
# Le dossier reproduit docs/ (Fabriquer_sa_cle.md, PDF d'exemple, Annexes/)
# pour que les liens relatifs entre documents restent justes ; s'y ajoutent
# les textes de licence sous un nom fixe (copyright = packaging/debian-copyright
# partout) et les copies de commodité à la racine (USER_GUIDE.md,
# GUIDE_DEPLOIEMENT_BANC.md, THIRD_PARTY_LICENSES.md). Chaque lien relatif
# d'un .md installé est réécrit : vers le fichier installé s'il l'est, sinon
# vers GitHub au tag de la version (<dépôt>/blob/v<version>/<chemin>, dépôt
# pris dans crates/secmet-attest/src/legal.rs).
#
# Annexes installées : l'index docs/Annexes/README.md et les annexes qu'il
# cite, et seulement si git les connaît (arbre de travail git) ; un fichier
# de docs/Annexes/ absent de l'index ou non versionné n'est jamais embarqué
# (cas d'une annexe interne laissée dans le dossier). Sans git (arbre exporté
# sans .git : release.sh, construction de la clé), l'index seul fait foi.
#
# Le résultat est contrôlé par check-installed-docs.sh (échec bloquant).
#
# usage : install-docs.sh <doc_dir> <version> [--appimage]
#   --appimage : avis tiers complétés par le runtime AppImage
set -euo pipefail

DOC="${1:?usage: install-docs.sh <doc_dir> <version> [--appimage]}"
VERSION="${2:?usage: install-docs.sh <doc_dir> <version> [--appimage]}"
APPIMAGE=0
[[ "${3:-}" == "--appimage" ]] && APPIMAGE=1

PKG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$PKG_DIR/.." && pwd)"

die() { echo "install-docs : $*" >&2; exit 1; }

REPO_URL="$(sed -n 's/^pub const SOURCE_REPOSITORY: &str = "\(https:[^"]*\)";$/\1/p' \
    "$ROOT_DIR/crates/secmet-attest/src/legal.rs")"
[[ -n "$REPO_URL" ]] || die "SOURCE_REPOSITORY introuvable dans crates/secmet-attest/src/legal.rs"
BLOB_URL="$REPO_URL/blob/v$VERSION"

# git disponible sur un arbre de travail (safe.directory : propriétaire du
# dépôt différent de l'utilisateur d'un conteneur).
git_ok=0
if command -v git >/dev/null 2>&1 \
    && git -c safe.directory='*' -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git_ok=1
fi
tracked() { # <chemin relatif au dépôt>
    [[ "$git_ok" -eq 1 ]] || return 0
    git -c safe.directory='*' -C "$ROOT_DIR" ls-files --error-unmatch -- "$1" >/dev/null 2>&1
}

# --- liste : chemin dans le dépôt → chemin installé -------------------------
declare -A MAP=(
    [LICENSE]=LICENSE
    [packaging/debian-copyright]=copyright
    [content/LICENSE]=content-LICENSE
    [packaging/bundled-tools-compliance.txt]=bundled-tools-compliance.txt
    [packaging/gpl-2.0.txt]=gpl-2.0.txt
    [docs/Fabriquer_sa_cle.md]=Fabriquer_sa_cle.md
    [docs/exemple_pdf.pdf]=exemple_pdf.pdf
    [docs/exemple_lot.pdf]=exemple_lot.pdf
    [docs/Annexes/README.md]=Annexes/README.md
)
INDEX="$ROOT_DIR/docs/Annexes/README.md"
[[ -f "$INDEX" ]] || die "docs/Annexes/README.md absent"
while read -r name; do
    src="docs/Annexes/$name"
    [[ -f "$ROOT_DIR/$src" ]] || die "$src cité par l'index mais absent"
    tracked "$src" || die "$src cité par l'index mais non versionné (git add)"
    MAP[$src]="Annexes/$name"
done < <(grep -o '](Annexe_[A-Za-z0-9_.-]*\.md' "$INDEX" | sed 's/^](//' | sort -u)
for f in "$ROOT_DIR"/docs/Annexes/*; do
    name="docs/Annexes/$(basename "$f")"
    [[ -n "${MAP[$name]:-}" ]] || echo "install-docs : $name non installé (absent de l'index ou non versionné)" >&2
done
[[ "$git_ok" -eq 1 ]] || echo "install-docs : git indisponible, annexes choisies par l'index seul" >&2

# Copies de commodité à la racine : même source, autre emplacement.
declare -A COPIES=(
    [USER_GUIDE.md]=docs/Annexes/Annexe_G_guide_utilisateur.md
    [GUIDE_DEPLOIEMENT_BANC.md]=docs/Annexes/Annexe_H_deploiement_banc.md
    [THIRD_PARTY_LICENSES.md]=docs/Annexes/Annexe_J_licences_tierces.md
)

# --- chemins ----------------------------------------------------------------
norm() { # <chemin> : « . » et « .. » résolus ; vide s'il sort du dépôt
    local part out=() parts
    IFS=/ read -ra parts <<<"$1"
    for part in "${parts[@]}"; do
        case "$part" in
            ''|.) ;;
            ..) [[ ${#out[@]} -gt 0 ]] || { echo ""; return; }
                unset 'out[${#out[@]}-1]' ;;
            *) out+=("$part") ;;
        esac
    done
    (IFS=/; echo "${out[*]}")
}
relto() { # <cible> <dossier> : chemin de <cible> vu de <dossier> (relatifs à DOC)
    local t b i=0 res=""
    IFS=/ read -ra t <<<"$1"
    IFS=/ read -ra b <<<"$2"
    while [[ $i -lt ${#t[@]} && $i -lt ${#b[@]} && "${t[$i]}" == "${b[$i]}" ]]; do i=$((i + 1)); done
    local j
    for ((j = i; j < ${#b[@]}; j++)); do res+="../"; done
    for ((j = i; j < ${#t[@]}; j++)); do res+="${t[$j]}"; [[ $j -lt $((${#t[@]} - 1)) ]] && res+="/"; done
    echo "$res"
}
dir_of() { [[ "$1" == */* ]] && echo "${1%/*}" || echo ""; }

# Réécrit les liens relatifs du Markdown <source dans le dépôt> installé en
# <chemin installé>.
install_md() {
    local src="$1" dst="$2" line
    mkdir -p "$DOC/$(dir_of "$dst")"
    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" == *"]("* ]]; then
            line="$(rewrite_line "$src" "$dst" "$line")" || exit 1
        fi
        printf '%s\n' "$line"
    done < "$ROOT_DIR/$src" > "$DOC/$dst"
    chmod 0644 "$DOC/$dst"
}
rewrite_line() { # <source> <installé> <ligne>
    local src="$1" dst="$2" out="" rest="$3" m link path anchor r new
    local re='\]\(([^)[:space:]]+)\)'
    while [[ "$rest" =~ $re ]]; do
        m="${BASH_REMATCH[0]}" link="${BASH_REMATCH[1]}"
        out+="${rest%%"$m"*}"
        rest="${rest#*"$m"}"
        case "$link" in
            *://*|mailto:*|\#*|/*) new="$link" ;;
            *)
                path="${link%%#*}" anchor="${link#"$path"}"
                r="$(norm "$(dir_of "$src")/$path")"
                if [[ -z "$r" ]]; then
                    die "$src : lien $link hors du dépôt"
                elif [[ -n "${MAP[$r]:-}" ]]; then
                    new="$(relto "${MAP[$r]}" "$(dir_of "$dst")")$anchor"
                else
                    [[ -e "$ROOT_DIR/$r" ]] && tracked "$r" \
                        || echo "install-docs : $src : $link → $r absent ou non versionné (lien GitHub laissé)" >&2
                    new="$BLOB_URL/$r$anchor"
                fi ;;
        esac
        out+="](${new})"
    done
    printf '%s' "$out$rest"
}

# --- installation -----------------------------------------------------------
mkdir -p "$DOC"
for src in "${!MAP[@]}"; do
    dst="${MAP[$src]}"
    [[ -f "$ROOT_DIR/$src" ]] || die "$src absent"
    case "$src" in
        *.md) install_md "$src" "$dst" ;;
        *) install -D -m 0644 "$ROOT_DIR/$src" "$DOC/$dst" ;;
    esac
done
for dst in "${!COPIES[@]}"; do
    install_md "${COPIES[$dst]}" "$dst"
done
# Avis de licences tierces générés (packaging/licenses, make licenses).
if [[ "$APPIMAGE" -eq 1 ]]; then
    cat "$PKG_DIR/licenses/secmet-linux.txt" "$PKG_DIR/licenses/appimage-runtime.txt" \
        > "$DOC/THIRD_PARTY_LICENSES.txt"
else
    install -m 0644 "$PKG_DIR/licenses/secmet-linux.txt" "$DOC/THIRD_PARTY_LICENSES.txt"
fi
chmod 0644 "$DOC/THIRD_PARTY_LICENSES.txt"

"$PKG_DIR/check-installed-docs.sh" "$DOC"
