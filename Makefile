.PHONY: help deps dev run clean deb rpm appimage install-deb test build release publish live live-image live-test live-key fix-pkexec verify-polkit fuzz-check

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

publish: ## Comme release, puis publication sur secmet-releases (confirmation demandée)
	@./packaging/release.sh --publish

clean: ## Nettoie les artefacts de build
	@$(CARGO_ENV) $(CARGO) clean

deb: deps ## Construit un .deb (reutilise target/release si a jour)
	@./packaging/build_deb.sh

appimage: ## Construit une AppImage autonome dans Ubuntu 24.04 (podman/docker requis)
	@./packaging/build_appimage.sh

rpm: ## Construit un .rpm (Fedora : rpm-build, gtk4-devel, testdisk requis)
	@./packaging/build_rpm.sh

live: ## Construit l'image de la clé USB SecMet (conteneur Ubuntu 24.04 privilégié) → dist/live/
	@./packaging/live/build_live.sh

live-image: ## Clé SecMet virtuelle dist/live/secmet-key.img (conteneur privilégié), pour QEMU
	@./packaging/live/make_key_image.sh

live-test: ## Démarre la clé virtuelle dans QEMU (UEFI OVMF)
	@./packaging/live/qemu-test.sh

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

.DEFAULT_GOAL := help
