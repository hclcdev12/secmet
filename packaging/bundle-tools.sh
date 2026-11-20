#!/usr/bin/env bash
# Copie TestDisk (+ ses libs privées) dans le paquet/AppDir pour les systèmes
# sans testdisk installé. Les accès disque (IDENTIFY, SMART, sanitize, Security
# Erase, NVMe) sont faits nativement par secmet-helper : plus aucun autre outil.
#
# usage : bundle-tools.sh <stage_dir>              (paquets et AppDir)
#         bundle-tools.sh --copyrights <appdir>    (AppImage seulement)
#
# --copyrights : pour chaque bibliothèque embarquée (usr/lib/**/*.so*, celles
# de GTK déposées par linuxdeploy comme celles de TestDisk) et pour testdisk,
# retrouve le paquet de la machine de construction qui la fournit (dpkg -S,
# sinon rpm -qf) et copie son fichier copyright dans
# <appdir>/usr/share/doc/<paquet>/copyright (sur Fedora, les licences de
# rpm -qL mises bout à bout). La liste « fichier paquet » est écrite dans
# usr/share/doc/secmet/bundled-libraries.txt. Un fichier sans paquet ou sans
# copyright fait échouer la construction. Pas pour le .deb ni le .rpm : leurs
# usr/share/doc/<paquet> appartiendraient aux paquets du système.
set -euo pipefail

if [[ "${1:-}" == "--copyrights" ]]; then
    APPDIR="${2:?usage: bundle-tools.sh --copyrights <appdir>}"
    MULTIARCH="$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo "$(uname -m)-linux-gnu")"

    # Chemin, sur la machine de construction, du fichier copié dans l'AppDir.
    host_path() {
        local f="$1" rel base cand
        rel="${f#"$APPDIR"/usr/lib/}"
        base="$(basename "$f")"
        case "$f" in
            "$APPDIR"/usr/lib/secmet/bin/*) type -P "$base"; return ;;
            "$APPDIR"/usr/lib/secmet/lib/*) rel="$base" ;;
        esac
        for cand in "/usr/lib/$MULTIARCH/$rel" "/usr/lib/$rel" "/usr/lib64/$rel" "/lib/$MULTIARCH/$rel" \
                    "/usr/lib/$MULTIARCH/$base" "/lib/$MULTIARCH/$base" "/usr/lib64/$base" "/usr/lib/$base"; do
            if [[ -e "$cand" ]]; then echo "$cand"; return 0; fi
        done
        cand="$(ldconfig -p 2>/dev/null | awk -v b="$base" '$1 == b { print $NF; exit }')"
        [[ -n "$cand" ]] && echo "$cand"
    }

    # Paquet qui possède un chemin : tel quel, résolu, puis avec et sans /usr
    # (usrmerge : dpkg connaît parfois /lib/… pour /usr/lib/…).
    owner_of() {
        local p="$1" real cand out
        real="$(readlink -f "$p")"
        for cand in "$p" "$real" "${real#/usr}" "${p#/usr}" "/usr${real#/usr}"; do
            [[ "$cand" == /* ]] || continue
            out=""
            if command -v dpkg >/dev/null 2>&1; then
                # « paquet:amd64: /chemin », ou « p1, p2: /chemin »
                out="$(dpkg -S "$cand" 2>/dev/null | grep -v '^diversion' | head -1 || true)"
                out="${out%%: /*}"; out="${out%%,*}"; out="${out%%:*}"
            fi
            if [[ -z "$out" ]] && command -v rpm >/dev/null 2>&1; then
                out="$(rpm -qf --qf '%{NAME}\n' "$cand" 2>/dev/null | head -1 || true)"
                [[ "$out" == *" "* ]] && out=""
            fi
            if [[ -n "$out" ]]; then echo "$out"; return 0; fi
        done
        return 1
    }

    # Copie le copyright d'un paquet dans l'AppDir (une fois par paquet).
    copy_copyright() {
        local pkg="$1" lic
        local dest="$APPDIR/usr/share/doc/$pkg/copyright"
        [[ -s "$dest" ]] && return 0
        mkdir -p "$(dirname "$dest")"
        if [[ -f "/usr/share/doc/$pkg/copyright" ]]; then
            cp -L "/usr/share/doc/$pkg/copyright" "$dest"
        elif command -v rpm >/dev/null 2>&1 && rpm -q "$pkg" >/dev/null 2>&1; then
            # Fedora : licences du paquet, sinon des paquets du même .src.rpm
            # (zlib-ng-compat n'en installe pas, zlib-ng si).
            local srpm sib found=0
            srpm="$(rpm -q --qf '%{SOURCERPM}' "$pkg")"
            printf 'Paquet : %s (%s)\nLicence : %s\n\n' "$pkg" "$srpm" "$(rpm -q --qf '%{LICENSE}' "$pkg")" > "$dest"
            while read -r lic; do
                [[ -f "$lic" ]] || continue
                found=1
                { printf '==> %s <==\n' "$lic"; cat "$lic"; printf '\n'; } >> "$dest"
            done < <(rpm -qL "$pkg" 2>/dev/null || true
                     rpm -qa --qf '%{SOURCERPM} %{NAME}\n' | awk -v s="$srpm" -v p="$pkg" '$1 == s && $2 != p { print $2 }' \
                         | while read -r sib; do rpm -qL "$sib" 2>/dev/null || true; done)
            [[ "$found" -eq 1 ]] || : > "$dest"
        fi
        [[ -s "$dest" ]] || { rm -f "$dest"; return 1; }
    }

    LIST="$APPDIR/usr/share/doc/secmet/bundled-libraries.txt"
    mkdir -p "$(dirname "$LIST")"
    printf '%s\n' "# Bibliothèques et outils embarqués dans l'AppImage : fichier, paquet d'origine" \
        "# (copyright dans usr/share/doc/<paquet>/copyright)." > "$LIST"
    missing=()
    while IFS= read -r -d '' f; do
        rel="${f#"$APPDIR"/}"
        src="$(host_path "$f" || true)"
        if [[ -z "$src" ]]; then
            missing+=("$rel : introuvable sur la machine de construction"); continue
        fi
        if ! pkg="$(owner_of "$src")"; then
            missing+=("$rel : aucun paquet ne fournit $src"); continue
        fi
        if ! copy_copyright "$pkg"; then
            missing+=("$rel : pas de copyright pour le paquet $pkg"); continue
        fi
        printf '%s %s\n' "$rel" "$pkg" >> "$LIST"
    done < <(find "$APPDIR/usr/lib" -type f \( -name '*.so' -o -name '*.so.*' -o -path "$APPDIR/usr/lib/secmet/bin/*" \) -print0 | sort -z)
    if [[ "${#missing[@]}" -gt 0 ]]; then
        echo "ERREUR : copyright manquant pour ${#missing[@]} fichier(s) embarqué(s) :" >&2
        printf '  %s\n' "${missing[@]}" >&2
        exit 1
    fi
    echo "Copyright : $(grep -vc '^#' "$LIST") fichier(s) embarqué(s), $(grep -v '^#' "$LIST" | cut -d' ' -f2 | sort -u | wc -l) paquet(s)"
    exit 0
fi

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
