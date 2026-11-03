#!/usr/bin/env bash
# Construit une AppImage autonome (GUI GTK4 embarquée + composant système).
# Le composant privilégié (helper + policy + outils) est installé sur l'hôte
# au premier lancement par packaging/install-helper.sh, embarqué dans le
# binaire et exécuté par pkexec — voir src/system_install.rs.
#
# L'assemblage se fait dans un conteneur Ubuntu 24.04 (packaging/
# appimage.Containerfile) : les bibliothèques embarquées (GTK4, TestDisk)
# doivent être liées à une glibc au plus aussi récente que celle du banc.
# SECMET_APPIMAGE_NATIVE=1 force un assemblage sur la machine courante.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
STAGE_DIR="$PKG_DIR/appimage_stage"
DIST_DIR="$ROOT_DIR/dist"
TOOLS_DIR="$PKG_DIR/tools"

# --- Délégation au conteneur de construction ---------------------------------
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
    # Dépôt monté en /src ; cible cargo séparée du build natif ; registre
    # cargo persistant dans un volume nommé.
    exec "$engine" run --rm \
        -v "$ROOT_DIR:/src:Z" \
        -v secmet-appimage-cargo-registry:/usr/local/cargo/registry:Z \
        -e CARGO_TARGET_DIR=/src/target-appimage \
        -e SECMET_FORCE_BUILD="${SECMET_FORCE_BUILD:-0}" \
        -w /src \
        "$IMAGE" ./packaging/build_appimage.sh
fi

# shellcheck source=../scripts/env.sh
source "$ROOT_DIR/scripts/env.sh"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/secmet"
HELPER_BINARY="$TARGET_DIR/release/secmet-helper"

VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
DEB_ARCH="$(dpkg --print-architecture 2>/dev/null || echo "amd64")"
case "$DEB_ARCH" in
    amd64) APPIMAGE_ARCH="x86_64" ;;
    arm64) APPIMAGE_ARCH="aarch64" ;;
    *)     APPIMAGE_ARCH="$DEB_ARCH" ;;
esac
OUT="$DIST_DIR/SecMet_${VERSION}_${APPIMAGE_ARCH}.AppImage"

# Reutilise target/release (ex. apres make dev / make build) pour eviter une
# recompilation complete sur CIFS. Forcer avec: SECMET_FORCE_BUILD=1 make appimage
needs_build=0
if [[ "${SECMET_FORCE_BUILD:-0}" == "1" ]]; then
    needs_build=1
elif [[ ! -f "$BINARY" || ! -f "$HELPER_BINARY" ]]; then
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

[[ -f "$BINARY" ]] || { echo "Release binary not found at: $BINARY"; exit 1; }
[[ -f "$HELPER_BINARY" ]] || { echo "Helper binary not found at: $HELPER_BINARY"; exit 1; }

# --- Outils d'assemblage (linuxdeploy + plugin GTK, téléchargés une fois) ---
LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-${APPIMAGE_ARCH}.AppImage"
LINUXDEPLOY_GTK="$TOOLS_DIR/linuxdeploy-plugin-gtk.sh"
if [[ ! -x "$LINUXDEPLOY" ]]; then
    echo "Downloading linuxdeploy..."
    mkdir -p "$TOOLS_DIR"
    curl -fL --retry 3 -o "$LINUXDEPLOY" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${APPIMAGE_ARCH}.AppImage"
    chmod +x "$LINUXDEPLOY"
fi
if [[ ! -x "$LINUXDEPLOY_GTK" ]]; then
    echo "Downloading linuxdeploy-plugin-gtk (script)..."
    mkdir -p "$TOOLS_DIR"
    curl -fL --retry 3 -o "$LINUXDEPLOY_GTK" \
        "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
    chmod +x "$LINUXDEPLOY_GTK"
fi

# --- Staging AppDir (même arborescence que le .deb + composant système) ---
rm -rf "$STAGE_DIR"
mkdir -p \
    "$STAGE_DIR/usr/bin" \
    "$STAGE_DIR/usr/share/applications" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps" \
    "$STAGE_DIR/usr/share/metainfo" \
    "$STAGE_DIR/usr/share/secmet" \
    "$STAGE_DIR/usr/share/doc/secmet"

install -m 0755 "$BINARY" "$STAGE_DIR/usr/bin/secmet"
install -m 0755 "$HELPER_BINARY" "$STAGE_DIR/usr/bin/secmet-helper"

# AppRun source : linuxdeploy le déplacera en AppRun.wrapped et générera son
# propre AppRun (sources des hooks plugin-gtk puis exec de AppRun.wrapped).
# Sans ce fichier, linuxdeploy produit un AppRun.wrapped vide (lancement mort).
cat > "$STAGE_DIR/AppRun" <<'EOF'
#! /bin/bash
APPDIR="${APPDIR:-"$(dirname "$(readlink -f "$0")")"}"
exec "$APPDIR/usr/bin/secmet" "$@"
EOF
chmod 0755 "$STAGE_DIR/AppRun"
install -m 0644 "$PKG_DIR/secmet.desktop" \
    "$STAGE_DIR/usr/share/applications/secmet.desktop"
install -m 0644 "$ROOT_DIR/content/SecMet_icon.svg" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/secmet.svg"
# install-helper.sh n'est pas copié : root ne peut pas lire le montage FUSE
# de l'AppImage ; le script est embarqué dans le binaire (include_str!) et
# extrait lui-même ce qu'il installe depuis le fichier AppImage.
install -m 0644 "$PKG_DIR/secmet.policy" \
    "$STAGE_DIR/usr/share/secmet/secmet.policy"

if [[ -f "$ROOT_DIR/LICENSE" ]]; then
    install -m 0644 "$ROOT_DIR/LICENSE" "$STAGE_DIR/usr/share/doc/secmet/LICENSE"
fi
if [[ -f "$ROOT_DIR/docs/Annexes/Annexe_J_licences_tierces.md" ]]; then
    install -m 0644 "$ROOT_DIR/docs/Annexes/Annexe_J_licences_tierces.md" \
        "$STAGE_DIR/usr/share/doc/secmet/THIRD_PARTY_LICENSES.md"
fi
for doc in debian-copyright bundled-tools-compliance.txt gpl-2.0.txt; do
    if [[ -f "$PKG_DIR/$doc" ]]; then
        install -m 0644 "$PKG_DIR/$doc" "$STAGE_DIR/usr/share/doc/secmet/$doc"
    fi
done
if [[ -f "$ROOT_DIR/content/LICENSE" ]]; then
    install -m 0644 "$ROOT_DIR/content/LICENSE" \
        "$STAGE_DIR/usr/share/doc/secmet/content-LICENSE"
fi
if [[ -d "$ROOT_DIR/docs/Annexes" ]]; then
    mkdir -p "$STAGE_DIR/usr/share/doc/secmet/Annexes"
    install -m 0644 "$ROOT_DIR/docs/Annexes/"*.md \
        "$STAGE_DIR/usr/share/doc/secmet/Annexes/"
fi

"$PKG_DIR/bundle-tools.sh" "$STAGE_DIR"

# --- AppStream metainfo (généré : version + date du build) ---
RELEASE_DATE="$(date -u +%Y-%m-%d)"
cat > "$STAGE_DIR/usr/share/metainfo/com.secmet.metainfo.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>com.secmet</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-or-later</project_license>
  <name>SecMet</name>
  <summary>Effacement sécurisé et certifié de disques</summary>
  <description>
    <p>Interface graphique d'effacement sécurisé de disques (NIST SP 800-88 :
    ATA/NVMe sanitize, Security Erase, écriture hôte vérifiée par canaris)
    avec certificats d'effacement signés et validation post-effacement.</p>
  </description>
  <launchable type="desktop-id">secmet.desktop</launchable>
  <provides>
    <binary>secmet</binary>
  </provides>
  <releases>
    <release version="$VERSION" date="$RELEASE_DATE"/>
  </releases>
  <content_rating type="oars-1.1"/>
</component>
EOF

# --- Assemblage de l'AppImage ---
mkdir -p "$DIST_DIR"
export PATH="$TOOLS_DIR:$PATH"
# AppImage extraits sans FUSE (linuxdeploy, plugin, appimagetool imbriqués).
export APPIMAGE_EXTRACT_AND_RUN=1

echo "Assembling AppImage with linuxdeploy (GTK4 bundling)..."
# Le plugin nomme lui-même la sortie (<APP_NAME>-<arch>.AppImage) dans le CWD :
# assembler dans un tmp local puis renommer vers dist/.
TMP_OUT_DIR="$(mktemp -d /tmp/secmet-appimage.XXXXXX)"
(
    cd "$TMP_OUT_DIR" && "$LINUXDEPLOY" --appimage-extract-and-run \
        --appdir "$STAGE_DIR" \
        -e "$STAGE_DIR/usr/bin/secmet" \
        -e "$STAGE_DIR/usr/bin/secmet-helper" \
        -d "$STAGE_DIR/usr/share/applications/secmet.desktop" \
        -i "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/secmet.svg" \
        --plugin gtk \
        --output appimage
)
OUT_FILE="$(find "$TMP_OUT_DIR" -maxdepth 1 -name '*.AppImage' | head -1)"
if [[ -z "$OUT_FILE" ]]; then
    echo "ERREUR: aucune AppImage produite par linuxdeploy" >&2
    rm -rf "$TMP_OUT_DIR"
    exit 1
fi
mv -f "$OUT_FILE" "$OUT"
rm -rf "$TMP_OUT_DIR"
[[ -f "$OUT" ]] || { echo "ERREUR: $OUT introuvable après assemblage" >&2; exit 1; }

echo "AppImage created at: $OUT"
