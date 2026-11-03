#!/usr/bin/env bash
# Copie TestDisk (+ ses libs privées) dans le paquet/AppDir pour les systèmes
# sans testdisk installé. Les accès disque (IDENTIFY, SMART, sanitize, Security
# Erase, NVMe) sont faits nativement par secmet-helper : plus aucun autre outil.
set -euo pipefail

STAGE_DIR="${1:?usage: bundle-tools.sh <stage_dir>}"

BUNDLE_BIN="$STAGE_DIR/usr/lib/secmet/bin"
BUNDLE_LIB="$STAGE_DIR/usr/lib/secmet/lib"
mkdir -p "$BUNDLE_BIN" "$BUNDLE_LIB"

# Copie tout ce que binaire lie, sauf le cœur glibc (toujours présent sur
# l'hôte) et le runtime compiler. Garantit que les outils embarqués tournent
# sans les paquets système correspondants.
copy_private_libs() {
    local bin="$1"
    local dest="$2"
    ldd "$bin" | while read -r line; do
        [[ "$line" =~ =\>\ (.+)\ \(0x ]] || continue
        local lib="${BASH_REMATCH[1]}"
        local base
        base="$(basename "$lib")"
        case "$base" in
            ld-linux*.so*|linux-vdso.so*|libc.so*|libm.so*|libdl.so*|libpthread.so*|libgcc_s.so*|libcrypto.so*|libssl.so*|libkeyutils.so*)
                continue
                ;;
            *)
                install -m 0644 "$lib" "$dest/$base"
                ;;
        esac
    done
}

bundle_binary() {
    local name="$1"
    local src
    src="$(command -v "$name" 2>/dev/null || true)"
    if [[ -z "$src" || ! -x "$src" ]]; then
        echo "WARN: $name introuvable sur la machine de build — non inclus dans le paquet"
        return 1
    fi
    install -m 0755 "$src" "$BUNDLE_BIN/$name"
    copy_private_libs "$src" "$BUNDLE_LIB"
    echo "Inclus: $name ($src)"
}

bundle_binary testdisk || true

DOC_DIR="$STAGE_DIR/usr/share/doc/secmet"
if [[ -f "$BUNDLE_BIN/testdisk" ]]; then
    cat > "$DOC_DIR/bundled-tools.txt" <<'EOF'
Outil embarqué (utilisé si le binaire système est absent)
  /usr/lib/secmet/bin/testdisk — TestDisk, licence GPL-2.0+

Détail légal, sources et offre GPL : bundled-tools-compliance.txt
Texte GPL-2.0 : gpl-2.0.txt
EOF
fi
