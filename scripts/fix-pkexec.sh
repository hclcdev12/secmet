#!/usr/bin/env bash
# Corrige le bit setuid de pkexec (requis par PolicyKit).
set -euo pipefail

say() { printf "%s\n" "$*"; }
fail() { say "ERROR: $*"; exit 1; }

if ! command -v pkexec >/dev/null 2>&1; then
    fail "pkexec introuvable. Installez: sudo apt install policykit-1"
fi

PKEXEC_BIN="$(command -v pkexec)"

if [[ -u "$PKEXEC_BIN" ]]; then
    say "OK: $PKEXEC_BIN a deja le bit setuid."
    ls -l "$PKEXEC_BIN"
    exit 0
fi

say "pkexec sans setuid: $PKEXEC_BIN"
ls -l "$PKEXEC_BIN"

if command -v apt-get >/dev/null 2>&1; then
    say "Reinstallation de policykit-1..."
    sudo apt install --reinstall -y policykit-1
fi

if [[ "$(id -u)" -ne 0 ]]; then
  sudo chown root:root "$PKEXEC_BIN" 2>/dev/null || true
  sudo chmod u+s "$PKEXEC_BIN"
else
  chown root:root "$PKEXEC_BIN" 2>/dev/null || true
  chmod u+s "$PKEXEC_BIN"
fi

if [[ -u "$PKEXEC_BIN" ]]; then
    say "OK: bit setuid restaure."
    ls -l "$PKEXEC_BIN"
    exit 0
fi

fail "Impossible de poser le bit setuid sur $PKEXEC_BIN (environnement restreint ?)."
