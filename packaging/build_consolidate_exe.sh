#!/usr/bin/env bash
# Construit l'exécutable Windows de « SecMet Regroupement » (crates/secmet-consolidate)
# depuis Linux, en compilation croisée : cible x86_64-pc-windows-gnu, éditeur
# de liens MinGW. Un seul fichier .exe, sans bibliothèque à côté. C'est le
# même exécutable que celui du travail consolidate-windows de package.yml
# (runner Windows, cible MSVC). Il intègre
# son avis de licences tierces (packaging/licenses/consolidate-windows.txt).
#
# Prérequis : rustup target add x86_64-pc-windows-gnu ; paquet mingw64-gcc
# (Fedora) ou gcc-mingw-w64-x86-64 (Debian / Ubuntu). Sans MinGW sur la
# machine, la compilation se fait dans le conteneur Ubuntu 24.04 de
# l'AppImage (packaging/appimage.Containerfile, podman ou docker), qui
# l'embarque.
#
# Sortie : dist/secmet-consolidate_<version>_x86_64.exe
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
DIST_DIR="$ROOT_DIR/dist"
TARGET=x86_64-pc-windows-gnu
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
VERSION="$(grep -m1 '^version' "$ROOT_DIR/crates/secmet-consolidate/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
OUT="$DIST_DIR/secmet-consolidate_${VERSION}_x86_64.exe"

if [[ -z "${SECMET_IN_CONTAINER:-}" ]] && ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    engine=""
    if command -v podman >/dev/null 2>&1; then
        engine=podman
    elif command -v docker >/dev/null 2>&1; then
        engine=docker
    fi
    if [[ -z "$engine" ]]; then
        echo "ERREUR: x86_64-w64-mingw32-gcc introuvable (dnf install mingw64-gcc / apt install gcc-mingw-w64-x86-64), et ni podman ni docker pour compiler dans le conteneur Ubuntu 24.04." >&2
        exit 1
    fi
    IMAGE="secmet-appimage-builder:ubuntu24.04"
    echo "MinGW absent : compilation dans le conteneur ($engine) $IMAGE"
    "$engine" build -t "$IMAGE" -f "$PKG_DIR/appimage.Containerfile" "$PKG_DIR"
    mkdir -p "$ROOT_DIR/target-windows" "$DIST_DIR"
    exec "$engine" run --rm \
        -v "$ROOT_DIR:/src:Z" \
        -v secmet-appimage-cargo-registry:/usr/local/cargo/registry:Z \
        -e CARGO_TARGET_DIR=/src/target-windows \
        -w /src \
        "$IMAGE" ./packaging/build_consolidate_exe.sh
fi

command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1 \
    || { echo "ERREUR: x86_64-w64-mingw32-gcc introuvable (dnf install mingw64-gcc / apt install gcc-mingw-w64-x86-64)" >&2; exit 1; }
rustup target list --installed 2>/dev/null | grep -qx "$TARGET" \
    || { echo "ERREUR: cible Rust absente : rustup target add $TARGET" >&2; exit 1; }

echo "== secmet-consolidate $VERSION : compilation croisée ($TARGET)"
( cd "$ROOT_DIR" && cargo build --release -p secmet-consolidate --target "$TARGET" )
BINARY="$TARGET_DIR/$TARGET/release/secmet-consolidate.exe"
[[ -f "$BINARY" ]] || { echo "ERREUR: $BINARY introuvable" >&2; exit 1; }

mkdir -p "$DIST_DIR"
cp -f "$BINARY" "$OUT"
# En-tête PE et absence de dépendance hors système (DLL MinGW).
[[ "$(head -c 2 "$OUT")" == "MZ" ]] || { echo "ERREUR: $OUT n'est pas un exécutable Windows" >&2; exit 1; }
if command -v x86_64-w64-mingw32-objdump >/dev/null 2>&1; then
    dlls="$(x86_64-w64-mingw32-objdump -p "$OUT" | sed -n 's/.*DLL Name: //p' | sort -u | tr '\n' ' ')"
    echo "DLL requises : $dlls"
    if grep -qiE 'libgcc|libstdc|libwinpthread' <<<"$dlls"; then
        echo "ERREUR: dépendance MinGW non système : l'exécutable ne serait pas autonome" >&2
        exit 1
    fi
fi
# Avis de licences tierces intégré au binaire (include_str!, « À propos »
# et --licenses) : le texte n'est pas compressé dans le .exe.
grep -q "SIL OPEN FONT LICENSE" "$OUT" \
    || { echo "ERREUR: avis de licences tierces absent de $OUT" >&2; exit 1; }
ls -la "$OUT"
