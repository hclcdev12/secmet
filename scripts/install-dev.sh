#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# shellcheck source=env.sh
source "$ROOT_DIR/scripts/env.sh"

PREFIX="${PREFIX:-/usr/local}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/secmet"
HELPER_BINARY="$TARGET_DIR/release/secmet-helper"
POLICY_SRC="$ROOT_DIR/packaging/secmet.policy"
DESKTOP_SRC="$ROOT_DIR/packaging/secmet.desktop"
ICON_SRC="$ROOT_DIR/content/SecMet_icon.svg"
HELPER_INSTALL="$PREFIX/bin/secmet-helper"

say() { printf "%s\n" "$*"; }
fail() { say "ERROR: $*"; exit 1; }

if [[ ! -f "$BINARY" ]]; then
    fail "Binaire introuvable: $BINARY (executez d'abord: cargo build --release)"
fi
if [[ ! -f "$HELPER_BINARY" ]]; then
    fail "Helper introuvable: $HELPER_BINARY"
fi
if [[ ! -f "$POLICY_SRC" ]]; then
    fail "Policy polkit introuvable: $POLICY_SRC"
fi
if [[ ! -f "$DESKTOP_SRC" ]]; then
    fail "Desktop entry introuvable: $DESKTOP_SRC"
fi
if [[ ! -f "$ICON_SRC" ]]; then
    fail "Icône introuvable: $ICON_SRC"
fi

TMP_POLICY="$(mktemp)"
trap 'rm -f "$TMP_POLICY"' EXIT

sed "s|/usr/bin/secmet-helper|$HELPER_INSTALL|g" "$POLICY_SRC" > "$TMP_POLICY"

say "Installation dev sous $PREFIX (sudo requis)..."
sudo rm -f \
    "$PREFIX/share/polkit-1/actions/shred-graphique.policy" \
    /usr/share/polkit-1/actions/shred-graphique.policy
sudo install -d \
    "$PREFIX/bin" \
    "$PREFIX/share/polkit-1/actions" \
    "$PREFIX/share/applications" \
    "$PREFIX/share/icons/hicolor/scalable/apps"
sudo install -m 0755 "$BINARY" "$PREFIX/bin/secmet"
sudo install -m 0755 "$HELPER_BINARY" "$PREFIX/bin/secmet-helper"
sudo install -m 0644 "$TMP_POLICY" "$PREFIX/share/polkit-1/actions/secmet.policy"
sudo install -m 0644 "$DESKTOP_SRC" "$PREFIX/share/applications/secmet.desktop"
sudo install -m 0644 "$ICON_SRC" "$PREFIX/share/icons/hicolor/scalable/apps/secmet.svg"
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    sudo gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
fi

say "Installe:"
say "  $PREFIX/bin/secmet"
say "  $PREFIX/bin/secmet-helper"
say "  $PREFIX/share/polkit-1/actions/secmet.policy"
say "  $PREFIX/share/applications/secmet.desktop"
say "  $PREFIX/share/icons/hicolor/scalable/apps/secmet.svg"
say "  (exec.path=$HELPER_INSTALL)"
say ""
say "Verification: make verify-polkit"
say "Lancement: make run"
say "  (SECMET_HELPER=$HELPER_INSTALL)"
