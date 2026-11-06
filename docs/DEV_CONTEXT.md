# Contexte développeur SecMet

Notes courtes pour travailler sur le dépôt (v0.3.12).

## Workspace Cargo

| Package | Chemin | Rôle |
|---------|--------|------|
| `secmet` | `src/` | Bibliothèque + binaire GTK4 `secmet` (interface, client du helper, certificats, réglages, i18n) |
| `secmet-device` | `crates/secmet-device` | Accès matériel sans outil externe : ATA PASS-THROUGH (`SG_IO`), ioctl NVMe, sysfs, protections système, signatures résiduelles, dé-frozen |
| `secmet-core` | `crates/secmet-core` | Moteur de décision NIST SP 800-88 (plan par disque), canaris ChaCha20 ; crate pure, sans GTK ni ioctl |
| `secmet-helper` | `crates/secmet-helper` | Daemon privilégié `secmet-helper serve` : sessions, ouverture `O_EXCL | O_DIRECT`, exécution des plans, TestDisk, coffre ATA |
| `secmet-attest` | `crates/secmet-attest` | Attestation JSON canonique signée Ed25519, QR Base45, PDF natif, binaire `secmet-verify` ; versions signées (`release`, `secmet-release`) ; manifeste de lot signé (`manifest`) |

```bash
source scripts/env.sh
cargo test --workspace                      # nécessite libgtk-4-dev pour le package principal
cargo test -p secmet-helper                 # helper seul (sans GTK)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build --release --workspace           # target/release/{secmet,secmet-helper,secmet-verify}
```

Règles de la base de code : `panic = "abort"` en release, clippy avec `unwrap_used` / `expect_used` / `undocumented_unsafe_blocks` en erreur (tout `unsafe` vit dans `secmet-device::sys` et `aligned`), `cargo fmt` obligatoire. Un test ne doit jamais lancer de programme externe ni ouvrir de fenêtre.

## Privilèges et helper

- Élévation **uniquement** via PolicyKit : `pkexec /usr/bin/secmet-helper serve` (action `com.secmet.pkexec.helper`, `auth_admin_keep`). **Pas de repli sudo** : sans `pkexec` setuid, l'application refuse de démarrer (scénario R-12).
- Le helper écoute sur `/run/secmet/helper.sock` (NDJSON, une requête par ligne). Il imprime une ligne `{"socket","bootstrap","ttl_seconds"}` sur stdout au démarrage ; le client s'authentifie avec ce jeton puis obtient un **jeton par disque** (`ScanDevice`, 10 min) lié au numéro de périphérique, au numéro de série et à l'empreinte firmware.
- Commandes : `Authenticate`, `ListDisks`, `HostInfo`, `ScanDevice`, `PrepareCanaries`, `ExecutePlan` (flux d'événements `Progress`/`StepStarted`/`StepFinished`/`Log`), `Cancel`, `GetStatus`, `Unfreeze`, `Unlock`, `VerifyTestdisk`, `Shutdown` (différé tant qu'un job tourne).
- Les options `--socket-dir`, `--vault-dir`, `--allow-uid` sont refusées sous pkexec (réservées aux tests). Le daemon s'arrête seul après 10 min sans connexion.
- Variable `SECMET_HELPER` : chemin du helper en développement (`make run` la définit sous `PREFIX`).

## Modules clés côté application

| Chemin | Rôle |
|--------|------|
| `src/ui/` | Interface GTK4 : `app.rs` (fenêtre, boutons), `auth.rs` (démarrage, composant AppImage), `disk_list.rs` (chargement asynchrone de l'inventaire), `enrichment.rs` (scan par disque), `erase.rs`, `verify.rs`, `testdisk_ui.rs`, `settings_ui.rs`, `dialogs.rs` |
| `src/helper_client.rs` | Client du daemon (socket Unix, NDJSON, reconnexion limitée aux commandes sans effet) |
| `src/disk.rs` | Modèle disque alimenté par l'inventaire puis le scan |
| `src/certificate/` | Session, enregistrements, construction de l'attestation, export PDF/JSON |
| `src/i18n/` | Tables `(fr, en, de)` ; tests de complétude et de marqueurs `{nom}` ; les journaux restent en français ; les certificats suivent la langue de l'interface (libellés fixes : `secmet-attest::lang`, textes libres : `src/i18n/certificate.rs`) |
| `src/settings.rs`, `src/pdf_logo.rs` | `~/.config/secmet/settings.json`, logo personnalisé `pdf-logo.jpg` |
| `src/ui/lot_ui.rs` | Lots côté interface : dialogue au démarrage, bandeau, contrôle de doublon (`check_before_erase`), `register_start` / `register_end` (certificats automatiques dans le lot), clôture (`close_lot_work` : PDF groupé, manifeste, export `SECMET-EXP`, archivage) ; actif seulement si `paths::dedicated_data_dir()` |
| `src/clock.rs`, `src/ui/clock_ui.rs` | Seule source d'heure (`clock::now()` / `stamp()`), décalage manuel, contrôle au démarrage en mode clé (NTP, références : exécutable, lots), réserve sur les certificats (`caveat_key`) |
| `src/lot.rs` | Lot persistant (annexe M) : `lots/<LOT-ID>/{lot.json,journal.ndjson,certificates,sessions}`, journal chaîné SHA-256, écritures tmp+fsync+rename, reprise, doublons, clôture et archivage ; pur, testé avec `tempfile` |
| `src/paths.rs` | Chemins : helper, policy, et **dossier de données** (`layout()` : `~/.config/secmet` + `~/.secmet_logs`, ou `SECMET_DATA_DIR/{config,logs}` pour la clé live) ; aucun autre module ne calcule un chemin de données |
| `src/update.rs`, `src/ui/update_ui.rs` | Mises à jour : `latest.json` signé (clé publique de version `RELEASE_PUBLIC_KEYS`), état `update-state.json`, fil de vérification, point jaune sur Réglages ; `SECMET_UPDATE_URL` pour un serveur d'essai |
| `src/system_install.rs` | AppImage : détection du composant, installeur embarqué (`packaging/install-helper.sh` via `include_str!`) |

## Données sur le poste

Avec `SECMET_DATA_DIR=/chemin` (clé live, annexe M), `~/.config/secmet/` devient `/chemin/config/` et `~/.secmet_logs/` devient `/chemin/logs/`.

| Emplacement | Contenu |
|-------------|---------|
| `~/.secmet_logs/sessions/<SEC-…>/` | `journal.log`, `<disque>.log`, journaux TestDisk (0700/0600) |
| `~/.secmet_logs/session_report_<date>.json` | Rapport de session |
| `~/.secmet_logs/certificates/` | `<SEC-…>.pdf`, `<SEC-…>-<disque>.json` (attestation signée), `…-lot.pdf/json` |
| `~/.config/secmet/` | `settings.json`, `pdf-logo.jpg`, `attestation-key.hex` (0600), `attestation-key.pub`, `trusted-keys`, `update-state.json` (dernière vérification des mises à jour) ; `release-key.hex` sur le poste du responsable des versions seulement |
| `/var/lib/secmet/ata-security/` (root) | Mots de passe ATA posés par SecMet, le temps d'un SECURITY ERASE ; `SECMET-DATA/vault/` (root 0700) dès que la clé est montée et que sa racine appartient à root (`secmet-helper::vault::resolve_dir`) |
| `/run/secmet/` (root) | Socket du helper, journaux TestDisk temporaires |

## Packaging

```bash
make deb        # dist/secmet_<version>_amd64.deb (TestDisk embarqué sous /usr/lib/secmet/)
make rpm        # dist/secmet-<version>-1.<distribution>.x86_64.rpm (Fedora)
make appimage   # dist/SecMet_<version>_x86_64.AppImage, assemblée dans un conteneur Ubuntu 24.04
make live       # dist/live/ : image de la clé USB SecMet (conteneur privilégié) ; make live-key DEV=/dev/sdX l'écrit
make release    # dist/release/<version>/ : les 4 paquets fumés en conteneurs + SHA256SUMS(.sig) + latest.json signés
make publish    # idem, puis release GitHub sur ce dépôt (confirmation demandée) ; le push d'un tag vX.Y.Z suffit (package.yml)
```

L'AppImage **doit** être assemblée dans le conteneur (`packaging/appimage.Containerfile`) : les bibliothèques GTK embarquées sont liées à la glibc de la machine de construction, et le banc est en glibc 2.39. Voir [Annexe H](Annexes/Annexe_H_deploiement_banc.md).

La CI GitHub Actions (`ci.yml`, `fuzz.yml`, `package.yml`) rejoue fmt, clippy, tests, `cargo audit`, shellcheck, fuzzing et construction des paquets ; détail dans le [README](../README.md#intégration-continue-github-actions).

## Certificats d'exemple

- `docs/exemple_pdf.pdf` (un disque) et `docs/exemple_lot.pdf` (lot) illustrent le rendu. Pour générer un lot de démonstration signé avec une clé fixe, dans une langue donnée : `cargo run -p secmet-attest --example sample_certificate -- <dossier> [fr|en|de]`.

## Mise en production (spécifiée, non implémentée)

- [Annexe M](Annexes/Annexe_M_cle_live_lots.md) : clé USB SecMet, partition de données `SECMET-DATA`, lots persistants sur plusieurs laptops, manifeste de lot signé.
- [Annexe N](Annexes/Annexe_N_mises_a_jour.md) : mises à jour signées (clé de version hors ligne), releases GitHub du dépôt du code `hclcdev12/secmet` (depuis 0.3.5 ; `secmet-releases` y renvoie par `moved_to`), publiées par `package.yml` sur tag ou par `make release` en local. **Réalisé** : `packaging/release.sh`, module `secmet-attest::release`, binaire `secmet-release` (`keygen`, `sign`, `verify`), vérification dans l'application (`src/update.rs`, point jaune, section Mises à jour). **Décision** : pas de bouton Installer (l'application annonce, l'installation reste manuelle). **À faire** : versions sur la clé live (annexe M).

## Banc de test

Un banc Linux Mint 22 sert aux essais réels (disques SATA HDD/SSD). Les tests de bout en bout (effacement, certificat, AppImage au premier lancement) s'y font sur l'interface réelle ; les tests unitaires ne touchent jamais un disque.

Voir aussi le [README](../README.md) et les [annexes](Annexes/README.md).
