#!/usr/bin/env bash
# Chaîne de publication d'une version SecMet (Annexe N, §N.7) : contrôles,
# paquets construits et fumés dans des conteneurs, empreintes signées avec la
# clé de version, latest.json. Rien ne quitte la machine sans --publish, qui
# demande confirmation avant d'envoyer la release sur le dépôt public.
#
# usage : packaging/release.sh [--publish] [--skip-checks] [--skip-build] [--skip-live] [--full-live]
#                              [--allow-dirty] [--allow-untagged]
#                              [--notes <fr.md>] [--notes-en <en.md>] [--notes-de <de.md>]
#
# env   : SECMET_RELEASE_KEY    clé privée de version (défaut ~/.config/secmet/release-key.hex)
#         SECMET_RELEASES_REPO  dépôt GitHub des releases (défaut hclcdev12/secmet)
#         SECMET_RELEASES_URL   base des téléchargements (défaut https://github.com/<repo>/releases/download)
#         SECMET_ENGINE         podman ou docker (détecté)
#
# Sortie : dist/release/<version>/ avec les 4 paquets, le livrable de fabrication
#          de la clé USB (secmet-live_<version>_x86_64.tar) et l'image prête à
#          écrire (secmet-key_<version>_x86_64.img.xz), sauf --skip-live,
#          SHA256SUMS, SHA256SUMS.sig, latest.json et les journaux de
#          construction (logs/). Deux cadences pour la clé (Annexe N, §N.7) :
#          si rien du système de la clé n'a changé depuis la release
#          précédente (packaging/live/live-mode.sh), son image est téléchargée,
#          vérifiée et reprise avec le nouveau paquet dans app/<version>/
#          (refresh_live.sh) ; sinon, ou avec --full-live, construction
#          complète. Avec KVM, la clé passe les contrôles QEMU
#          (packaging/live/qemu-check.sh et image-check.sh).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
cd "$ROOT_DIR"

PUBLISH=0 SKIP_CHECKS=0 SKIP_BUILD=0 SKIP_LIVE=0 FULL_LIVE=0 ALLOW_DIRTY=0 ALLOW_UNTAGGED=0
NOTES_FR="" NOTES_EN="" NOTES_DE=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --publish) PUBLISH=1 ;;
        --skip-checks) SKIP_CHECKS=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        --skip-live) SKIP_LIVE=1 ;;
        --full-live) FULL_LIVE=1 ;;
        --allow-dirty) ALLOW_DIRTY=1 ;;
        --allow-untagged) ALLOW_UNTAGGED=1 ;;
        --notes) NOTES_FR="$2"; shift ;;
        --notes-en) NOTES_EN="$2"; shift ;;
        --notes-de) NOTES_DE="$2"; shift ;;
        -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
    shift
done

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
TAG="v$VERSION"
REPO="${SECMET_RELEASES_REPO:-hclcdev12/secmet}"
BASE_URL="${SECMET_RELEASES_URL:-https://github.com/$REPO/releases/download}"
KEY="${SECMET_RELEASE_KEY:-$HOME/.config/secmet/release-key.hex}"
OUT_DIR="$ROOT_DIR/dist/release/$VERSION"
LOG_DIR="$OUT_DIR/logs"
DATE="$(date -u +%F)"

step() { printf '\n\033[1;33m== %s\033[0m (%s)\n' "$1" "$(date +%T)"; }
die()  { echo "ERREUR : $*" >&2; exit 1; }

# shellcheck source=../scripts/env.sh
source "$ROOT_DIR/scripts/env.sh"

# --- 0. État du dépôt ---------------------------------------------------------
step "SecMet $VERSION — état du dépôt"
if [[ "$ALLOW_DIRTY" -eq 0 && -n "$(git status --porcelain)" ]]; then
    die "arbre de travail modifié ; commitez, ou --allow-dirty"
fi
if [[ "$ALLOW_UNTAGGED" -eq 0 ]]; then
    head_tag="$(git describe --tags --exact-match HEAD 2>/dev/null || true)"
    [[ "$head_tag" == "$TAG" ]] || die "HEAD ne porte pas le tag $TAG (git tag -a $TAG), ou --allow-untagged"
fi
[[ -f "$KEY" ]] || die "clé de version absente : $KEY (secmet-release keygen \"$KEY\")"
PUBKEY="$(tr -d '[:space:]' < "${KEY%.hex}.pub")"
echo "commit : $(git rev-parse --short HEAD)  tag : $TAG  sortie : $OUT_DIR"

ENGINE="${SECMET_ENGINE:-}"
if [[ -z "$ENGINE" ]]; then
    if command -v podman >/dev/null 2>&1; then ENGINE=podman
    elif command -v docker >/dev/null 2>&1; then ENGINE=docker
    fi
fi
[[ "$SKIP_BUILD" -eq 1 || -n "$ENGINE" ]] || die "podman ou docker requis pour construire les paquets"

mkdir -p "$OUT_DIR" "$LOG_DIR"

# --- 1. Contrôles -------------------------------------------------------------
if [[ "$SKIP_CHECKS" -eq 0 ]]; then
    step "Contrôles : fmt, clippy, tests, audit, shellcheck"
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -q -- -D warnings
    cargo test --workspace -q 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v 'ok. 0 passed' || true
    cargo test --workspace -q >/dev/null
    if command -v cargo-audit >/dev/null 2>&1; then cargo audit -q; else echo "cargo-audit absent : audit ignoré" >&2; fi
    if command -v shellcheck >/dev/null 2>&1; then
        shellcheck "$PKG_DIR"/*.sh scripts/*.sh
    else
        echo "shellcheck absent : scripts non vérifiés" >&2
    fi
fi

# --- 2. Paquets ---------------------------------------------------------------
smoke_deb_rpm='
set -euo pipefail
secmet-verify --help >/dev/null
if secmet-helper 2>/dev/null; then exit 1; else test $? -eq 2; fi
test -x /usr/lib/secmet/bin/testdisk
test -f /usr/share/polkit-1/actions/secmet.policy
echo SMOKE_OK
'

build_in_container() { # <nom> <image> <script>
    local name="$1" image="$2" script="$3"
    step "Paquet $name ($image)"
    if "$ENGINE" run --rm \
        -v "$SRC_COPY:/src:ro,Z" -v "$OUT_DIR:/out:Z" \
        -e SECMET_FORCE_BUILD=1 -e SECMET_DEB_DISTRO="${DEB_DISTRO:-}" \
        "$image" bash -c "$script" > "$LOG_DIR/$name.log" 2>&1
    then
        tail -1 "$LOG_DIR/$name.log"
    else
        die "construction $name échouée, voir $LOG_DIR/$name.log"
    fi
}

if [[ "$SKIP_BUILD" -eq 0 ]]; then
    command -v rsync >/dev/null 2>&1 || die "rsync requis"
    SRC_COPY="$(mktemp -d)"
    trap 'rm -rf "$SRC_COPY"' EXIT
    rsync -a --exclude target --exclude 'target-*' --exclude dist --exclude .git \
        --exclude fuzz/target --exclude 'packaging/stage' --exclude 'packaging/appimage_stage' --exclude packaging/rpmbuild \
        --exclude packaging/tools "$ROOT_DIR/" "$SRC_COPY/"
    rm -f "$OUT_DIR"/*.deb "$OUT_DIR"/*.rpm "$OUT_DIR"/*.AppImage "$OUT_DIR"/*.exe

    deb_script="
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates git \
  libgtk-4-dev libglib2.0-dev policykit-1 testdisk dpkg-dev fakeroot >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal
. \"\$HOME/.cargo/env\"
cp -a /src /build && cd /build
./packaging/build_deb.sh
apt-get install -y -qq ./dist/secmet_*.deb >/dev/null
dpkg -s secmet | grep -E '^(Version|Depends)'
$smoke_deb_rpm
cp dist/*.deb /out/
"
    DEB_DISTRO=ubuntu24.04 build_in_container deb-ubuntu24.04 docker.io/library/ubuntu:24.04 "$deb_script"
    DEB_DISTRO=ubuntu22.04 build_in_container deb-ubuntu22.04 docker.io/library/ubuntu:22.04 "$deb_script"

    rpm_script="
set -euo pipefail
dnf install -y -q gcc make pkgconf-pkg-config curl ca-certificates git gtk4-devel glib2-devel \
  polkit testdisk rpm-build rpmdevtools >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal
. \"\$HOME/.cargo/env\"
cp -a /src /build && cd /build
./packaging/build_rpm.sh
dnf install -y -q dist/secmet-*.rpm >/dev/null
rpm -q secmet
$smoke_deb_rpm
cp dist/*.rpm /out/
"
    build_in_container rpm-fedora42 docker.io/library/fedora:42 "$rpm_script"

    step "AppImage (conteneur Ubuntu 24.04)"
    SECMET_FORCE_BUILD=1 "$PKG_DIR/build_appimage.sh" > "$LOG_DIR/appimage.log" 2>&1 \
        || die "AppImage échouée, voir $LOG_DIR/appimage.log"
    appimage="$ROOT_DIR/dist/SecMet_${VERSION}_x86_64.AppImage"
    [[ -f "$appimage" ]] || die "AppImage introuvable : $appimage"
    cp -f "$appimage" "$OUT_DIR/"
    extract="$(mktemp -d)"
    ( cd "$extract" && "$OUT_DIR/SecMet_${VERSION}_x86_64.AppImage" --appimage-extract >/dev/null )
    test -x "$extract/squashfs-root/usr/bin/secmet"
    test -x "$extract/squashfs-root/usr/bin/secmet-helper"
    test -f "$extract/squashfs-root/usr/share/secmet/secmet.policy"
    if command -v objdump >/dev/null 2>&1; then
        max="$(objdump -T "$extract/squashfs-root/usr/lib/libgtk-4.so.1" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
        [[ "$(printf '%s\n%s\n' "$max" GLIBC_2.39 | sort -V | tail -1)" == GLIBC_2.39 ]] \
            || die "AppImage : glibc $max requise (> 2.39, incompatible avec le banc)"
        echo "glibc max requise par GTK : $max"
    fi
    rm -rf "$extract"

    step "SecMet Clé USB (AppImage, conteneur Ubuntu 24.04)"
    "$PKG_DIR/build_key_writer_appimage.sh" > "$LOG_DIR/key-writer.log" 2>&1 \
        || die "AppImage de SecMet Clé USB échouée, voir $LOG_DIR/key-writer.log"
    key_writer="$ROOT_DIR/dist/secmet-key-writer_${VERSION}_x86_64.AppImage"
    [[ -f "$key_writer" ]] || die "AppImage introuvable : $key_writer"
    cp -f "$key_writer" "$OUT_DIR/"
    extract="$(mktemp -d)"
    ( cd "$extract" && "$OUT_DIR/secmet-key-writer_${VERSION}_x86_64.AppImage" --appimage-extract >/dev/null )
    "$extract/squashfs-root/usr/bin/secmet-key-writer" --version | grep -q "secmet-key-writer $VERSION" \
        || die "SecMet Clé USB : mauvaise version"
    rm -rf "$extract"

    # Exécutable Windows : compilation croisée (MinGW sur la machine, sinon
    # dans le conteneur Ubuntu 24.04 de l'AppImage, qui l'embarque).
    step "SecMet Clé USB (.exe Windows, compilation croisée)"
    "$PKG_DIR/build_key_writer_exe.sh" > "$LOG_DIR/key-writer-exe.log" 2>&1 \
        || die "exécutable Windows de SecMet Clé USB échoué, voir $LOG_DIR/key-writer-exe.log"
    cp -f "$ROOT_DIR/dist/secmet-key-writer_${VERSION}_x86_64.exe" "$OUT_DIR/"
    [[ "$(head -c 2 "$OUT_DIR/secmet-key-writer_${VERSION}_x86_64.exe")" == "MZ" ]] \
        || die "SecMet Clé USB : le .exe n'est pas un exécutable Windows"
fi

# --- 2c. Image de la clé USB --------------------------------------------------
# Deux cadences (Annexe N, §N.7). Système inchangé depuis la release
# précédente : son image et son livrable sont téléchargés, vérifiés avec la
# clé de version et repris, seule la partition de données est refaite avec
# le nouveau .deb dans app/<version>/ signé (déployé au démarrage). Sinon,
# conteneur privilégié (20 à 30 min). Les deux livrables rejoignent les
# paquets et SHA256SUMS ; avec KVM, la clé virtuelle doit déployer la version
# la plus récente de son magasin au démarrage (packaging/live/qemu-check.sh,
# construction complète seulement) et l'image publiable (.img.xz) doit passer
# son premier démarrage en BIOS (packaging/live/image-check.sh : clé de
# signature créée, export agrandi, version du magasin déployée).
if [[ "$SKIP_BUILD" -eq 0 && "$SKIP_LIVE" -eq 0 ]]; then
    step "Cadence de l'image de la clé USB"
    mode_line="$(SECMET_LIVE_FULL="$FULL_LIVE" SECMET_RELEASES_REPO="$REPO" "$PKG_DIR/live/live-mode.sh" "$VERSION" 2>"$LOG_DIR/live-mode.log" \
        || echo "mode=full reason=live-mode.sh en erreur (voir $LOG_DIR/live-mode.log)")"
    echo "$mode_line"
    "$PKG_DIR/live/tests/app-update.sh" > "$LOG_DIR/live-script-test.log" 2>&1 || die "test du script de mise à jour échoué, voir $LOG_DIR/live-script-test.log"
    if [[ "$mode_line" == mode=reuse* ]]; then
        previous="${mode_line#*previous=}"
        step "Clé USB reprise de la release $previous (système inchangé)"
        command -v gh >/dev/null 2>&1 || die "gh (GitHub CLI) requis pour reprendre l'image de $previous (ou --full-live)"
        rm -rf "$OUT_DIR/previous"
        gh release download "$previous" --repo "$REPO" --dir "$OUT_DIR/previous" \
            -p 'secmet-key_*_x86_64.img.xz' -p 'secmet-live_*_x86_64.tar' -p SHA256SUMS -p SHA256SUMS.sig -p latest.json \
            > "$LOG_DIR/live-previous.log" 2>&1 || die "téléchargement de la release $previous échoué, voir $LOG_DIR/live-previous.log"
        SECMET_RELEASES_URL="$BASE_URL" "$PKG_DIR/live/refresh_live.sh" --previous "$OUT_DIR/previous" \
            --deb "$OUT_DIR/secmet_${VERSION}~ubuntu24.04_amd64.deb" --key "$KEY" --trust "$PUBKEY" \
            --version "$VERSION" --out "$ROOT_DIR/dist" > "$LOG_DIR/live-refresh.log" 2>&1 \
            || die "reprise de l'image échouée, voir $LOG_DIR/live-refresh.log"
        tail -1 "$LOG_DIR/live-refresh.log"
        rm -rf "$OUT_DIR/previous"
    else
        step "Image de la clé USB (conteneur privilégié)"
        "$PKG_DIR/live/build_live.sh" > "$LOG_DIR/live.log" 2>&1 || die "image de la clé échouée, voir $LOG_DIR/live.log"
        "$PKG_DIR/live/bundle_live.sh" > "$LOG_DIR/live-bundle.log" 2>&1 || die "livrable de la clé échoué, voir $LOG_DIR/live-bundle.log"
        step "Image publiable de la clé USB (.img.xz)"
        "$PKG_DIR/live/make_key_image.sh" --dist > "$LOG_DIR/live-dist.log" 2>&1 || die "image publiable échouée, voir $LOG_DIR/live-dist.log"
    fi
    cp -f "$ROOT_DIR/dist/secmet-live_${VERSION}_x86_64.tar" "$ROOT_DIR/dist/secmet-key_${VERSION}_x86_64.img.xz" "$OUT_DIR/"
    if [[ -r /dev/kvm ]]; then
        if [[ "$mode_line" != mode=reuse* ]]; then
            step "Contrôle QEMU de la mise à jour au démarrage de la clé"
            "$PKG_DIR/live/qemu-check.sh" > "$LOG_DIR/live-check.log" 2>&1 || die "contrôle QEMU échoué, voir $LOG_DIR/live-check.log"
            tail -1 "$LOG_DIR/live-check.log"
        fi
        step "Contrôle QEMU du premier démarrage de l'image publiable (BIOS)"
        "$PKG_DIR/live/image-check.sh" > "$LOG_DIR/live-dist-check.log" 2>&1 || die "contrôle de l'image publiable échoué, voir $LOG_DIR/live-dist-check.log"
        tail -1 "$LOG_DIR/live-dist-check.log"
    else
        echo "pas de KVM : contrôles QEMU sautés (make live-check et make live-dist-check sur un poste qui l'a)"
    fi
fi

# --- 2b. Noms publiés ---------------------------------------------------------
# GitHub remplace « ~ » par « . » dans les noms des fichiers d'une release :
# on adopte sa convention, les .deb sont publiés sous
# « secmet_<version>.<distro>_amd64.deb » (la version Debian
# « <version>~<distro> » reste inchangée dans le paquet) ; un fichier déposé
# avec son nom d'origine aboutirait au même nom, et l'application télécharge
# ce nom-là.
for f in "$OUT_DIR"/secmet_*~*_amd64.deb; do
    [[ -f "$f" ]] && mv "$f" "${f/\~/.}"
done

# --- 3. Empreintes et signatures ---------------------------------------------
step "Signature avec la clé de version"
cargo build --release -q -p secmet-attest --bin secmet-release
RELEASE_BIN="${CARGO_TARGET_DIR:-$ROOT_DIR/target}/release/secmet-release"
notes_args=()
[[ -n "$NOTES_FR" ]] && notes_args+=(--notes-fr "$(cat "$NOTES_FR")")
[[ -n "$NOTES_EN" ]] && notes_args+=(--notes-en "$(cat "$NOTES_EN")")
[[ -n "$NOTES_DE" ]] && notes_args+=(--notes-de "$(cat "$NOTES_DE")")
"$RELEASE_BIN" sign "$OUT_DIR" --key "$KEY" --version "$VERSION" --date "$DATE" \
    --sums-url "$BASE_URL/$TAG/SHA256SUMS" "${notes_args[@]}"
"$RELEASE_BIN" verify "$OUT_DIR" --trust "$PUBKEY" >/dev/null
echo "vérification locale : OK"

# --- 4. Publication -----------------------------------------------------------
assets=("$OUT_DIR"/*.deb "$OUT_DIR"/*.rpm "$OUT_DIR"/*.AppImage
        "$OUT_DIR/SHA256SUMS" "$OUT_DIR/SHA256SUMS.sig" "$OUT_DIR/latest.json")
for f in "$OUT_DIR"/*.exe; do [[ -f "$f" ]] && assets+=("$f"); done
step "Résumé"
ls -la "${assets[@]}"
if [[ "$PUBLISH" -eq 0 ]]; then
    echo
    echo "Rien n'a été publié. Pour publier sur $REPO : packaging/release.sh --publish --skip-build --skip-checks"
    exit 0
fi

command -v gh >/dev/null 2>&1 || die "gh (GitHub CLI) requis pour publier"
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    die "la release $TAG existe déjà sur $REPO"
fi
echo
echo "Publication de SecMet $VERSION sur https://github.com/$REPO (release $TAG, ${#assets[@]} fichiers)."
read -r -p "Confirmer ? [oui/N] " answer
[[ "$answer" == "oui" ]] || { echo "annulé"; exit 1; }
notes_file="${NOTES_FR:-}"
if [[ -z "$notes_file" ]]; then
    notes_file="$(mktemp)"
    printf 'SecMet %s\n\nEmpreintes et notes signées : SHA256SUMS.sig, latest.json (clé de version %s).\n' \
        "$VERSION" "$(sed -n 's/.*"key_id": "\([0-9a-f]*\)".*/\1/p' "$OUT_DIR/latest.json" | head -1)" > "$notes_file"
fi
gh release create "$TAG" --repo "$REPO" --title "SecMet $VERSION" --notes-file "$notes_file" "${assets[@]}"
echo "latest.json : https://github.com/$REPO/releases/latest/download/latest.json"
