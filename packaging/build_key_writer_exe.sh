#!/usr/bin/env bash
# Construit l'exécutable Windows de « SecMet Clé USB » (crates/secmet-key-writer)
# depuis Linux, en compilation croisée : cible x86_64-pc-windows-gnu, éditeur
# de liens MinGW. Un seul fichier .exe, sans bibliothèque à côté (TLS
# Schannel, xz en Rust pur). C'est le même exécutable que celui du travail
# key-writer-windows de package.yml (runner Windows, cible MSVC).
#
# Prérequis : rustup target add x86_64-pc-windows-gnu ; paquet mingw64-gcc
# (Fedora) ou gcc-mingw-w64-x86-64 (Debian / Ubuntu).
#
# Sortie : dist/secmet-key-writer_<version>_x86_64.exe
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
TARGET=x86_64-pc-windows-gnu
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
VERSION="$(grep -m1 '^version' "$ROOT_DIR/crates/secmet-key-writer/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
OUT="$DIST_DIR/secmet-key-writer_${VERSION}_x86_64.exe"

command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1 \
    || { echo "ERREUR: x86_64-w64-mingw32-gcc introuvable (dnf install mingw64-gcc / apt install gcc-mingw-w64-x86-64)" >&2; exit 1; }
rustup target list --installed 2>/dev/null | grep -qx "$TARGET" \
    || { echo "ERREUR: cible Rust absente : rustup target add $TARGET" >&2; exit 1; }

echo "== secmet-key-writer $VERSION : compilation croisée ($TARGET)"
( cd "$ROOT_DIR" && cargo build --release -p secmet-key-writer --target "$TARGET" )
BINARY="$TARGET_DIR/$TARGET/release/secmet-key-writer.exe"
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
ls -la "$OUT"
