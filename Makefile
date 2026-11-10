.PHONY: help deps dev run clean deb rpm appimage key-writer key-writer-appimage key-writer-exe install-deb test build release publish live live-image live-dist live-dist-check live-test live-key fix-pkexec verify-polkit fuzz-check

SHELL := /bin/bash
PREFIX ?= /usr/local
CARGO  ?= cargo
BINARY := secmet

GREEN  := \033[0;32m
YELLOW := \033[1;33m
NC     := \033[0m

# Rust (rustup) + contournement CIFS (hardlink build-scripts → EINVAL).
# Voir scripts/env.sh et scripts/nohardlink.c.
define CARGO_ENV
. "$(CURDIR)/scripts/env.sh";
endef

help: ## Affiche l'aide
	@echo "$(GREEN)SecMet$(NC)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(YELLOW)%-12s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "PREFIX=$(PREFIX)  (installation dev: make dev)"

deps: ## Installe et verifie les dependances systeme (apt)
	@./scripts/deps.sh

fix-pkexec: ## Restaure le bit setuid de pkexec (PolicyKit)
	@./scripts/fix-pkexec.sh

verify-polkit: ## Verifie pkexec, helper et policy Polkit (coherence chemins)
	@./scripts/verify-polkit.sh

dev: deps ## Build release + installation locale (helper + polkit)
	@$(CARGO_ENV) $(CARGO) build --release --workspace
	@PREFIX="$(PREFIX)" ./scripts/install-dev.sh

run: ## Lance l'application (apres make dev)
	@SECMET_HELPER="$(PREFIX)/bin/secmet-helper" \
		"$(PREFIX)/bin/$(BINARY)"

build: ## Compile en mode release uniquement
	@$(CARGO_ENV) $(CARGO) build --release --workspace

release: ## Chaîne de publication : contrôles, paquets en conteneurs, fumée, signature (rien n'est publié)
	@./packaging/release.sh

publish: ## Comme release, puis publication de la release GitHub (confirmation demandée)
	@./packaging/release.sh --publish

clean: ## Nettoie les artefacts de build
	@$(CARGO_ENV) $(CARGO) clean

deb: deps ## Construit un .deb (reutilise target/release si a jour)
	@./packaging/build_deb.sh

appimage: ## Construit une AppImage autonome dans Ubuntu 24.04 (podman/docker requis)
	@./packaging/build_appimage.sh

key-writer: ## Compile SecMet Clé USB (outil d'écriture de l'image de la clé, crates/secmet-key-writer)
	@cargo build --release -p secmet-key-writer

key-writer-appimage: ## AppImage de SecMet Clé USB dist/secmet-key-writer_<version>_x86_64.AppImage (conteneur Ubuntu 24.04)
	@./packaging/build_key_writer_appimage.sh

key-writer-exe: ## .exe Windows de SecMet Clé USB dist/secmet-key-writer_<version>_x86_64.exe (croisé : rustup target add x86_64-pc-windows-gnu, paquet mingw64-gcc)
	@./packaging/build_key_writer_exe.sh

rpm: ## Construit un .rpm (Fedora : rpm-build, gtk4-devel, testdisk requis)
	@./packaging/build_rpm.sh

live: ## Construit l'image de la clé USB SecMet (conteneur Ubuntu 24.04 privilégié) → dist/live/
	@./packaging/live/build_live.sh

live-bundle: ## Livrable de la clé USB dist/secmet-live_<version>_x86_64.tar (après make live)
	@./packaging/live/bundle_live.sh

live-image: ## Clé SecMet virtuelle dist/live/secmet-key.img (conteneur privilégié), pour QEMU
	@./packaging/live/make_key_image.sh

live-dist: ## Image publiable de la clé dist/secmet-key_<version>_x86_64.img.xz (après make live ; Etcher, Rufus, dd)
	@./packaging/live/make_key_image.sh --dist

live-dist-check: ## Contrôle QEMU (BIOS) du premier démarrage de l'image publiable : clé de signature créée, export agrandi (4 min)
	@./packaging/live/image-check.sh

live-test: ## Démarre la clé virtuelle dans QEMU (UEFI OVMF)
	@./packaging/live/qemu-test.sh

live-check: ## Contrôle QEMU de la mise à jour au démarrage de la clé (dist/live et un .deb requis, 5 min)
	@./packaging/live/qemu-check.sh

test-live-scripts: ## Test du script de mise à jour au démarrage de la clé (doublures, sans QEMU)
	@./packaging/live/tests/app-update.sh

live-key: ## Écrit la clé USB SecMet : make live-key DEV=/dev/sdX (root, efface la clé)
	@test -n "$(DEV)" || { echo "usage : make live-key DEV=/dev/sdX"; exit 1; }
	@sudo ./packaging/live/secmet-make-key.sh "$(DEV)"

fuzz-check: ## Compile les cibles de fuzz (stable, sans instrumentation)
	@$(CARGO_ENV) cargo check --manifest-path fuzz/Cargo.toml --all-targets

install-deb: deb ## Installe le .deb via apt (résout Depends, dont testdisk)
	@deb=$$(ls -1t dist/secmet_*.deb 2>/dev/null | head -1); \
	 if [ -z "$$deb" ]; then echo "Aucun .deb dans dist/ — lancez make deb"; exit 1; fi; \
	 echo "Installation: $$deb"; \
	 sudo apt-get install -y "./$$deb"

test: deps ## Lance les tests unitaires
	@$(CARGO_ENV) $(CARGO) test --workspace
	@./packaging/live/tests/app-update.sh

.DEFAULT_GOAL := help
