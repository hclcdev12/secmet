#!/usr/bin/env bash
# Vérifie la cohérence pkexec / policy Polkit pour SecMet (dev et prod).
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
HELPER="${SECMET_HELPER:-$PREFIX/bin/secmet-helper}"
POLICY_NAME="secmet.policy"
LEGACY_POLICY="shred-graphique.policy"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

ok=0
fail=0
warn=0

say_ok()   { printf "${GREEN}OK${NC}   %s\n" "$*"; ok=$((ok + 1)); }
say_fail() { printf "${RED}FAIL${NC} %s\n" "$*"; fail=$((fail + 1)); }
say_warn() { printf "${YELLOW}WARN${NC} %s\n" "$*"; warn=$((warn + 1)); }

# ── pkexec setuid ─────────────────────────────────────────────────────────────
if command -v pkexec >/dev/null 2>&1; then
    PKEXEC_BIN="$(command -v pkexec)"
    if [[ -u "$PKEXEC_BIN" ]]; then
        say_ok "pkexec setuid: $PKEXEC_BIN"
    else
        say_fail "pkexec sans bit setuid: $PKEXEC_BIN (make fix-pkexec)"
    fi
else
    say_fail "pkexec introuvable (apt install policykit-1)"
fi

# ── helper ────────────────────────────────────────────────────────────────────
if [[ -x "$HELPER" ]]; then
    say_ok "helper exécutable: $HELPER"
else
    say_fail "helper absent ou non exécutable: $HELPER (make dev)"
fi

# ── policies legacy ───────────────────────────────────────────────────────────
for legacy in \
    "/usr/share/polkit-1/actions/$LEGACY_POLICY" \
    "$PREFIX/share/polkit-1/actions/$LEGACY_POLICY"; do
    if [[ -f "$legacy" ]]; then
        say_warn "policy legacy présente: $legacy (supprimez ou relancez make dev)"
    fi
done

# ── secmet.policy ─────────────────────────────────────────────────────────────
POLICY=""
for candidate in \
    "$PREFIX/share/polkit-1/actions/$POLICY_NAME" \
    "/usr/share/polkit-1/actions/$POLICY_NAME" \
    "/usr/local/share/polkit-1/actions/$POLICY_NAME"; do
    if [[ -f "$candidate" ]]; then
        POLICY="$candidate"
        break
    fi
done

if [[ -z "$POLICY" ]]; then
    say_fail "secmet.policy introuvable (make dev)"
else
    say_ok "policy trouvée: $POLICY"
fi

# ── cohérence exec.path ───────────────────────────────────────────────────────
if [[ -n "$POLICY" && -x "$HELPER" ]]; then
    EXEC_PATH="$(grep -oP '(?<=org.freedesktop.policykit.exec.path">)[^<]+' "$POLICY" 2>/dev/null \
        || sed -n 's|.*org.freedesktop.policykit.exec.path">\([^<]*\)<.*|\1|p' "$POLICY" | head -1)"
    if [[ -z "$EXEC_PATH" ]]; then
        say_fail "annotation exec.path absente dans $POLICY"
    elif [[ "$EXEC_PATH" == "$HELPER" ]]; then
        say_ok "exec.path cohérent: $EXEC_PATH"
    else
        say_fail "exec.path incohérent: policy=$EXEC_PATH helper=$HELPER (make dev)"
    fi

    if grep -q '<allow_inactive>auth_admin_keep</allow_inactive>' "$POLICY"; then
        say_ok "allow_inactive=auth_admin_keep"
    else
        say_fail "allow_inactive doit etre auth_admin_keep dans $POLICY (relancez make dev)"
    fi
fi

# ── résumé ────────────────────────────────────────────────────────────────────
echo ""
if [[ $fail -eq 0 ]]; then
    printf "${GREEN}Verification Polkit OK${NC} (%d ok" "$ok"
    [[ $warn -gt 0 ]] && printf ", %d avertissement(s)" "$warn"
    printf ")\n"
    exit 0
else
    printf "${RED}Verification Polkit echouee${NC} (%d ok, %d echec(s)" "$ok" "$fail"
    [[ $warn -gt 0 ]] && printf ", %d avertissement(s)" "$warn"
    printf ")\n"
    exit 1
fi
