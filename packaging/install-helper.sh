#!/usr/bin/env bash
# Installation du composant système SecMet depuis une AppImage.
#
# Embarqué dans le binaire secmet (include_str!) et exécuté en root par
# pkexec : `bash -c "<ce script>" secmet-install <fichier AppImage>`.
#
# Pourquoi extraire depuis le fichier et non lire le montage : le montage
# FUSE d'une AppImage n'est lisible que par l'utilisateur qui l'a lancée
# (user_id=…, sans allow_root) ; root y reçoit « Permission non accordée ».
# Le fichier AppImage, lui, est un fichier ordinaire : root l'extrait dans
# un répertoire qui lui appartient, et rien de ce qui est installé ne passe
# par un chemin inscriptible par l'utilisateur.
#
# Installe :
#   - le helper privilégié          → /usr/bin/secmet-helper
#   - le bundle d'outils disque     → /usr/lib/secmet/{bin,lib}
#   - la policy PolicyKit           → /usr/share/polkit-1/actions/secmet.policy
set -euo pipefail

fail() {
    echo "ERREUR: $*" >&2
    exit 3
}

APPIMAGE="${1:?usage: install-helper.sh <fichier AppImage>}"
[[ -f "$APPIMAGE" ]] || fail "AppImage introuvable : $APPIMAGE"
[[ -x "$APPIMAGE" ]] || fail "AppImage non exécutable : $APPIMAGE"
[[ "$(id -u)" -eq 0 ]] || fail "doit être exécuté en root (pkexec)"

WORK="$(mktemp -d /var/tmp/secmet-install.XXXXXX)"
chmod 0700 "$WORK"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

# Extraction sans FUSE, par motifs, dans ./squashfs-root (runtime AppImage).
extract() {
    "$APPIMAGE" --appimage-extract "$1" >/dev/null 2>&1 || true
}
extract 'usr/bin/secmet-helper'
extract 'usr/share/secmet/secmet.policy'
extract 'usr/lib/secmet/*'

SRC="$WORK/squashfs-root"
HELPER_SRC="$SRC/usr/bin/secmet-helper"
POLICY_SRC="$SRC/usr/share/secmet/secmet.policy"
BUNDLE_SRC="$SRC/usr/lib/secmet"

[[ -f "$HELPER_SRC" ]] || fail "helper absent de l'AppImage (usr/bin/secmet-helper)"
[[ -f "$POLICY_SRC" ]] || fail "policy absente de l'AppImage (usr/share/secmet/secmet.policy)"
if find "$SRC" -type l -print -quit | grep -q .; then
    fail "l'AppImage contient des liens symboliques dans le composant système : refus"
fi

install -D -m 0755 "$HELPER_SRC" /usr/bin/secmet-helper

if [[ -d "$BUNDLE_SRC" ]]; then
    rm -rf /usr/lib/secmet.new
    mkdir -p /usr/lib/secmet.new
    cp -R --no-preserve=ownership,mode "$BUNDLE_SRC/." /usr/lib/secmet.new/
    chown -R root:root /usr/lib/secmet.new
    chmod -R u=rwX,go=rX /usr/lib/secmet.new
    # cp sans conservation du mode : les binaires ont perdu leur bit x.
    if [[ -d /usr/lib/secmet.new/bin ]]; then
        find /usr/lib/secmet.new/bin -type f -exec chmod 0755 {} +
    fi
    rm -rf /usr/lib/secmet
    mv /usr/lib/secmet.new /usr/lib/secmet
fi

install -D -m 0644 "$POLICY_SRC" /usr/share/polkit-1/actions/secmet.policy

echo "Composant système SecMet installé :"
echo "  /usr/bin/secmet-helper"
[[ -d "$BUNDLE_SRC" ]] && echo "  /usr/lib/secmet/ (outils disque embarqués)"
echo "  /usr/share/polkit-1/actions/secmet.policy"
