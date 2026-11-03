#!/usr/bin/env bash
# Construit l'image de la clé USB SecMet (Annexe M) dans un conteneur
# Ubuntu 24.04 privilégié : packaging/live/build_rootfs.sh y fabrique le
# système (debootstrap + XFCE + SecMet), le squashfs, le noyau, l'initrd et
# les binaires de démarrage. Sortie : dist/live/.
#
# usage : packaging/live/build_live.sh
# env   : SECMET_OPERATOR_PASSWORD  mot de passe de l'opérateur « secmet » (défaut : secmet)
#         SECMET_ENGINE             podman ou docker (détecté)
#         SECMET_UBUNTU_MIRROR      miroir apt (défaut : http://archive.ubuntu.com/ubuntu)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIVE_DIR="$ROOT_DIR/packaging/live"
OUT_DIR="$ROOT_DIR/dist/live"

ENGINE="${SECMET_ENGINE:-}"
if [[ -z "$ENGINE" ]]; then
    if command -v podman >/dev/null 2>&1; then ENGINE=podman
    elif command -v docker >/dev/null 2>&1; then ENGINE=docker
    else echo "podman ou docker requis" >&2; exit 1; fi
fi
IMAGE="secmet-live-builder:ubuntu24.04"
echo "Image de construction ($ENGINE) : $IMAGE"
"$ENGINE" build -t "$IMAGE" -f "$LIVE_DIR/live.Containerfile" "$LIVE_DIR"
mkdir -p "$OUT_DIR" "$ROOT_DIR/target-live"
exec "$ENGINE" run --rm --privileged \
    -v "$ROOT_DIR:/src:ro,Z" \
    -v "$OUT_DIR:/out:Z" \
    -v "$ROOT_DIR/target-live:/target:Z" \
    -v secmet-live-cargo-registry:/usr/local/cargo/registry:Z \
    -e SECMET_OPERATOR_PASSWORD="${SECMET_OPERATOR_PASSWORD:-secmet}" \
    -e SECMET_UBUNTU_MIRROR="${SECMET_UBUNTU_MIRROR:-http://archive.ubuntu.com/ubuntu}" \
    "$IMAGE" /src/packaging/live/build_rootfs.sh
