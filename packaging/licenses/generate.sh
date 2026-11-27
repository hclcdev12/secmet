#!/usr/bin/env bash
# Avis de licences tierces de SecMet (plan B point 1, Annexe J §1.1) :
# générés par cargo-about (version épinglée), puis complétés des textes
# ajoutés à la main, et commités dans packaging/licenses/.
#
#   generate.sh           régénère les fichiers (make licenses)
#   generate.sh --check   régénère dans un dossier temporaire, compare aux
#                         fichiers commités et vérifie qu'aucune crate livrée
#                         ne manque (make licenses-check, CI)
#   generate.sh --complete  seulement ce dernier contrôle (rapide, sans cargo-about)
#
# Fichiers produits :
#   secmet-linux.txt        application, secmet-helper, secmet-verify,
#                           secmet-release (crate racine et ses dépendances)
#   key-writer-linux.txt    SecMet Clé USB Linux (crates + polices egui)
#   key-writer-windows.txt  SecMet Clé USB Windows (crates + polices egui +
#                           runtime MinGW) ; intégré au .exe (include_str!)
#   consolidate-linux.txt   SecMet Regroupement Linux (crates + polices egui)
#   consolidate-windows.txt SecMet Regroupement Windows (crates + polices egui +
#                           runtime MinGW) ; intégré au .exe (include_str!)
#   fonts-egui.txt          textes des polices d'egui, copiés du registre
#                           cargo avec contrôle sha256
#   appimage-runtime.txt    runtime des deux AppImage (joint à l'installation)
#   windows-mingw.txt       CRT MinGW-w64 et libgcc (inclus dans l'avis Windows)
set -euo pipefail

# Version de cargo-about : la sortie en dépend, elle est donc épinglée (et
# reprise par le travail « licenses » de .github/workflows/ci.yml).
ABOUT_VERSION="0.9.3"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SRC="$HERE/src"

CHECK=0
case "${1:-}" in
    --check) CHECK=1 ;;
    --complete) CHECK=2 ;;
    "") ;;
    *) echo "usage : $0 [--check | --complete]" >&2; exit 2 ;;
esac

die() { echo "ERREUR: $*" >&2; exit 1; }

# Rien ne manque : chaque crate livrée (cargo tree, dépendances normales,
# sans macro procédurale ni crate du dépôt) figure dans l'avis de sa cible.
missing() { # <avis> <manifeste> <cible>...
    local notice="$1" manifest="$2"; shift 2
    local t line
    for t in "$@"; do
        while read -r line; do
            echo "ERREUR: $line ($t) absent de $(basename "$notice")" >&2
            status=1
        done < <(comm -23 \
            <(cd "$ROOT" && cargo tree --locked -q --manifest-path "$manifest" \
                -e normal,no-proc-macro --target "$t" --prefix none --format '{p}' \
                | awk 'NF >= 2 && $3 !~ /^\(\// { sub(/^v/, "", $2); print $1, $2 }' | sort -u) \
            <(awk '/^  - / { print $2, $3 }' "$notice" | sort -u))
    done
}
complete_check() {
    missing "$HERE/secmet-linux.txt" Cargo.toml x86_64-unknown-linux-gnu
    missing "$HERE/key-writer-linux.txt" crates/secmet-key-writer/Cargo.toml x86_64-unknown-linux-gnu
    missing "$HERE/key-writer-windows.txt" crates/secmet-key-writer/Cargo.toml \
        x86_64-pc-windows-msvc x86_64-pc-windows-gnu
    missing "$HERE/consolidate-linux.txt" crates/secmet-consolidate/Cargo.toml x86_64-unknown-linux-gnu
    missing "$HERE/consolidate-windows.txt" crates/secmet-consolidate/Cargo.toml \
        x86_64-pc-windows-msvc x86_64-pc-windows-gnu
}

if [[ "$CHECK" == 2 ]]; then
    status=0
    complete_check
    [[ "$status" == 0 ]] && echo "Avis de licences tierces complets."
    exit "$status"
fi

command -v cargo-about >/dev/null 2>&1 \
    || die "cargo-about introuvable : cargo install --locked --features cli cargo-about@$ABOUT_VERSION"
have="$(cargo about --version | awk '{print $2}')"
[[ "$have" == "$ABOUT_VERSION" ]] \
    || die "cargo-about $have trouvé, $ABOUT_VERSION attendu (la sortie en dépend) : cargo install --locked --features cli cargo-about@$ABOUT_VERSION"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/secmet-licenses-work.XXXXXX")"
if [[ "$CHECK" == 1 ]]; then
    OUT="$(mktemp -d "${TMPDIR:-/tmp}/secmet-licenses.XXXXXX")"
    trap 'rm -rf "$WORK" "$OUT"' EXIT
else
    OUT="$HERE"
    trap 'rm -rf "$WORK"' EXIT
fi

# --- 1. Crates, par cible -----------------------------------------------------
about() { # <manifeste> <config> <sortie>
    ( cd "$ROOT" && cargo about generate --locked \
        --manifest-path "$1" -c "$HERE/$2" \
        -o "$3" "$HERE/about.txt.hbs" ) \
        || die "cargo about generate ($1, $2)"
}
about Cargo.toml about-linux.toml "$WORK/secmet-crates.txt"
about crates/secmet-key-writer/Cargo.toml about-linux.toml "$WORK/key-writer-linux-crates.txt"
about crates/secmet-key-writer/Cargo.toml about-windows.toml "$WORK/key-writer-windows-crates.txt"
about crates/secmet-consolidate/Cargo.toml about-linux.toml "$WORK/consolidate-linux-crates.txt"
about crates/secmet-consolidate/Cargo.toml about-windows.toml "$WORK/consolidate-windows-crates.txt"

# --- 2. Polices d'egui, copiées du registre cargo -----------------------------
# epaint_default_fonts déclare « (MIT OR Apache-2.0) AND OFL-1.1 AND
# Ubuntu-font-1.0 » sans les textes Hack (MIT + Bitstream Vera) et
# emoji-icon-font (MIT) : on joint les quatre fichiers d'origine. Les
# empreintes sont celles de la version indiquée ; une autre version de la
# crate demande de les relire et de les mettre à jour ici.
FONTS_VERSION="0.37.0"
declare -A FONT_SHA=(
    [UFL.txt]=2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002
    [OFL.txt]=6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2
    [Hack-Regular.txt]=47c0cccbeec7e8614548cc485588b28149e7874188df5f41b36efebcee285c87
    [emoji-icon-font-mit-license.txt]=b9d2c1d909aa149996fd4c91dcb92b2362a04431640c1d200959da94caf8cde1
)
declare -A FONT_TITLE=(
    [UFL.txt]="Ubuntu-Light.ttf — Ubuntu Font Licence 1.0"
    [OFL.txt]="NotoEmoji-Regular.ttf — SIL Open Font License 1.1"
    [Hack-Regular.txt]="Hack-Regular.ttf — MIT et licence Bitstream Vera"
    [emoji-icon-font-mit-license.txt]="emoji-icon-font.ttf, egui-icons.ttf — MIT"
)
lock_version="$(awk '/^name = "epaint_default_fonts"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$ROOT/Cargo.lock")"
[[ "$lock_version" == "$FONTS_VERSION" ]] \
    || die "epaint_default_fonts $lock_version dans Cargo.lock, $FONTS_VERSION attendu : relire les licences des polices et mettre à jour FONTS_VERSION et FONT_SHA dans $0"
fonts_dir=""
for d in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"epaint_default_fonts-$FONTS_VERSION/fonts"; do
    [[ -d "$d" ]] && fonts_dir="$d" && break
done
[[ -n "$fonts_dir" ]] || die "sources de epaint_default_fonts-$FONTS_VERSION absentes du registre cargo (cargo fetch)"
{
    echo "=============================================================================="
    echo "Polices intégrées par egui (epaint_default_fonts $FONTS_VERSION, outils egui de SecMet)"
    echo "Fonts embedded by egui (epaint_default_fonts $FONTS_VERSION, SecMet egui tools)"
    echo "=============================================================================="
    for f in UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt; do
        sum="$(sha256sum "$fonts_dir/$f" | cut -d' ' -f1)"
        [[ "$sum" == "${FONT_SHA[$f]}" ]] || die "$fonts_dir/$f : empreinte inattendue ($sum)"
        echo
        echo "------------------------------------------------------------------------------"
        echo "${FONT_TITLE[$f]}"
        echo "------------------------------------------------------------------------------"
        echo
        cat "$fonts_dir/$f"
    done
} > "$OUT/fonts-egui.txt"

# --- 3. Textes ajoutés à la main (packaging/licenses/src) ---------------------
section() { # <titre> <fichier>
    printf '\n------------------------------------------------------------------------------\n%s\n------------------------------------------------------------------------------\n\n' "$1"
    cat "$2"
}
{
    cat "$SRC/appimage-runtime.head.txt"
    cat "$SRC/type2-runtime.LICENSE"
    section "squashfuse — BSD-2-Clause" "$SRC/squashfuse.LICENSE"
    section "zstd — BSD-3-Clause" "$SRC/zstd.LICENSE"
    section "zlib — Zlib" "$SRC/zlib.LICENSE"
    section "linuxdeploy-plugin-gtk (crochet AppRun de l'AppImage SecMet) — MIT" "$SRC/linuxdeploy-plugin-gtk.LICENSE"
    section "libfuse 3 — GNU Lesser General Public License 2.1 (https://github.com/libfuse/libfuse)" "$SRC/libfuse.LGPL-2.1.txt"
} > "$OUT/appimage-runtime.txt"
{
    cat "$SRC/windows-mingw.head.txt"
    if [[ -f "$SRC/mingw-w64-runtime.COPYING" ]]; then
        section "MinGW-w64 runtime — COPYING.MinGW-w64-runtime.txt" "$SRC/mingw-w64-runtime.COPYING"
    fi
    section "GCC Runtime Library Exception 3.1 (libgcc)" "$SRC/gcc-runtime-exception-3.1.txt"
} > "$OUT/windows-mingw.txt"

# --- 4. Assemblage ------------------------------------------------------------
mpl_note() {
    if grep -q "option-ext" "$1"; then
        printf '\nMPL-2.0 : les sources de option-ext sont sur https://crates.io/crates/option-ext\n(et https://github.com/soc/option-ext).\n'
    fi
}
{ cat "$WORK/secmet-crates.txt"; mpl_note "$WORK/secmet-crates.txt"; } > "$OUT/secmet-linux.txt"
{ cat "$WORK/key-writer-linux-crates.txt"; mpl_note "$WORK/key-writer-linux-crates.txt"; echo; cat "$OUT/fonts-egui.txt"; } \
    > "$OUT/key-writer-linux.txt"
{ cat "$WORK/key-writer-windows-crates.txt"; mpl_note "$WORK/key-writer-windows-crates.txt"; echo; cat "$OUT/fonts-egui.txt"; echo; cat "$OUT/windows-mingw.txt"; } \
    > "$OUT/key-writer-windows.txt"
{ cat "$WORK/consolidate-linux-crates.txt"; mpl_note "$WORK/consolidate-linux-crates.txt"; echo; cat "$OUT/fonts-egui.txt"; } \
    > "$OUT/consolidate-linux.txt"
{ cat "$WORK/consolidate-windows-crates.txt"; mpl_note "$WORK/consolidate-windows-crates.txt"; echo; cat "$OUT/fonts-egui.txt"; echo; cat "$OUT/windows-mingw.txt"; } \
    > "$OUT/consolidate-windows.txt"

GENERATED=(secmet-linux.txt key-writer-linux.txt key-writer-windows.txt consolidate-linux.txt consolidate-windows.txt fonts-egui.txt appimage-runtime.txt windows-mingw.txt)

if [[ "$CHECK" == 0 ]]; then
    echo "Avis régénérés dans $HERE : ${GENERATED[*]}"
    exit 0
fi

# --- 5. Contrôles (--check) ---------------------------------------------------
status=0
for f in "${GENERATED[@]}"; do
    if ! cmp -s "$OUT/$f" "$HERE/$f"; then
        echo "ERREUR: packaging/licenses/$f n'est pas à jour (make licenses, puis commit)" >&2
        diff -u "$HERE/$f" "$OUT/$f" 2>/dev/null | head -20 >&2 || true
        status=1
    fi
done

complete_check

[[ "$status" == 0 ]] && echo "Avis de licences tierces à jour et complets."
exit "$status"
