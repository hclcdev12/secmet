#!/usr/bin/env bash
# Arborescence commune des paquets (.deb, .rpm) : binaires, lanceur, icône,
# policy PolicyKit, documentation, TestDisk embarqué. Les métadonnées propres
# à chaque format (DEBIAN/control, spec RPM) sont ajoutées par l'appelant.
#
# Usage : stage-tree.sh <stage_dir> <secmet> <secmet-helper> <secmet-verify> [secmet-release]
set -euo pipefail

STAGE_DIR="${1:?usage: stage-tree.sh <stage_dir> <secmet> <secmet-helper> <secmet-verify> [secmet-release]}"
BINARY="${2:?binaire secmet}"
HELPER_BINARY="${3:?binaire secmet-helper}"
VERIFY_BINARY="${4:?binaire secmet-verify}"

PKG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$PKG_DIR/.." && pwd)"

for bin in "$BINARY" "$HELPER_BINARY" "$VERIFY_BINARY"; do
    [[ -f "$bin" ]] || { echo "binaire introuvable : $bin (make build)" >&2; exit 1; }
done

rm -rf "$STAGE_DIR"
mkdir -p \
    "$STAGE_DIR/usr/bin" \
    "$STAGE_DIR/usr/share/applications" \
    "$STAGE_DIR/usr/share/polkit-1/actions" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps" \
    "$STAGE_DIR/usr/share/doc/secmet/Annexes"

install -m 0755 "$BINARY" "$STAGE_DIR/usr/bin/secmet"
install -m 0755 "$HELPER_BINARY" "$STAGE_DIR/usr/bin/secmet-helper"
install -m 0755 "$VERIFY_BINARY" "$STAGE_DIR/usr/bin/secmet-verify"
# Outil de signature/vérification des versions (clé live : contrôle des
# paquets conservés sur la partition de données avant installation).
if [[ -n "${5:-}" ]]; then install -m 0755 "$5" "$STAGE_DIR/usr/bin/secmet-release"; fi
install -m 0644 "$PKG_DIR/secmet.desktop" \
    "$STAGE_DIR/usr/share/applications/secmet.desktop"
install -m 0644 "$ROOT_DIR/content/SecMet_icon.svg" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/secmet.svg"
install -m 0644 "$PKG_DIR/secmet.policy" \
    "$STAGE_DIR/usr/share/polkit-1/actions/secmet.policy"

DOC="$STAGE_DIR/usr/share/doc/secmet"
install_doc() {
    local src="$1" dst="$2"
    [[ -f "$src" ]] && install -m 0644 "$src" "$DOC/$dst"
    return 0
}
install_doc "$ROOT_DIR/LICENSE" LICENSE
install_doc "$ROOT_DIR/docs/Annexes/Annexe_J_licences_tierces.md" THIRD_PARTY_LICENSES.md
install_doc "$PKG_DIR/debian-copyright" copyright
install_doc "$PKG_DIR/bundled-tools-compliance.txt" bundled-tools-compliance.txt
install_doc "$PKG_DIR/gpl-2.0.txt" gpl-2.0.txt
install_doc "$ROOT_DIR/content/LICENSE" content-LICENSE
install_doc "$ROOT_DIR/docs/Annexes/Annexe_G_guide_utilisateur.md" USER_GUIDE.md
install_doc "$ROOT_DIR/docs/Annexes/Annexe_H_deploiement_banc.md" GUIDE_DEPLOIEMENT_BANC.md
if compgen -G "$ROOT_DIR/docs/Annexes/*.md" >/dev/null; then
    install -m 0644 "$ROOT_DIR/docs/Annexes/"*.md "$DOC/Annexes/"
fi

"$PKG_DIR/bundle-tools.sh" "$STAGE_DIR"
