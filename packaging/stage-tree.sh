#!/usr/bin/env bash
# Arborescence commune des paquets (.deb, .rpm) : binaires, lanceur, icône,
# policy PolicyKit, documentation (install-docs.sh), TestDisk embarqué. Les
# métadonnées propres à chaque format (DEBIAN/control, spec RPM) sont
# ajoutées par l'appelant.
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
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps"

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

# Documentation : liste et noms communs avec l'AppImage (install-docs.sh,
# contrôlée par check-installed-docs.sh), avis de licences tierces compris.
VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
"$PKG_DIR/install-docs.sh" "$STAGE_DIR/usr/share/doc/secmet" "$VERSION"

"$PKG_DIR/bundle-tools.sh" "$STAGE_DIR"
