#!/usr/bin/env bash
# Construit un paquet RPM (Fedora, et dérivés avec gtk4 ≥ 4.0 et polkit).
# Prérequis : rpm-build, rpmdevtools, gtk4-devel, glib2-devel, testdisk, Rust.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
STAGE_DIR="$PKG_DIR/rpm_stage"
TOPDIR="$PKG_DIR/rpmbuild"
DIST_DIR="$ROOT_DIR/dist"

# shellcheck source=../scripts/env.sh
source "$ROOT_DIR/scripts/env.sh"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/secmet"
HELPER_BINARY="$TARGET_DIR/release/secmet-helper"
VERIFY_BINARY="$TARGET_DIR/release/secmet-verify"
RELEASE_BINARY="$TARGET_DIR/release/secmet-release"

VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"

command -v rpmbuild >/dev/null 2>&1 || { echo "rpmbuild introuvable (dnf install rpm-build rpmdevtools)" >&2; exit 1; }

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
    echo "Building release binaries (secmet + secmet-helper + secmet-verify)..."
    ( cd "$ROOT_DIR" && cargo build --release --workspace )
else
    echo "Using existing release binaries (skip cargo build)"
fi

"$PKG_DIR/stage-tree.sh" "$STAGE_DIR" "$BINARY" "$HELPER_BINARY" "$VERIFY_BINARY" "$RELEASE_BINARY"

rm -rf "$TOPDIR"
mkdir -p "$TOPDIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS} "$DIST_DIR"
tar -C "$STAGE_DIR" -czf "$TOPDIR/SOURCES/secmet-$VERSION.tar.gz" usr

echo "Building .rpm package..."
rpmbuild -bb \
    --define "_topdir $TOPDIR" \
    --define "secmet_version $VERSION" \
    "$PKG_DIR/secmet.spec"

find "$TOPDIR/RPMS" -name 'secmet-*.rpm' -exec cp -f {} "$DIST_DIR/" \;
echo "Package created at: $(find "$DIST_DIR" -maxdepth 1 -name "secmet-$VERSION-*.rpm" | head -1)"
