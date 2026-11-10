#!/usr/bin/env bash
# Construit l'AppImage de « SecMet Clé USB » (crates/secmet-key-writer) :
# un seul binaire Rust (egui), sans bibliothèque embarquée. Comme l'AppImage
# de SecMet, l'assemblage se fait dans le conteneur Ubuntu 24.04 pour que
# la glibc exigée reste celle de Mint 22 / Ubuntu 24.04 (2.39).
# SECMET_APPIMAGE_NATIVE=1 force un assemblage sur la machine courante.
#
# Sortie : dist/secmet-key-writer_<version>_x86_64.AppImage
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
STAGE_DIR="$PKG_DIR/key_writer_stage"
DIST_DIR="$ROOT_DIR/dist"
TOOLS_DIR="$PKG_DIR/tools"

if [[ -z "${SECMET_IN_CONTAINER:-}" && "${SECMET_APPIMAGE_NATIVE:-0}" != "1" ]]; then
    engine=""
    if command -v podman >/dev/null 2>&1; then
        engine=podman
    elif command -v docker >/dev/null 2>&1; then
        engine=docker
    fi
    if [[ -z "$engine" ]]; then
        echo "ERREUR: podman ou docker requis pour assembler l'AppImage dans Ubuntu 24.04." >&2
        echo "        (SECMET_APPIMAGE_NATIVE=1 pour assembler ici, au risque d'une glibc trop récente)" >&2
        exit 1
    fi
    IMAGE="secmet-appimage-builder:ubuntu24.04"
    echo "Image de construction ($engine) : $IMAGE"
    "$engine" build -t "$IMAGE" -f "$PKG_DIR/appimage.Containerfile" "$PKG_DIR"
    mkdir -p "$ROOT_DIR/target-appimage" "$DIST_DIR"
    exec "$engine" run --rm \
        -v "$ROOT_DIR:/src:Z" \
        -v secmet-appimage-cargo-registry:/usr/local/cargo/registry:Z \
        -e CARGO_TARGET_DIR=/src/target-appimage \
        -w /src \
        "$IMAGE" ./packaging/build_key_writer_appimage.sh
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/secmet-key-writer"
VERSION="$(grep -m1 '^version' "$ROOT_DIR/crates/secmet-key-writer/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
ARCH="$(uname -m)"
OUT="$DIST_DIR/secmet-key-writer_${VERSION}_${ARCH}.AppImage"

echo "== secmet-key-writer $VERSION : compilation"
( cd "$ROOT_DIR" && cargo build --release -p secmet-key-writer )
[[ -x "$BINARY" ]] || { echo "ERREUR: $BINARY introuvable" >&2; exit 1; }

LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-${ARCH}.AppImage"
if [[ ! -x "$LINUXDEPLOY" ]]; then
    echo "== téléchargement de linuxdeploy"
    mkdir -p "$TOOLS_DIR"
    curl -fL --retry 3 -o "$LINUXDEPLOY" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage"
    chmod +x "$LINUXDEPLOY"
fi

echo "== arborescence"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/share/applications" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps" "$STAGE_DIR/usr/share/doc/secmet-key-writer"
install -m 0755 "$BINARY" "$STAGE_DIR/usr/bin/secmet-key-writer"
install -m 0644 "$PKG_DIR/secmet-key-writer.desktop" "$STAGE_DIR/usr/share/applications/"
install -m 0644 "$ROOT_DIR/content/SecMet_icon.svg" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/secmet-key-writer.svg"
install -m 0644 "$ROOT_DIR/LICENSE" "$STAGE_DIR/usr/share/doc/secmet-key-writer/LICENSE"
# AppRun : l'exécutable lui-même (aucune variable d'environnement à poser).
# linuxdeploy n'embarque aucune bibliothèque : le binaire ne lie que la
# glibc et charge Wayland/X11/EGL de l'hôte à l'exécution.
cat > "$STAGE_DIR/AppRun" <<'APPRUN'
#! /bin/bash
APPDIR="${APPDIR:-"$(dirname "$(readlink -f "$0")")"}"
exec "$APPDIR/usr/bin/secmet-key-writer" "$@"
APPRUN
chmod 0755 "$STAGE_DIR/AppRun"

echo "== assemblage"
mkdir -p "$DIST_DIR"
export APPIMAGE_EXTRACT_AND_RUN=1
TMP_OUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/secmet-key-writer.XXXXXX")"
(
    cd "$TMP_OUT_DIR" && "$LINUXDEPLOY" --appimage-extract-and-run \
        --appdir "$STAGE_DIR" \
        -e "$STAGE_DIR/usr/bin/secmet-key-writer" \
        -d "$STAGE_DIR/usr/share/applications/secmet-key-writer.desktop" \
        -i "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/secmet-key-writer.svg" \
        --output appimage
)
OUT_FILE="$(find "$TMP_OUT_DIR" -maxdepth 1 -name '*.AppImage' | head -1)"
[[ -n "$OUT_FILE" ]] || { echo "ERREUR: aucune AppImage produite" >&2; rm -rf "$TMP_OUT_DIR"; exit 1; }
mv -f "$OUT_FILE" "$OUT"
rm -rf "$TMP_OUT_DIR" "$STAGE_DIR"
echo "AppImage : $OUT ($(du -h "$OUT" | cut -f1))"
