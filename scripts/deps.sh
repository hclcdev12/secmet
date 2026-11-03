#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

say() { printf "%s\n" "$*"; }
fail() { say "ERROR: $*"; exit 1; }

# shellcheck source=env.sh
source "$ROOT_DIR/scripts/env.sh"

APT_PACKAGES=(
    build-essential
    pkg-config
    curl
    libgtk-4-dev
    libglib2.0-dev
    policykit-1
    testdisk
)

REQUIRED_COMMANDS=(
    pkexec
    pkg-config
    curl
    testdisk
)

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

is_pkg_installed() {
    dpkg -s "$1" >/dev/null 2>&1
}

ensure_apt() {
    if ! command -v apt-get >/dev/null 2>&1; then
        fail "apt-get introuvable. Ce script supporte uniquement Ubuntu/Debian."
    fi
}

install_missing_packages() {
    local missing=()
    local pkg

    for pkg in "${APT_PACKAGES[@]}"; do
        if ! is_pkg_installed "$pkg"; then
            missing+=("$pkg")
        fi
    done

    if [[ ${#missing[@]} -eq 0 ]]; then
        say "Paquets apt deja installes."
        return 0
    fi

    say "Installation des paquets manquants: ${missing[*]}"
    sudo apt-get update
    sudo apt-get install -y "${missing[@]}"
}

ensure_rust_toolchain() {
    load_cargo_env

    if have_cmd cargo && have_cmd rustc; then
        return 0
    fi

    say "Installation du toolchain Rust (rustup)..."
    if ! have_cmd rustup; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
    fi

    load_cargo_env

    if have_cmd rustup; then
        rustup toolchain install stable
        rustup default stable
    fi

    load_cargo_env

    if ! have_cmd cargo || ! have_cmd rustc; then
        fail "cargo/rustc toujours absents. Executez: source \"\$HOME/.cargo/env\""
    fi

    ensure_cargo_shell_profile
    say "Rust toolchain pret."
}

# rustup ajoute normalement ~/.cargo/env au profil ; on verifie pour les shells interactifs.
ensure_cargo_shell_profile() {
    if [[ ! -f "$HOME/.cargo/env" ]]; then
        return 0
    fi

    local line='. "$HOME/.cargo/env"'
    if grep -qF '.cargo/env' "$HOME/.bashrc" 2>/dev/null \
        || grep -qF '.cargo/env' "$HOME/.profile" 2>/dev/null; then
        return 0
    fi

    say "Configuration du PATH cargo dans ~/.bashrc (une seule fois)..."
    {
        echo ""
        echo "# Rust (rustup) — charge cargo dans les terminaux interactifs"
        echo "$line"
    } >>"$HOME/.bashrc"
    say "  -> Nouveau terminal, ou: source \"\$HOME/.bashrc\""
}

verify_environment() {
    local ok=true
    local cmd

    load_cargo_env

    for cmd in "${REQUIRED_COMMANDS[@]}"; do
        if ! have_cmd "$cmd"; then
            say "  manquant: $cmd"
            ok=false
        fi
    done

    if ! have_cmd cargo; then
        say "  manquant: cargo"
        ok=false
    fi
    if ! have_cmd rustc; then
        say "  manquant: rustc"
        ok=false
    fi

    if ! pkg-config --exists gtk4 2>/dev/null; then
        say "  manquant: bibliotheque gtk4 (pkg-config gtk4)"
        ok=false
    fi

    if have_cmd pkexec; then
        local pkexec_bin
        pkexec_bin="$(command -v pkexec)"
        if [[ ! -u "$pkexec_bin" ]]; then
            local chmod_err restricted=false
            chmod_err="$(chmod u+s "$pkexec_bin" 2>&1)" || true
            if echo "$chmod_err" | grep -qiE 'not permitted|non permise|read-only|read only'; then
                restricted=true
            fi
            if [[ ! -u "$pkexec_bin" ]] && command -v sudo >/dev/null 2>&1; then
                local sudo_err
                sudo_err="$(sudo -n chmod u+s "$pkexec_bin" 2>&1)" || true
                if echo "$sudo_err" | grep -qiE 'not permitted|non permise|read-only|read only'; then
                    restricted=true
                fi
            fi
            if [[ -u "$pkexec_bin" ]]; then
                say "  pkexec: bit setuid corrige ($pkexec_bin)"
            elif [[ "$restricted" == "true" ]]; then
                say "  AVERTISSEMENT: pkexec sans setuid ($pkexec_bin)"
                say "    chmod impossible dans cet environnement (restreint) — poursuite."
                say "    Sur une machine complete: make fix-pkexec"
            else
                say "  pkexec sans setuid: $pkexec_bin"
                say "    -> make fix-pkexec"
                say "    -> ou: sudo apt install --reinstall policykit-1"
                ok=false
            fi
        fi
    fi

    if [[ "$ok" == "true" ]]; then
        return 0
    fi
    return 1
}

print_success() {
    say ""
    say "Environnement OK."
    say "  gtk4: $(pkg-config --modversion gtk4)"
    say "  cargo: $(command -v cargo)"
    say ""
    say "Dans un terminal (si 'cargo' est introuvable sans make):"
    say "  source \"$ROOT_DIR/scripts/env.sh\""
    say "  # ou: ./scripts/cargo build   # wrapper avec PATH rustup"
    say ""
    say "Prochaines etapes:"
    say "  make dev    # build release + install locale"
    say "  make run    # lancer l'application"
}

main() {
    # Ordre volontaire : apt d'abord (dont curl pour rustup), Rust ensuite.
    # load_cargo_env() n'est appele qu'apres installation rustup (dans ensure_rust_toolchain).
    ensure_apt
    install_missing_packages
    ensure_rust_toolchain

    say "Verification finale..."
    if verify_environment; then
        print_success
        exit 0
    fi

    say ""
    fail "Dependances incompletes apres installation. Verifiez sudo/apt puis relancez: make deps"
}

main "$@"
