#!/usr/bin/env bash
# Test de la documentation installée (install-docs.sh, check-installed-docs.sh),
# sans binaire ni paquet : installation dans un dossier temporaire, contrôle,
# puis cas qui doivent faire échouer le contrôle (annexe hors index ou non
# versionnée, lien relatif cassé, ancien nom debian-copyright, avis tiers
# absent). Vérifie aussi quelques réécritures de liens (fichier installé,
# GitHub au tag de la version).
#
# usage : packaging/tests/install-docs.sh        (code 0 si tout passe)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/secmet-install-docs-XXXXXX")"
trap 'rm -rf "$T"' EXIT
failures=0
check() {
    # check <nom du cas> <attendu> <obtenu>
    if [[ "$2" == "$3" ]]; then
        echo "ok   $1"
    else
        echo "ÉCHEC $1 : attendu « $2 », obtenu « $3 »"
        failures=$((failures + 1))
    fi
}
fails() { "$ROOT_DIR/packaging/check-installed-docs.sh" "$1" >/dev/null 2>&1 && echo non || echo oui; }
has() { grep -qF -- "$2" "$1" && echo oui || echo non; }

D="$T/doc"
"$ROOT_DIR/packaging/install-docs.sh" "$D" 9.9.9 > "$T/log" 2>&1 || { cat "$T/log"; exit 1; }
REPO="$(sed -n 's/^pub const SOURCE_REPOSITORY: &str = "\(https:[^"]*\)";$/\1/p' "$ROOT_DIR/crates/secmet-attest/src/legal.rs")"

echo "# arborescence et noms"
for f in copyright LICENSE content-LICENSE gpl-2.0.txt bundled-tools-compliance.txt \
    THIRD_PARTY_LICENSES.txt THIRD_PARTY_LICENSES.md USER_GUIDE.md GUIDE_DEPLOIEMENT_BANC.md \
    Fabriquer_sa_cle.md exemple_pdf.pdf exemple_lot.pdf Annexes/README.md \
    Annexes/Annexe_G_guide_utilisateur.md; do
    check "$f installé" oui "$([[ -s "$D/$f" ]] && echo oui || echo non)"
done
check "pas de debian-copyright" non "$([[ -e "$D/debian-copyright" ]] && echo oui || echo non)"
check "copyright = packaging/debian-copyright" oui "$(cmp -s "$D/copyright" "$ROOT_DIR/packaging/debian-copyright" && echo oui || echo non)"
check "avis tiers = secmet-linux.txt" oui "$(cmp -s "$D/THIRD_PARTY_LICENSES.txt" "$ROOT_DIR/packaging/licenses/secmet-linux.txt" && echo oui || echo non)"
check "autant d'annexes que l'index" \
    "$(grep -o '](Annexe_[A-Za-z0-9_.-]*\.md' "$ROOT_DIR/docs/Annexes/README.md" | sort -u | wc -l)" \
    "$(find "$D/Annexes" -name 'Annexe_*.md' | wc -l)"

echo "# liens réécrits"
check "annexe → ../Fabriquer_sa_cle.md" oui "$(has "$D/Annexes/Annexe_M_cle_live_lots.md" '](../Fabriquer_sa_cle.md')"
check "annexe J → ../content-LICENSE" oui "$(has "$D/Annexes/Annexe_J_licences_tierces.md" '](../content-LICENSE')"
check "annexe J → ../bundled-tools-compliance.txt" oui "$(has "$D/Annexes/Annexe_J_licences_tierces.md" '](../bundled-tools-compliance.txt')"
check "copie racine → Annexes/…" oui "$(has "$D/USER_GUIDE.md" '](Annexes/Annexe_N_mises_a_jour.md')"
check "non installé → GitHub au tag" oui "$(has "$D/Fabriquer_sa_cle.md" "]($REPO/blob/v9.9.9/packaging/live/README.md")"
check "plus aucun ../../" non "$(grep -rqF '](../../' "$D" && echo oui || echo non)"

echo "# contrôle : installation propre acceptée"
check "contrôle réussi" non "$(fails "$D")"

echo "# AppImage : runtime ajouté aux avis tiers"
"$ROOT_DIR/packaging/install-docs.sh" "$T/app" 9.9.9 --appimage > "$T/log" 2>&1 || { cat "$T/log"; exit 1; }
check "avis tiers AppImage" oui "$(cat "$ROOT_DIR/packaging/licenses/secmet-linux.txt" "$ROOT_DIR/packaging/licenses/appimage-runtime.txt" | cmp -s - "$T/app/THIRD_PARTY_LICENSES.txt" && echo oui || echo non)"

echo "# contrôle : cas refusés"
cp -a "$D" "$T/c1"
printf '# interne\n' > "$T/c1/Annexes/Annexe_I_estimation_couts.md"
check "annexe hors index et non versionnée" oui "$(fails "$T/c1")"
cp -a "$D" "$T/c2"
printf 'voir [x](Annexe_Z_absente.md)\n' >> "$T/c2/Annexes/Annexe_A_besoins_fonctionnels.md"
check "lien relatif cassé" oui "$(fails "$T/c2")"
cp -a "$D" "$T/c3"
cp "$T/c3/copyright" "$T/c3/debian-copyright"
check "ancien nom debian-copyright" oui "$(fails "$T/c3")"
cp -a "$D" "$T/c4"
rm "$T/c4/THIRD_PARTY_LICENSES.txt"
check "avis tiers absent" oui "$(fails "$T/c4")"
cp -a "$D" "$T/c5"
rm "$T/c5/copyright"
check "copyright absent" oui "$(fails "$T/c5")"
if git -c safe.directory='*' -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    cp -a "$D" "$T/c6"
    printf '# interne\n' > "$T/c6/Annexes/Annexe_Z_interne.md"
    printf -- '- [Z](Annexe_Z_interne.md)\n' >> "$T/c6/Annexes/README.md"
    check "annexe citée par l'index mais non versionnée" oui "$(fails "$T/c6")"
fi

echo
if [[ "$failures" -eq 0 ]]; then
    echo "documentation installée : tous les cas passent"
else
    echo "documentation installée : $failures échec(s)"
    exit 1
fi
