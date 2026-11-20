#!/usr/bin/env bash
# Empreintes du système de la clé (SYSTEM.sums, Annexe M §M.3) pour une
# partition SECMET-SYS extraite (mcopy -s) : tous les fichiers sous live/ et
# EFI/, les grub.cfg et les modules de GRUB BIOS (boot/grub/i386-pc), au
# format de sha256sum avec des chemins relatifs à la partition, triés. La
# police (boot/grub/fonts) n'en fait pas partie : grub-install (clé
# fabriquée par secmet-make-key.sh) y copie celle du poste qui fabrique.
# Calculé sur les fichiers tels qu'ils sont dans l'image (après le sed de
# grub.cfg), signé par la clé de version au moment de publier
# (seal_system.sh), vérifié au démarrage par secmet-image-check.
#
# usage : packaging/live/system-sums.sh <arborescence> <libellé du système>  (→ sortie standard)
set -euo pipefail
[[ $# -eq 2 && -d "$1" ]] || { echo "usage : $0 <arborescence SECMET-SYS> <libellé>" >&2; exit 2; }
cd "$1"
printf '# %s\n' "$2"
{
    for d in live EFI boot/grub/i386-pc; do
        [[ -d "$d" ]] && find "$d" -type f
    done
    [[ -f boot/grub/grub.cfg ]] && echo boot/grub/grub.cfg
    true
} | LC_ALL=C sort | while IFS= read -r f; do
    case "$f" in *\\*|*$'\n'*) echo "nom refusé : $f" >&2; exit 1 ;; esac
    printf '%s  %s\n' "$(sha256sum < "$f" | cut -c1-64)" "$f"
done
