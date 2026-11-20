#!/usr/bin/env bash
# Construit l'image de la clé USB SecMet (Annexe M) dans un conteneur
# Debian 13 privilégié : packaging/live/build_rootfs.sh y fabrique le
# système (debootstrap + XFCE + SecMet), le squashfs, le noyau, l'initrd et
# les binaires de démarrage. Sortie : dist/live/.
#
# usage : packaging/live/build_live.sh
# env   : SECMET_OPERATOR_PASSWORD  mot de passe de l'opérateur « secmet » (défaut : secmet)
#         SECMET_ENGINE             podman ou docker (détecté)
#         SECMET_DEBIAN_MIRROR      miroir apt (défaut : http://deb.debian.org/debian)
#         SECMET_DEBIAN_SECURITY_MIRROR  mises à jour de sécurité
#                                   (défaut : http://security.debian.org/debian-security)
#         SECMET_EFI_UPDATE=1       chaîne UEFI à la version courante de trixie au lieu
#                                   de packaging/live/efi-pin.sha256 (fetch_efi.sh)
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
IMAGE="secmet-live-builder:debian13"
echo "Image de construction ($ENGINE) : $IMAGE"
"$ENGINE" build -t "$IMAGE" -f "$LIVE_DIR/live.Containerfile" "$LIVE_DIR"
mkdir -p "$OUT_DIR" "$ROOT_DIR/target-live"
exec "$ENGINE" run --rm --privileged \
    -v "$ROOT_DIR:/src:ro,Z" \
    -v "$OUT_DIR:/out:Z" \
    -v "$ROOT_DIR/target-live:/target:Z" \
    -v secmet-live-cargo-registry:/usr/local/cargo/registry:Z \
    -e SECMET_OPERATOR_PASSWORD="${SECMET_OPERATOR_PASSWORD:-secmet}" \
    -e SECMET_DEBIAN_MIRROR="${SECMET_DEBIAN_MIRROR:-http://deb.debian.org/debian}" \
    -e SECMET_DEBIAN_SECURITY_MIRROR="${SECMET_DEBIAN_SECURITY_MIRROR:-http://security.debian.org/debian-security}" \
    -e SECMET_EFI_UPDATE="${SECMET_EFI_UPDATE:-0}" \
    "$IMAGE" /src/packaging/live/build_rootfs.sh
