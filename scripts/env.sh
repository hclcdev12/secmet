#!/usr/bin/env bash
# Environnement Rust pour SecMet (rustup installe dans ~/.cargo).
# Usage dans un terminal :
#   source /chemin/vers/secmet/scripts/env.sh
# Ou une fois pour toutes (apres make deps) : voir ~/.bashrc
#
# Sur un depot monte en CIFS/SMB, active aussi un contournement (LD_PRELOAD)
# pour que cargo copie les build-scripts au lieu de les hardlinker (EINVAL).

_SECMET_SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Si ~/.rustup n'a pas de toolchain par défaut (disque home plein, install
# incomplète), réutilise un toolchain sous /tmp/secmet-rustup s'il existe.
ensure_rustup_toolchain_home() {
    local home_rustup="${RUSTUP_HOME:-$HOME/.rustup}"
    local tmp_rustup="/tmp/secmet-rustup"
    local tmp_cargo="/tmp/secmet-cargo"

    # Déjà pointé vers un toolchain utilisable.
    if [[ -n "${RUSTUP_HOME:-}" && -d "$RUSTUP_HOME/toolchains" ]] \
        && compgen -G "$RUSTUP_HOME/toolchains/*" >/dev/null 2>&1; then
        export CARGO_HOME="${CARGO_HOME:-$tmp_cargo}"
        mkdir -p "$CARGO_HOME"
        return 0
    fi

    if [[ -d "$home_rustup/toolchains" ]] \
        && compgen -G "$home_rustup/toolchains/*" >/dev/null 2>&1; then
        return 0
    fi

    if [[ -d "$tmp_rustup/toolchains" ]] \
        && compgen -G "$tmp_rustup/toolchains/*" >/dev/null 2>&1; then
        export RUSTUP_HOME="$tmp_rustup"
        export CARGO_HOME="${CARGO_HOME:-$tmp_cargo}"
        mkdir -p "$CARGO_HOME"
        return 0
    fi

    return 0
}

load_cargo_env() {
    ensure_rustup_toolchain_home
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    export PATH="$HOME/.cargo/bin:$PATH"
    # Fallback rustup sous /tmp (poste sans toolchain home).
    if [[ -d /tmp/secmet-cargo/bin ]]; then
        export PATH="/tmp/secmet-cargo/bin:$PATH"
        export RUSTUP_HOME="${RUSTUP_HOME:-/tmp/secmet-rustup}"
        export CARGO_HOME="${CARGO_HOME:-/tmp/secmet-cargo}"
    fi
}

# Si `cc` système absent : utilise Zig (toolchain user sous /tmp) comme linker.
ensure_local_cc() {
    if command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
        return 0
    fi
    local zig_cc="/tmp/secmet-toolchain/zig-cc"
    local zig_bin="/tmp/secmet-toolchain/zig/zig"
    if [[ -x "$zig_cc" ]]; then
        export CC="$zig_cc"
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$zig_cc"
        return 0
    fi
    if [[ -x "$zig_bin" ]]; then
        cat > "$zig_cc" <<'EOF'
#!/bin/bash
exec /tmp/secmet-toolchain/zig/zig cc "$@"
EOF
        chmod +x "$zig_cc"
        export CC="$zig_cc"
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$zig_cc"
        return 0
    fi
    echo "WARN: aucun compilateur C (cc/gcc). Installez gcc ou placez Zig dans /tmp/secmet-toolchain/zig" >&2
}

load_secmet_build_env() {
    load_cargo_env
    ensure_local_cc
    ensure_cifs_nohardlink
}

_secmet_path_on_cifs() {
    local path="$1"
    local fstype
    fstype="$(findmnt -n -o FSTYPE -T "$path" 2>/dev/null || true)"
    [[ "$fstype" == "cifs" || "$fstype" == "smb3" || "$fstype" == "smbfs" ]]
}

# Compile ~/.cache/secmet/nohardlink.so et exporte LD_PRELOAD si depot sur CIFS.
ensure_cifs_nohardlink() {
    local root src dst
    root="$(cd "$_SECMET_SCRIPTS_DIR/.." && pwd)"
    if ! _secmet_path_on_cifs "$root"; then
        return 0
    fi

    src="$_SECMET_SCRIPTS_DIR/nohardlink.c"
    dst="${XDG_CACHE_HOME:-$HOME/.cache}/secmet/nohardlink.so"
    if [[ ! -f "$src" ]]; then
        echo "WARN: $src manquant (contournement CIFS inactive)" >&2
        return 0
    fi
    local cc_bin=""
    if command -v gcc >/dev/null 2>&1; then
        cc_bin=gcc
    elif [[ -n "${CC:-}" ]]; then
        cc_bin="$CC"
    fi
    if [[ -z "$cc_bin" ]]; then
        echo "WARN: gcc absent, contournement CIFS inactive (make deps)" >&2
        return 0
    fi

    mkdir -p "$(dirname "$dst")"
    if [[ ! -f "$dst" || "$src" -nt "$dst" ]]; then
        if ! "$cc_bin" -shared -fPIC -O2 -o "$dst" "$src" 2>/dev/null; then
            echo "WARN: echec compilation nohardlink.so (contournement CIFS inactive)" >&2
            return 0
        fi
    fi

    case ":${LD_PRELOAD:-}:" in
        *":$dst:"*) ;;
        *) export LD_PRELOAD="$dst${LD_PRELOAD:+:$LD_PRELOAD}" ;;
    esac

    # Sur CIFS + disque home saturé : artefacts de build hors dépôt.
    if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
        export CARGO_TARGET_DIR="/tmp/secmet-target"
        mkdir -p "$CARGO_TARGET_DIR"
    fi
}

# Chargement automatique si le fichier est source (pas execute).
if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
    load_secmet_build_env
fi
