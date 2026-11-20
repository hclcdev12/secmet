#!/usr/bin/env bash
# Contrôle de la documentation installée (install-docs.sh) : échec si
#   - copyright, LICENSE ou THIRD_PARTY_LICENSES.txt manque, ou si l'ancien
#     nom debian-copyright est présent ;
#   - une annexe installée n'est pas citée par l'index Annexes/README.md,
#     ou n'est pas versionnée (git ls-files docs/Annexes, quand git est
#     disponible sur le dépôt) ;
#   - un lien relatif d'un .md installé ne mène à aucun fichier installé
#     (ancre retirée).
#
# usage : check-installed-docs.sh <doc_dir>
set -euo pipefail

DOC="$(cd "${1:?usage: check-installed-docs.sh <doc_dir>}" && pwd)"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
errors=0
err() { echo "documentation installée : $*" >&2; errors=$((errors + 1)); }

for f in copyright LICENSE THIRD_PARTY_LICENSES.txt Annexes/README.md; do
    [[ -s "$DOC/$f" ]] || err "$f absent"
done
[[ ! -e "$DOC/debian-copyright" ]] || err "debian-copyright présent (nom unique : copyright)"

git_ok=0
if command -v git >/dev/null 2>&1 \
    && git -c safe.directory='*' -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git_ok=1
fi
for f in "$DOC"/Annexes/*; do
    [[ -e "$f" ]] || continue
    name="$(basename "$f")"
    [[ "$name" == README.md ]] && continue
    grep -qF "]($name" "$DOC/Annexes/README.md" 2>/dev/null \
        || err "Annexes/$name absente de l'index Annexes/README.md"
    if [[ "$git_ok" -eq 1 ]] \
        && ! git -c safe.directory='*' -C "$ROOT_DIR" ls-files --error-unmatch -- "docs/Annexes/$name" >/dev/null 2>&1; then
        err "Annexes/$name non versionnée (docs/Annexes/$name absent de git ls-files)"
    fi
done

re='\]\(([^)[:space:]]+)\)'
while IFS= read -r -d '' md; do
    rel="${md#"$DOC"/}"
    base="$(dirname "$md")"
    while IFS= read -r text || [[ -n "$text" ]]; do
        [[ "$text" == *"]("* ]] || continue
        while [[ "$text" =~ $re ]]; do
            link="${BASH_REMATCH[1]}"
            text="${text#*"${BASH_REMATCH[0]}"}"
            case "$link" in *://*|mailto:*|\#*) continue ;; esac
            path="${link%%#*}"
            [[ -e "$base/$path" ]] || err "$rel : lien $link sans fichier installé"
        done
    done < "$md"
done < <(find "$DOC" -name '*.md' -print0)

if [[ "$errors" -gt 0 ]]; then
    echo "documentation installée : $errors erreur(s) dans $DOC" >&2
    exit 1
fi
echo "documentation installée : $(find "$DOC" -type f | wc -l) fichiers contrôlés ($DOC)"
