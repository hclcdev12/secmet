# Contexte développeur SecMet

Notes courtes pour travailler sur le dépôt (v0.6.1).

## Workspace Cargo

| Package | Chemin | Rôle |
|---------|--------|------|
| `secmet` | `src/` | Bibliothèque + binaire GTK4 `secmet` (interface, client du helper, certificats, réglages, i18n) |
| `secmet-device` | `crates/secmet-device` | Accès matériel sans outil externe : ATA PASS-THROUGH (`SG_IO`), ioctl NVMe, sysfs, protections système, signatures résiduelles, dégel SATA |
| `secmet-core` | `crates/secmet-core` | Moteur de décision NIST SP 800-88 (plan par disque), canaris ChaCha20 ; crate pure, sans GTK ni ioctl |
| `secmet-helper` | `crates/secmet-helper` | Daemon privilégié `secmet-helper serve` : sessions, ouverture `O_EXCL | O_DIRECT`, exécution des plans, TestDisk, coffre ATA |
| `secmet-attest` | `crates/secmet-attest` | Attestation JSON canonique signée Ed25519, QR Base45, PDF natif, binaire `secmet-verify` ; versions signées (`release`, `secmet-release`) ; manifeste de lot signé (`manifest`) |
| `secmet-key-writer` | `crates/secmet-key-writer` | SecMet Clé USB (egui, Linux et Windows) : vérification de la release signée puis de l'empreinte, écriture de l'image de la clé avec complétion de la GPT pendant l'écriture (attributs GPT qui cachent `SECMET-SYS` et `SECMET-DATA` à Windows, `gpt::windows_attributes`), relecture ; hors `default-members` (`make key-writer`, `cargo build -p secmet-key-writer`), rust-version 1.95 |
| `secmet-consolidate` | `crates/secmet-consolidate` | SecMet Regroupement (egui, Linux et Windows) : regroupe les lots clôturés de plusieurs clés (Annexe M §M.12) ; cœur `secmet_attest::consolidation` (load_lot, discover, build, write, index non signé), PDF `secmet_attest::pdf::grouped` ; ligne de commande `--out` ; hors `default-members` (`make consolidate`), rust-version 1.95 |

```bash
source scripts/env.sh
cargo test --workspace                      # nécessite libgtk-4-dev pour le package principal
cargo test -p secmet-helper                 # helper seul (sans GTK)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo build --release --workspace           # target/release/{secmet,secmet-helper,secmet-verify,secmet-release}
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
| `src/output.rs` | Dossier de sortie des certificats sur un poste : réglage, contrôle au choix (`validate_choice`), résolution avec repli hors du fil GTK (`resolve`), point de montage (`mount_point_of`) |
| `src/ui/lot_ui.rs` | Lots côté interface : dialogue au démarrage, bandeau, contrôle de doublon (`check_before_erase`), `register_start` / `register_end` (certificats automatiques dans le lot), clôture (`close_lot_work` : PDF groupé, manifeste, export `SECMET-EXP`, archivage) ; actif si `paths::dedicated_data_dir()` (clé, lots de machines) ou si `settings.lots_enabled` (poste installé, « Mode lot » des Réglages, lots de disques, sans export sur une clé branchée) |
| `src/clock.rs`, `src/ui/clock_ui.rs` | Seule source d'heure (`clock::now()` / `stamp()`), décalage manuel, contrôle au démarrage en mode clé (NTP, références : exécutable, lots), réserve sur les certificats (`caveat_key`) |
| `src/lot.rs` | Lot persistant (annexe M) : `lots/<LOT-ID>/{lot.json,journal.ndjson,certificates,sessions}`, journal chaîné SHA-256, écritures tmp+fsync+rename, reprise, doublons, clôture et archivage ; pur, testé avec `tempfile` |
| `src/paths.rs` | Chemins : helper, policy, et **dossier de données** (`layout()` : `~/.config/secmet` + `~/.secmet_logs`, ou `SECMET_DATA_DIR/{config,logs}` pour la clé live) ; aucun autre module ne calcule un chemin de données |
| `src/update.rs`, `src/ui/update_ui.rs` | Mises à jour : `latest.json` signé (clé publique de version `RELEASE_PUBLIC_KEYS`), état `update-state.json`, fil de vérification, point jaune sur Réglages ; `SECMET_UPDATE_URL` pour un serveur d'essai |
| `src/update_install.rs` | Bouton **Installer la mise à jour** (`settings_ui.rs`) : `package_kind()` / `package_kind_for()` (AppImage → aucun, lien seul ; apt/dpkg → `.deb` choisi selon la glibc ; dnf/rpm → `.rpm` choisi par `rpm_asset()` parmi les `fcN` de `SHA256SUMS` selon `VERSION_ID` de `/etc/os-release`, sinon le plus récent ≤, sinon le plus ancien), téléchargement, contrôle de `SHA256SUMS` signé, installation sous `pkexec` ; sur la clé, dépôt en plus dans `app/<version>/` (deux versions gardées) |
| `src/busy.rs` | Opérations en cours (`begin(BusyKind)` → garde RAII : effacement, analyse, TestDisk, inventaire, PDF, clôture, déverrouillage, dé-frozen ; gardes portées par le minuteur de chaque opération), installation en cours, relance attendue ; `install_blocker()` pure : les Réglages masquent **Installer la mise à jour** tant qu'une opération tourne ou qu'un lot est ouvert, relu toutes les 500 ms et au clic (D14) |
| `src/system_install.rs` | AppImage : détection du composant, installeur embarqué (`packaging/install-helper.sh` via `include_str!`) |

## Données sur le poste

Avec `SECMET_DATA_DIR=/chemin` (clé live, annexe M), `~/.config/secmet/` devient `/chemin/config/` et `~/.secmet_logs/` devient `/chemin/logs/`.

| Emplacement | Contenu |
|-------------|---------|
| `~/.secmet_logs/sessions/<SEC-…>/` | `journal.log`, `<disque>.log`, journaux TestDisk (0700/0600) |
| `~/.secmet_logs/session_report_<date>.json` | Rapport de session |
| `~/Documents/SecMet-Certificats/` ou dossier choisi (`output::resolve`, défaut `paths::certificates_dir`, dossier Documents XDG ; lots clôturés copiés dans `<LOT>/` ; `logs/certificates/` avec `SECMET_DATA_DIR` ; ancien `~/.secmet_logs/certificates/` laissé en place) | `<SEC-…>.pdf`, `<SEC-…>-<disque>.json` (attestation signée), `…-lot.pdf/json` |
| `~/.config/secmet/` | `settings.json`, `pdf-logo.jpg`, `attestation-key.hex` (0600), `attestation-key.pub` (affichée dans les Réglages, copiée à chaque clôture de lot sous `cle-publique-secmet-<key_id>.pub`), `key-history.json` (clés créées par l'application), `trusted-keys`, `update-state.json` (dernière vérification des mises à jour) ; `release-key.hex` sur le poste du responsable des versions seulement |
| `/var/lib/secmet/ata-security/` (root) | Mots de passe ATA posés par SecMet, le temps d'un SECURITY ERASE ; `SECMET-DATA/vault/` (root 0700) dès que la clé est montée et que sa racine appartient à root (`secmet-helper::vault::resolve_dir`) |
| `/run/secmet/` (root) | Socket du helper, journaux TestDisk temporaires |

## Packaging

```bash
make deb        # dist/secmet_<version>_amd64.deb (TestDisk embarqué sous /usr/lib/secmet/)
make rpm        # dist/secmet-<version>-1.<distribution>.x86_64.rpm (Fedora)
make appimage   # dist/SecMet_<version>_x86_64.AppImage, assemblée dans un conteneur Ubuntu 24.04
make live       # dist/live/ : image de la clé USB SecMet (conteneur Debian 13 privilégié) ; make live-key DEV=/dev/sdX l'écrit
make release    # dist/release/<version>/ : les 17 fichiers (5 paquets fumés en conteneurs : .deb 24.04 et 22.04, .rpm fc44 et fc42, AppImage ; SecMet Clé USB et SecMet Regroupement, AppImage et .exe ; secmet-verify et secmet-release autonomes ; .img.xz, .tar et .packages.tar de la clé ; SHA256SUMS(.sig), latest.json signés)
make publish    # idem, puis release GitHub (confirmation) ; n'envoie pas encore .img.xz ni .tar (release.sh, liste assets) ; le push d'un tag vX.Y.Z suffit (package.yml)
```

L'AppImage **doit** être assemblée dans le conteneur (`packaging/appimage.Containerfile`) : les bibliothèques GTK embarquées sont liées à la glibc de la machine de construction, et le banc est en glibc 2.39. Voir [Annexe H](Annexes/Annexe_H_deploiement_banc.md).

Historique des entrées de démarrage de la clé (`packaging/live/grub.cfg`, dont les commentaires ne portent plus ni date ni modèle) : le gel au réveil constaté le 2026-11-02 sur un HP ProBook 455 G3 venait de `nomodeset`, pas de la racine sur la clé ; la veille du dé-frozen avec la racine sur la clé y a été vérifiée le 2026-11-05.

La CI GitHub Actions (`ci.yml`, `fuzz.yml`, `package.yml`) rejoue fmt, clippy, tests, `cargo audit`, shellcheck, fuzzing et construction des paquets ; détail dans le [README](../README.md#intégration-continue).

## Certificats d'exemple

- `docs/exemple_pdf.pdf` (un disque) et `docs/exemple_lot.pdf` (lot) illustrent le rendu. Pour générer un lot de démonstration signé avec une clé fixe, dans une langue donnée : `cargo run -p secmet-attest --example sample_certificate -- <dossier> [fr|en|de]`.

## Mise en production (réalisée, v0.4.0)

- [Annexe M](Annexes/Annexe_M_cle_live_lots.md) : clé USB SecMet (image `secmet-key_<version>_x86_64.img.xz` publiée), partition de données `SECMET-DATA`, lots persistants sur plusieurs laptops, manifeste de lot signé, horloge, nettoyage de la clé. Mot de passe du compte `secmet` : `secmet` par défaut (`build_rootfs.sh`, `SECMET_OPERATOR_PASSWORD`), demandé par l'invite du helper.
- [Annexe N](Annexes/Annexe_N_mises_a_jour.md) : mises à jour signées, releases GitHub du dépôt du code `hclcdev12/secmet` (depuis 0.3.5 ; `secmet-releases` y renvoie par `moved_to`), 14 fichiers par release (11 en 0.4.0). **Réalisé** : `packaging/release.sh`, module `secmet-attest::release`, binaire `secmet-release` (`keygen`, `sign`, `verify`), vérification dans l'application (`src/update.rs`, point jaune), bouton **Installer la mise à jour** puis **Relancer SecMet** (`src/update_install.rs`), magasin `app/<version>/` déployé au démarrage de la clé (`secmet-app-update`), SecMet Clé USB (Linux et Windows). Signature : en CI par `package.yml` (voie normale, au push d'un tag) avec le secret `SECMET_RELEASE_KEY` de l'environnement GitHub `release`, réservé au travail `publish` ; `release.sh` reste une chaîne locale de secours. Convention de version : un seul numéro X.Y.Z, tag annoté dont le corps devient les notes, `vX.Y.Z-rcN` construit sans publier.
- **Auto-contrôle du système de la clé (0.5.0, Annexe M §M.3)** : `secmet-attest::system_image` (`SYSTEM.sums` à chemins relatifs validés, signature par la clé de version sous l'étiquette `system`, contrôle par flux, fichiers en trop sous `EFI/` et `live/`, `IntegrityReport` : `ok`, `altered`, `unsigned`, `unavailable`, `error`, `pending`), `secmet-release sign-system` et `verify-system [--json]` ; empreintes écrites par `make_key_image.sh` (`packaging/live/system-sums.sh`), signées dans le travail `publish` (`packaging/live/seal_system.sh`), vérifiées par `refresh_live.sh` en reprise ; service `secmet-image-check` (overlay) ; côté application `src/system_integrity.rs` (réserve `erasure.caveats` dans `attestation_from_record`, à côté de celle de l'horloge, seulement avec un dossier de données dédié) et `src/ui/integrity_ui.rs` (bandeau rouge si altéré, rapport relu toutes les 3 s jusqu'au verdict). `sums_of` et `check_sums` lisent aussi par flux (`sha256_file_hex`). Limite : le vérificateur est dans ce qu'il vérifie.
- **Écarts connus (après 0.4.1)** : `secmet-first-boot` recrée encore une clé de signature au démarrage si `attestation-key.hex` disparaît (dans l'image de la clé, à reprendre avec sa reconstruction, phase 3) ; mot de passe `secmet` identique sur toutes les clés publiées (choix assumé).
- **Politique NIST (0.4.1)** : `nist-800-88r2` seule (`impl Default for Policy` et `PolicyId::default()`, `helper_client::APP_POLICY` envoyé à l'analyse comme à l'exécution) ; aucun choix de révision dans l'interface. `nist-800-88r1` (`taxonomy::LEGACY_POLICY_ID`) reste pour relire les anciens certificats et sessions (repli de `certificate/pdf.rs` pour un enregistrement sans job) et reste accepté par le protocole du helper. Les tests qui dépendent d'une politique la nomment (`nist_800_88_rev1()` ou `nist_800_88_rev2()`), jamais `Policy::default()`. Effet visible : Security Erase sur SSD et Format NVM user data sortent Clear.
- **Clé de signature des certificats (0.4.1)** : jamais recréée en silence. `keystore::state` (`Missing`, `Lost`, `Plain`), `load` sans création, `create(path, CreateMode)` explicite ; la `.pub` (ou `key-history.json`) témoigne qu'une clé a existé et `CreateMode::FirstKey` refuse alors. Sur un poste, seule la toute première clé se crée au premier certificat ; une clé perdue, ou absente sur la clé USB, ouvre le dialogue « Clé de signature absente » (`src/ui/signing_key_ui.rs`, au lancement et avant « Générer le PDF »), qui ne crée une clé (`CreateMode::Replace`, notée dans `key-history.json`) qu'à la demande de l'opérateur. Le contrôle avant clôture d'un lot lit la `.pub` (`signing_public_key_hex`). `secmet-first-boot` et `secmet-release keygen` (mode `Replace`) n'ont pas changé.

## Banc de test

Un banc Linux Mint 22 sert aux essais réels (disques SATA HDD/SSD). Les tests de bout en bout (effacement, certificat, AppImage au premier lancement) s'y font sur l'interface réelle ; les tests unitaires ne touchent jamais un disque.

Voir aussi le [README](../README.md) et les [annexes](Annexes/README.md).
