#!/usr/bin/env bash
# Installation seule d'un .deb SecMet dans une distribution propre (C8) :
# apt-get install résout les dépendances du paquet (Depends: libgtk-4-1,
# pkexec | policykit-1), puis fumée : pkexec présent, outils annexes,
# helper, policy PolicyKit, TestDisk embarqué, documentation installée
# (copyright sous ce seul nom, avis de licences tierces, liens relatifs :
# check-installed-docs.sh).
#
# Cibles : .deb ubuntu24.04 dans debian:13 (glibc 2.41 ≥ 2.39), .deb
# ubuntu22.04 dans debian:12 (glibc 2.36 ≥ 2.35), et les Ubuntu d'origine.
#
# usage : packaging/tests/install-deb.sh <paquet.deb> <image>...
#           conteneur neuf par image (podman, sinon docker ; SECMET_ENGINE
#           pour choisir), paquet copié dans un dossier temporaire monté en
#           lecture seule (jamais le dépôt)
#         packaging/tests/install-deb.sh --inside <paquet.deb>
#           dans le système courant, en root (travail deb-install de
#           package.yml, ou appel depuis le conteneur)
# exemple : packaging/tests/install-deb.sh dist/secmet_0.5.0~ubuntu24.04_amd64.deb docker.io/library/debian:13
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG_DIR="$(cd "$HERE/.." && pwd)"

inside() {
    local deb="$1" doc=/usr/share/doc/secmet
    fail() { echo "install-deb : $*" >&2; exit 1; }
    [[ "$(id -u)" -eq 0 ]] || fail "--inside demande root"
    export DEBIAN_FRONTEND=noninteractive
    . /etc/os-release
    echo "== $PRETTY_NAME : $(basename "$deb")"
    apt-get update -qq
    apt-get install -y -qq "$deb" >/dev/null || fail "apt-get install a échoué sur $PRETTY_NAME"
    dpkg -s secmet | grep -E '^(Version|Depends):'
    command -v pkexec >/dev/null || fail "pkexec absent après installation"
    secmet-verify --help >/dev/null || fail "secmet-verify --help"
    secmet-release --help >/dev/null || fail "secmet-release --help"
    # Sans argument, le helper refuse (code 2) au lieu de démarrer.
    if secmet-helper >/dev/null 2>&1; then fail "secmet-helper sans argument a démarré"; fi
    [[ -f /usr/share/polkit-1/actions/secmet.policy ]] || fail "policy PolicyKit absente"
    [[ -x /usr/lib/secmet/bin/testdisk ]] || fail "TestDisk embarqué absent"
    [[ -s "$doc/copyright" ]] || fail "$doc/copyright absent"
    [[ ! -e "$doc/debian-copyright" ]] || fail "$doc/debian-copyright présent"
    [[ -s "$doc/THIRD_PARTY_LICENSES.txt" ]] || fail "$doc/THIRD_PARTY_LICENSES.txt absent"
    bash "$PKG_DIR/check-installed-docs.sh" "$doc"
    echo "== $PRETTY_NAME : installation et fumée réussies"
}

if [[ "${1:-}" == "--inside" ]]; then
    deb="${2:?usage: install-deb.sh --inside <paquet.deb>}"
    [[ "$deb" == /* ]] || deb="$PWD/$deb"
    inside "$deb"
    exit 0
fi

DEB="${1:?usage: install-deb.sh <paquet.deb> <image>...}"
shift
[[ $# -gt 0 ]] || { echo "usage: install-deb.sh <paquet.deb> <image>..." >&2; exit 2; }
[[ -f "$DEB" ]] || { echo "install-deb : $DEB introuvable" >&2; exit 1; }
ENGINE="${SECMET_ENGINE:-}"
if [[ -z "$ENGINE" ]]; then
    if command -v podman >/dev/null 2>&1; then ENGINE=podman
    elif command -v docker >/dev/null 2>&1; then ENGINE=docker
    else echo "podman ou docker requis" >&2; exit 1; fi
fi

# Dossier propre au test : le paquet, ce script et le contrôle de la
# documentation (rien d'autre du dépôt n'est monté).
WORK="$(mktemp -d "${TMPDIR:-/tmp}/secmet-install-deb-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/packaging/tests"
cp "$DEB" "$WORK/"
cp "$HERE/install-deb.sh" "$WORK/packaging/tests/"
cp "$PKG_DIR/check-installed-docs.sh" "$WORK/packaging/"
name="$(basename "$DEB")"

failures=0
for image in "$@"; do
    if "$ENGINE" run --rm -v "$WORK:/t:ro,Z" "$image" \
        bash /t/packaging/tests/install-deb.sh --inside "/t/$name"; then
        echo "ok   $image"
    else
        echo "ÉCHEC $image"
        failures=$((failures + 1))
    fi
done
[[ "$failures" -eq 0 ]]
