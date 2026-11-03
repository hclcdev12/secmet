#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
STAGE_DIR="$PKG_DIR/stage"
DIST_DIR="$ROOT_DIR/dist"

# shellcheck source=../scripts/env.sh
source "$ROOT_DIR/scripts/env.sh"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/secmet"
HELPER_BINARY="$TARGET_DIR/release/secmet-helper"
VERIFY_BINARY="$TARGET_DIR/release/secmet-verify"
RELEASE_BINARY="$TARGET_DIR/release/secmet-release"

VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo "amd64")"
# Suffixe de distribution (ex. SECMET_DEB_DISTRO=ubuntu22.04) : version
# Debian « 0.3.1~ubuntu22.04 », pour distinguer les paquets construits sur
# plusieurs bases (bibliothèques GTK liées différemment).
DEB_VERSION="$VERSION${SECMET_DEB_DISTRO:+~$SECMET_DEB_DISTRO}"

mkdir -p "$DIST_DIR"

# Reutilise target/release (ex. apres make dev / make build) pour eviter une
# recompilation complete sur CIFS. Forcer avec: SECMET_FORCE_BUILD=1 make deb
needs_build=0
if [[ "${SECMET_FORCE_BUILD:-0}" == "1" ]]; then
    needs_build=1
elif [[ ! -f "$BINARY" || ! -f "$HELPER_BINARY" || ! -f "$VERIFY_BINARY" ]]; then
    needs_build=1
elif find "$ROOT_DIR/src" "$ROOT_DIR/crates" "$ROOT_DIR/Cargo.toml" "$ROOT_DIR/Cargo.lock" \
        -type f -newer "$BINARY" 2>/dev/null | grep -q .; then
    needs_build=1
fi

if [[ "$needs_build" -eq 1 ]]; then
    echo "Building release binaries (secmet + secmet-helper)..."
    ( cd "$ROOT_DIR" && cargo build --release --workspace )
else
    echo "Using existing release binaries (skip cargo build)"
fi

if [[ ! -f "$BINARY" ]]; then
    echo "Release binary not found at: $BINARY"
    echo "Lancez d'abord: make build  (ou make dev)"
    exit 1
fi
if [[ ! -f "$HELPER_BINARY" ]]; then
    echo "Helper binary not found at: $HELPER_BINARY"
    echo "Lancez d'abord: make build  (ou make dev)"
    exit 1
fi

"$PKG_DIR/stage-tree.sh" "$STAGE_DIR" "$BINARY" "$HELPER_BINARY" "$VERIFY_BINARY" "$RELEASE_BINARY"
mkdir -p "$STAGE_DIR/DEBIAN"

cat > "$STAGE_DIR/DEBIAN/control" <<EOF
Package: secmet
Version: $DEB_VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libgtk-4-1, policykit-1
Recommends: testdisk, evince | atril | okular
Replaces: shred-graphique
Conflicts: shred-graphique
Maintainer: SecMet <noreply@example.com>
Description: Secure disk wiping GUI (GTK4)
 Graphical interface to securely wipe disks (NIST SP 800-88): native ATA/NVMe
 sanitize, Security Erase and verified host overwrite through a privileged
 helper, signed PDF certificates. Inclut testdisk embarqué (/usr/lib/secmet),
 utilisé automatiquement si l'outil système est absent.
EOF

cat > "$STAGE_DIR/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
rm -f /usr/bin/shred-graphique /usr/bin/shred-graphique-helper
rm -f /usr/share/applications/shred-graphique.desktop
rm -f /usr/share/polkit-1/actions/shred-graphique.policy
rm -f /usr/share/icons/hicolor/scalable/apps/shred-graphique.svg
rm -f /usr/lib/secmet/bin/nvme /usr/lib/secmet/bin/hdparm /usr/lib/secmet/bin/smartctl
EOF
chmod 0755 "$STAGE_DIR/DEBIAN/postinst"

echo "Building .deb package..."
dpkg-deb --root-owner-group --build "$STAGE_DIR" "$DIST_DIR/secmet_${DEB_VERSION}_${ARCH}.deb"

echo "Package created at: $DIST_DIR/secmet_${DEB_VERSION}_${ARCH}.deb"
