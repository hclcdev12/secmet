# SecMet

Application **Rust** (GTK4, v0.3.7) pour l'effacement sécurisé de disques sous Linux, conforme **NIST SP 800-88** : commandes firmware ATA/NVMe (sanitize, Security Erase, Format NVM) et écriture hôte vérifiée, réalisées nativement par un helper privilégié (aucun `hdparm`, `nvme-cli`, `smartctl`, `shred`, `lsblk` ni `blkid`). Pensée pour un usage en **atelier / banc de nettoyage** : détection des disques branchés, sélection multiple, effacements en parallèle avec suivi de progression.

> **Attention** — Cette application détruit définitivement les données des disques sélectionnés. Vérifiez toujours les cibles avant de lancer une suppression. Le disque système est protégé, mais une erreur d'identification reste possible sur d'autres supports.

> **Note** — L'ancien paquet `shred-graphique` est remplacé par `secmet`. Au premier lancement, les journaux et certificats sous `~/.shred_graphique_logs/` sont migrés automatiquement vers `~/.secmet_logs/` (fusion si les deux dossiers existent ; l'ancien dossier est renommé en `~/.shred_graphique_logs.bak`).

## Installation

Les versions publiées sont sur la page [Releases](https://github.com/hclcdev12/secmet/releases) : paquets `.deb` (Ubuntu 24.04 / Mint 22 / Debian 13 ; Ubuntu 22.04 / Mint 21 / Debian 12), `.rpm` (Fedora 42), AppImage, avec `SHA256SUMS` signé et `latest.json`. Une seule installation à la main ; ensuite SecMet se met à jour depuis ses Réglages (téléchargement, vérification de la signature, installation).

```bash
# Ubuntu 24.04, Linux Mint 22, Debian 13 (remplacer <version>)
sudo apt install ./secmet_<version>.ubuntu24.04_amd64.deb
# Ubuntu 22.04, Linux Mint 21, Debian 12
sudo apt install ./secmet_<version>.ubuntu22.04_amd64.deb
# Fedora 42
sudo dnf install ./secmet-<version>-1.fc42.x86_64.rpm
```

Autre distribution : l'AppImage (`chmod +x`, puis lancer ; au premier lancement un mot de passe administrateur installe le composant système). Le mot de passe demandé au démarrage est celui de votre compte : il protège les opérations sur les disques. Les disques effacés le sont définitivement : ne tester qu'avec des disques dédiés.

Chaque release est signée par la clé de version SecMet (identifiant `7854d9fe5a5e2be6`, clé publique Ed25519 `bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5`) ; `secmet-release verify <dossier> --trust <clé publique>` vérifie un téléchargement à la main. Mécanisme détaillé : [Annexe N](docs/Annexes/Annexe_N_mises_a_jour.md).

## Fonctionnalités

- Inventaire des disques par sysfs (transport, protections système, LVM/RAID tracés) puis identification firmware (IDENTIFY DEVICE / Identify Controller) via le helper, rafraîchissement à chaud
- Sélection multiple, effacements en parallèle (tous les disques cochés)
- **Lancer la suppression sécurisée** : plan décidé par le moteur `secmet-core` (Purge firmware d'abord, replis explicites, écriture hôte en dernier) ; HDD sans sanitize : choix des passes dans le dialogue de confirmation (défaut **1 passe + zéros**, NIST Clear ; option « historique » 3 passes)
- **Dé-frozen** : dialogue dédié (disques SATA gelés + mise en veille système)
- Disques **USB** listés : écriture hôte uniquement (aucune commande firmware à travers un pont USB), catégorie Clear
- Arrêt d'une opération en cours (bouton **Stop**)
- Élévation des privilèges via **PolicyKit** (`pkexec`) et helper dédié
- Consultation **SMART** (table ATA avec seuils, page Health NVMe) et vérification post-effacement par relecture d'échantillons (canaris ChaCha20) et recherche de signatures résiduelles
- **Certificat PDF** d'effacement rendu nativement en Rust (`secmet-attest`, sans Chromium) : une page par disque, catégorie NIST SP 800-88 (Purge / Clear), attestation JSON signée **Ed25519** et QR code Base45 vérifiable hors ligne avec `secmet-verify`
- Journalisation des opérations (fichiers JSON dans le répertoire utilisateur)
- Paquets **`.deb`** (Ubuntu 24.04 et 22.04) et **`.rpm`** (Fedora) pour installation système (TestDisk embarqué si absent du système)
- **AppImage** autonome (GTK4 et TestDisk embarqués, assemblée dans un conteneur Ubuntu 24.04) : un seul fichier, composant système installé au premier lancement
- **Réglages** : langue de l'interface et des certificats PDF (français, anglais, allemand), logo des certificats PDF, nom de l'opérateur pré-rempli, **mises à jour** (vérification quotidienne du canal de publication, signature contrôlée, point jaune sur l'engrenage, désactivable) ; **Déverrouiller** : levée d'un mot de passe ATA connu
- Bandeau d'avertissement quand la racine de l'hôte n'est pas rattachée à un disque local (média live, ZFS) et réserve correspondante sur le certificat

## Prérequis

### Communs (dev et paquet .deb)

| Composant | Rôle |
|-----------|------|
| Linux (Debian/Ubuntu pour `make deps`) | Plateforme cible |
| GTK 4 | Interface graphique |
| `policykit-1` / `pkexec` | Droits administrateur (un seul prompt par session) |
| `testdisk` | Validation post-effacement TestDisk (embarqué dans .deb/AppImage si absent du système) |
| `evince`, `okular` ou `atril` | Consultation des certificats PDF (recommandé) |

### Développement depuis les sources

| Composant | Rôle |
|-----------|------|
| Rust (stable) | Compilation |

### Paquet Debian (.deb)

Les dépendances `Depends` couvrent GTK et PolicyKit. `testdisk` est **recommandé** (installé par apt par défaut) et de toute façon **embarqué** sous `/usr/lib/secmet/bin/` : il est utilisé automatiquement si le binaire système est absent — l'installation `dpkg -i` directe fonctionne donc aussi.

## Installation rapide

### Depuis les sources (développement)

```bash
git clone <url-du-repo>
cd <répertoire-du-repo>

make deps    # installe et vérifie les dépendances (apt : GTK4, PolicyKit, testdisk)
make dev     # compile en release + installe sous /usr/local (sudo)
make run     # lance l'application
```

Rust est installé par **rustup** dans `~/.cargo/bin` (pas via `apt`). Ce n'est pas un changement de chemin du projet : **`make`** charge déjà `~/.cargo/env`, un terminal nu souvent non.

Après `make deps` :

- un nouveau terminal, ou `source ~/.bashrc` (le script ajoute la ligne si elle manquait) ;
- ou `source scripts/env.sh` depuis le dépôt ;
- ou `./scripts/cargo clean` au lieu de `cargo clean` ;
- en pratique : **`make clean`**, **`make dev`**, **`make test`** (recommandé).

Si `cargo` est introuvable :

```bash
source "$HOME/.cargo/env"
# ou
source scripts/env.sh
```

Installation dans un autre préfixe :

```bash
make dev PREFIX=$HOME/.local
make run PREFIX=$HOME/.local
```

### Paquet Debian

```bash
make deb
# Artefact : dist/secmet_<version>_<arch>.deb
# apt résout les Depends (testdisk, etc.) — préférer à dpkg -i seul
sudo apt install ./dist/secmet_*.deb
```

Si le paquet a déjà été installé avec `dpkg -i` et que des dépendances manquent :

```bash
sudo apt-get install -f
```

Dépendances runtime du paquet : `libgtk-4-1`, `policykit-1`. Recommandés : `testdisk` (embarqué dans le paquet s'il est présent sur la machine de build), `evince` / `okular` / `atril` (lecteur PDF). L'inventaire SMART, firmware et banc (DMI) est lu nativement par le helper : aucun outil externe.

### AppImage (sans dépendances apt)

L'AppImage embarque **GTK4** et **TestDisk** : un seul fichier à copier, rien à installer via apt.

```bash
make appimage
# Artefact : dist/SecMet_<version>_<arch>.AppImage
```

L'assemblage se fait **dans un conteneur Ubuntu 24.04** (`packaging/appimage.Containerfile`, podman ou docker requis) : les bibliothèques embarquées sont ainsi liées à la glibc 2.39, la plus ancienne supportée par le banc. Une AppImage assemblée directement sur une distribution plus récente (Fedora, Arch…) refuse de démarrer sur Mint 22 / Ubuntu 24.04 (`GLIBC_2.4x not found`) ; `SECMET_APPIMAGE_NATIVE=1 make appimage` force malgré tout un assemblage local.

Copier le fichier sur le banc (clé USB, partage…), le rendre exécutable et le lancer. Au **premier lancement**, l'application propose d'installer son **composant système** — helper privilégié (`/usr/bin/secmet-helper`), policy PolicyKit et TestDisk embarqué (`/usr/lib/secmet/`) — via `pkexec` : **un mot de passe**, une seule fois par machine (redemandé seulement si une AppImage plus récente embarque un helper différent). L'installeur, embarqué dans le binaire, extrait le composant depuis le **fichier** AppImage lui-même : le montage FUSE d'une AppImage n'est pas lisible par root.

Prérequis résiduels côté hôte : `pkexec` (PolicyKit) et FUSE 2 (`libfuse2`, présent sur Mint ; sinon `APPIMAGE_EXTRACT_AND_RUN=1`). Les certificats PDF ne dépendent d'aucun navigateur ; l'inventaire du banc est lu dans sysfs (DMI) par le helper.

Compatibilité : glibc ≥ 2.39 → **Ubuntu 24.04+**, **Debian 13+**, **Mint 22+** et lives récents.

## Déploiement du banc atelier

SecMet est déployé sur un **banc Linux** : paquet `.deb` (machine dédiée) ou **AppImage** (copier-lancer — idéale pour live USB/PXE, voir [Annexe H](docs/Annexes/Annexe_H_deploiement_banc.md)). Trois modes courants :

| Mode | Usage typique | Démontage du disque |
|------|---------------|---------------------|
| **Machine dédiée** | Atelier fixe, volume (référence R-01) | Souvent requis (disque branché au banc) |
| **Live USB** | Laptop / SSD interne **sur place** | **Non** — boot USB, effacement in situ |
| **PXE** | Parc homogène, effacement **sur place** | **Non** — boot réseau, effacement in situ |

Le banc dédié reste la **référence** pour le traitement en volume. Live USB et PXE permettent souvent de traiter un SSD interne **sans extraire** le disque du châssis — plus rapide qu'un démontage sur laptop. Le réseau PXE sert au **démarrage** du poste, pas à l'exécution de SecMet (application locale). Guide complet : [docs/Annexes/Annexe_H_deploiement_banc.md](docs/Annexes/Annexe_H_deploiement_banc.md).

## Utilisation

1. Lancer **SecMet** (menu applications ou `make run` / `secmet`).
2. Saisir le mot de passe administrateur **une fois** au démarrage (la fenêtre reste réactive pendant l'invite). Si la session PolicyKit expire, le mot de passe peut être redemandé au clic sur une action.
3. Cliquer sur **Rafraîchir** si un disque vient d'être branché.
4. Sélectionner les disques à effacer (case à cocher ou **Tout sélectionner**).
5. Vérifier que les cibles ne sont pas le disque système ni un support critique.
6. Lancer **la suppression sécurisée** et confirmer.

**Réglages** (icône engrenage dans la barre de titre) : langue de l'interface (français natif, anglais, allemand — appliquée immédiatement ; les certificats PDF générés ensuite sont rédigés dans la même langue, les journaux restent en français), logo des certificats PDF (image PNG/JPEG/GIF/BMP/WebP/TIFF convertie en JPEG sur fond blanc dans `~/.config/secmet/pdf-logo.jpg`, utilisée en haut à gauche de chaque page à la place du logo SecMet jusqu'au retour au logo par défaut) et nom de l'opérateur, pré-rempli dans le dialogue **Générer le PDF**. Section **Mises à jour** : version installée, dernière vérification, nouvelle version et ses notes si le canal en annonce une (`latest.json` signé par la clé de version intégrée, sinon ignoré), bouton **Vérifier maintenant** et lien vers la page de téléchargement ; la vérification automatique (une requête HTTPS par jour, sans donnée personnelle) se désactive par une case. Un **point jaune** sur l'engrenage signale une version plus récente. Les réglages sont persistants dans `~/.config/secmet/settings.json` (`check_updates`, `update_url` pour un miroir interne) ; `SECMET_UPDATE_URL` remplace l'adresse pour les essais.

**Dossier de données** : `SECMET_DATA_DIR=/chemin/absolu` place réglages, logo, clé d'attestation et état des mises à jour dans `/chemin/config/`, et journaux, rapports et certificats dans `/chemin/logs/`, au lieu de `~/.config/secmet/` et `~/.secmet_logs/`. C'est le mécanisme de la clé USB SecMet ([Annexe M](docs/Annexes/Annexe_M_cle_live_lots.md)) : les données suivent la clé d'un poste à l'autre, clé de signature comprise. Dans ce mode, SecMet travaille par **lots** : un lot regroupe les machines traitées successivement (dialogue au démarrage, bandeau, certificats générés automatiquement en fin d'effacement, journal chaîné), et sa clôture produit le certificat groupé, un manifeste signé et une copie sur la partition d'export `SECMET-EXP`.

**Vérifier un certificat** : `secmet-verify <certificat.json>` (ou `--qr "<texte scanné>"`) contrôle la signature Ed25519 et n'accepte que les clés de confiance : `--trust <clé publique ou key_id>`, variable `SECMET_TRUSTED_KEYS`, `/etc/secmet/trusted-keys`, `~/.config/secmet/attestation-key.pub`. La clé publique du banc est dans `~/.config/secmet/attestation-key.pub` ; distribuez-la aux vérificateurs. `--allow-untrusted` affiche le résultat pour une clé inconnue sans l'accepter comme preuve. `secmet-verify --manifest <lot>-manifest.json` vérifie le **manifeste signé d'un lot** (clé USB, [Annexe M](docs/Annexes/Annexe_M_cle_live_lots.md)) et le confronte aux attestations `.json` du même dossier : certificat manquant, modifié ou en trop → code 1.

**Déverrouiller** : un disque ATA verrouillé (ou portant un mot de passe actif) peut être libéré en saisissant le mot de passe connu du propriétaire (utilisateur ou maître) ; le helper déverrouille puis lève le mot de passe. Sans le bon mot de passe, le disque refuse et rien n'est modifié.

Pendant l'effacement : progression par disque relayée en temps réel par le helper (étape du plan, phase, **pourcentage, débit et ETA** pour l'écriture hôte ; pourcentage du disque pour un sanitize). Bouton **Stop** : annulation entre deux blocs ou deux sondages (un Format NVM ou un SECURITY ERASE UNIT en cours ne s'interrompt pas), autres disques grisés jusqu'à la fin.

Quand **tous** les jobs de la session sont terminés : **Générer le PDF** — saisir le nom de l'opérateur, cocher les disques, un **seul PDF multi-pages** est créé sous `~/.secmet_logs/certificates/`.

Exemples : [certificat d'un disque](docs/exemple_pdf.pdf) · [certificat de lot](docs/exemple_lot.pdf).

Guide détaillé : [docs/Annexes/Annexe_G_guide_utilisateur.md](docs/Annexes/Annexe_G_guide_utilisateur.md).

## Méthodes d'effacement et preuve

Le moteur `secmet-core` décide d'un **plan** par disque à partir des capacités relues par le helper (IDENTIFY DEVICE, Identify Controller, journal sanitize, protections système) :

1. **Purge firmware** d'abord : NVMe Sanitize (block erase, crypto erase), Format NVM ses=2/1, ATA SANITIZE (block erase, crypto scramble, overwrite sur HDD), SECURITY ERASE UNIT (mot de passe aléatoire enregistré durablement dans `/var/lib/secmet/ata-security/` avant usage, enhanced si supporté ; si l'effacement est interrompu, le scan suivant lève automatiquement ce mot de passe et nettoie le coffre).
2. **Canaris** : avant toute commande firmware, un motif unique par bloc (ChaCha20, seed de session) est écrit sur un échantillon de 1 000 blocs ; après, aucun ne doit subsister. Un firmware qui acquitte sans effacer est pris en défaut.
3. **Replis explicites** : chaque méthode écartée (frozen, mot de passe tiers, pont USB) est consignée avec sa consigne ; si la meilleure méthode retenue est Clear alors qu'un Purge existait, le plan le dit (dégradation).
4. **Écriture hôte** en dernier (`O_EXCL | O_DIRECT`) : passes pseudo-aléatoires recomputables par LBA puis zéros, validées par relecture d'échantillons ; un contrôleur paresseux est détecté.
5. **Signatures résiduelles** : table de partitions et systèmes de fichiers recherchés en tête et en queue de disque après chaque étape.

Le certificat indique la **méthode réellement obtenue** et sa **catégorie NIST SP 800-88** (Purge, Clear, ou non classé en cas d'échec), avec le seed, le nombre de blocs relus et les anomalies.

### Correspondance NIST SP 800-88 Rev. 1

| Méthode | Niveau NIST | Couverture |
|---|---|---|
| NVMe / ATA Sanitize, Format NVM ses=2/1, Security Erase (HDD, enhanced sur SSD) | **Purge** | Espace adressable + zones cachées (over-provisioning, cellules réallouées) |
| Écriture hôte (1 ou 3 passes + zéros), Security Erase normal sur SSD | **Clear** | Espace adressable uniquement |
| TRIM seul | — | Jamais retenu comme méthode |

Référence : NIST SP 800-88 Rev. 1, *Guidelines for Media Sanitization* (déc. 2014) — <https://doi.org/10.6028/nist.sp.800-88r1>. Une politique conservatrice (lecture Rev. 2 / IEEE 2883, Format user-data erase et Security Erase SSD en Clear) est disponible dans le moteur.

### Limitations

- **USB** : aucune commande firmware n'est envoyée à travers un pont USB (acquittements non fiables) ; écriture hôte, catégorie Clear.
- **Frozen (SATA)** : bouton **Dé-frozen** (veille système via le helper, disque ré-identifié à la reprise) ou hot-plug / AHCI.
- **Format NVM / SECURITY ERASE UNIT** : bloquants dans le pilote, non interruptibles ; le sanitize et l'écriture hôte s'interrompent entre deux sondages ou deux blocs.
- **HDD** : SANITIZE OVERWRITE ou Security Erase donnent Purge quand le disque les supporte ; sinon écriture hôte (défaut **1 passe + zéros**, option « historique » 3 passes, mémorisée dans `~/.config/secmet/settings.json`).

Chaque session génère un journal lisible et un rapport JSON dans `~/.secmet_logs/`, et chaque disque un rapport détaillé des étapes.

## Dépannage rapide

| Symptôme | Action |
|----------|--------|
| Avertissement / refus « pkexec sans bit setuid » au démarrage | `make fix-pkexec` (ou contacter l'admin système) ; **pas de repli sudo** — l'application refuse de démarrer (R-12) |
| Échec d'ouverture du PDF | Installer un lecteur PDF (`evince`, `okular`, `atril`) ; la génération elle-même n'a aucune dépendance externe |
| Anciens journaux introuvables après mise à jour | Vérifier `~/.secmet_logs/` ; ancien dossier sauvegardé en `~/.shred_graphique_logs.bak` |
| Disque USB | Écriture hôte uniquement (Clear) ; voir Limitations ci-dessus et [docs/Annexes/Annexe_G_guide_utilisateur.md](docs/Annexes/Annexe_G_guide_utilisateur.md) |

Après installation ou mise à jour de la policy Polkit : `make dev` puis `make verify-polkit`.

## Commandes Make

| Commande | Description |
|----------|-------------|
| `make help` | Aide des cibles |
| `make deps` | Dépendances système + Rust (vérifie, installe si besoin) |
| `make dev` | Build release + installation locale (binaires + polkit) |
| `make run` | Lance l'app installée sous `PREFIX` |
| `make build` | Compile en release sans installation |
| `make release` | Chaîne de publication : contrôles, 4 paquets construits et fumés en conteneurs, `SHA256SUMS` et `latest.json` signés avec la clé de version ; rien n'est publié |
| `make publish` | Comme `make release`, puis publication de la release GitHub (ce dépôt) après confirmation ; en pratique, le push d'un tag `vX.Y.Z` fait la même chose par `package.yml` |
| `make test` | Tests unitaires Rust |
| `make deb` | Construit le paquet `.deb` (`SECMET_DEB_DISTRO=ubuntu22.04` ajoute un suffixe de version) |
| `make install-deb` | Installe le dernier `.deb` de `dist/` via apt |
| `make appimage` | Construit l'AppImage dans un conteneur Ubuntu 24.04 (podman/docker) |
| `make rpm` | Construit le paquet `.rpm` (Fedora : rpm-build, gtk4-devel, testdisk) |
| `make live` | Construit l'image de la clé USB SecMet dans un conteneur privilégié Ubuntu 24.04 → `dist/live/` ([packaging/live/README.md](packaging/live/README.md)) |
| `make live-key DEV=/dev/sdX` | Écrit une clé USB SecMet (root ; efface la clé) |
| `make fuzz-check` | Compile les cibles de fuzz (`fuzz/`) sans instrumentation |
| `make clean` | Nettoie `target/` (via rustup, pas besoin de `cargo` dans le PATH) |
| `make fix-pkexec` | Restaure le bit setuid de `pkexec` (PolicyKit) |
| `make verify-polkit` | Vérifie pkexec, helper et policy Polkit (cohérence des chemins) |

## Intégration continue (GitHub Actions)

| Workflow | Déclencheur | Contenu |
|----------|-------------|---------|
| `ci.yml` | push / PR sur `main` et `I/O_SecMet` | `cargo fmt`, `clippy -D warnings`, tests du workspace et des crates sans GTK, `cargo audit`, shellcheck des scripts, compilation des cibles de fuzz |
| `fuzz.yml` | push / PR touchant `crates/` ou `fuzz/` (60 s par cible), hebdomadaire (30 min), manuel (durée au choix) | cargo-fuzz (nightly, libFuzzer) sur neuf décodeurs purs : sense ATA, IDENTIFY, SMART, NVMe, trames IPC, QR et JSON d'attestation, mountinfo, canaris ; entrées fautives publiées en artefact |
| `package.yml` | push sur `main`, tags `vX.Y.Z`, manuel | `.deb` Ubuntu 24.04 et 22.04 (version `0.3.5~ubuntu22.04`, publiée sous `secmet_0.3.5.ubuntu22.04_amd64.deb`), `.rpm` Fedora 42, AppImage ; chaque paquet installé et fumé dans son conteneur ; release GitHub avec `SHA256SUMS` sur un tag |

## Publication d'une version

La publication ne dépend pas de ces workflows : elle tourne sur le poste du responsable des versions (`packaging/release.sh`, podman ou docker requis) et signe les paquets avec une **clé de version** gardée hors ligne, distincte des clés d'attestation des certificats. Détail dans l'[Annexe N](docs/Annexes/Annexe_N_mises_a_jour.md).

```bash
secmet-release keygen ~/.config/secmet/release-key.hex   # une fois ; sauvegarder la clé hors ligne
git tag -a v0.3.2 -m "SecMet 0.3.2" && git push origin main v0.3.2
make release      # dist/release/0.3.2/ : paquets, SHA256SUMS, SHA256SUMS.sig, latest.json
make publish      # release GitHub sur https://github.com/hclcdev12/secmet (confirmation)
secmet-release verify dist/release/0.3.2 --trust <clé publique ou key_id>
```

`SECMET_RELEASE_KEY`, `SECMET_RELEASES_REPO` et `SECMET_RELEASES_URL` remplacent les valeurs par défaut. L'application lit `https://github.com/<dépôt>/releases/latest/download/latest.json` et n'accepte que les fichiers signés par une clé listée dans `src/update.rs` (`RELEASE_PUBLIC_KEYS`). L'application annonce les versions mais n'installe rien : la section Mises à jour renvoie à la page de téléchargement, et l'installation suit la voie normale du livrable (`apt`, `dnf`, ou remplacement de l'AppImage).

Les cibles de fuzz vivent dans `fuzz/` (crate hors workspace) : `cargo +nightly fuzz run ata_sense` en local après `cargo install cargo-fuzz`.

## Architecture (aperçu)

```
src/
  lib.rs               # Bibliothèque (modules partagés)
  main.rs              # Point d'entrée, migration, vérifications outils
  ui/                  # Interface GTK4 (sous-modules)
  helper_client.rs     # Client du daemon privilégié (socket Unix, NDJSON, événements)
  disk.rs              # Modèle disque alimenté par le helper (inventaire + scan)
  disk_info/           # Panneau SMART (table ATA, Health NVMe) depuis le scan
  verification/        # Analyse de la sortie TestDisk
  certificate/         # Session, attestation, libellés, ouverture des PDF
  i18n/                # Traductions de l'interface (fr natif, en, de)
  settings.rs          # ~/.config/secmet/settings.json (passes HDD, langue, logo, opérateur, mises à jour)
  update.rs            # Vérification des mises à jour : latest.json signé, clé de version intégrée, état
  pdf_logo.rs          # Conversion et stockage du logo personnalisé des certificats
  system_install.rs    # AppImage : installation du composant système au premier lancement
  paths.rs             # Chemin du helper (env SECMET_HELPER)
  migrate.rs           # Migration ~/.shred_graphique_logs → ~/.secmet_logs

crates/
  secmet-device/       # Accès matériel : SG_IO ATA, ioctl NVMe, sysfs, protections, signatures
  secmet-core/         # Moteur de décision NIST SP 800-88 et canaris (pur)
  secmet-helper/       # Daemon privilégié : sessions, ouverture O_EXCL anti-TOCTOU, exécution des plans
  secmet-attest/       # Certificats : attestation signée Ed25519, QR Base45, PDF natif, secmet-verify ;
                       # versions publiées signées (module release, binaire secmet-release)

scripts/
  deps.sh              # Installation des dépendances (apt)
  install-dev.sh       # Installation locale pour le dev

packaging/
  stage-tree.sh        # Arborescence commune des paquets (binaires, policy, docs, TestDisk)
  build_deb.sh         # Construction du .deb
  build_rpm.sh         # Construction du .rpm (spec : secmet.spec)
  build_appimage.sh    # Construction de l'AppImage (délègue au conteneur)
  release.sh           # Chaîne de publication : contrôles, paquets en conteneurs, signature, publication
  live/                # Clé USB SecMet : image live (build_live.sh, build_rootfs.sh, overlay/) et secmet-make-key.sh
  appimage.Containerfile  # Image de construction Ubuntu 24.04
  install-helper.sh    # Installeur du composant système (embarqué dans le binaire)
  secmet.policy        # Règle PolicyKit
```

En production, le helper est installé sous `/usr/bin/secmet-helper`. En développement, `make dev` l'installe sous `PREFIX` (défaut `/usr/local`) et `make run` définit `SECMET_HELPER` en conséquence.

Autres dossiers : `fuzz/` (cibles cargo-fuzz, hors workspace), `docs/` (annexes, certificats d'exemple).

Documentation développeur : [docs/DEV_CONTEXT.md](docs/DEV_CONTEXT.md).

## Sécurité

- Les opérations sur disques passent par **pkexec** et le daemon `secmet-helper serve` déclaré dans une policy Polkit : un seul prompt par session, jeton par disque lié au numéro de périphérique, au numéro de série et à l'empreinte firmware, ouverture exclusive et ré-identification avant toute écriture.
- Le disque hébergeant la racine du système est **refusé** pour l'effacement (UI et helper).
- Sans `pkexec` setuid, l'application **refuse de démarrer** (pas de repli `sudo`).
- Les effacements sont **irréversibles** ; le bouton Stop interrompt une écriture hôte ou un sanitize suivi par sondage, jamais un Format NVM ou un SECURITY ERASE UNIT déjà lancé.

## Licence

[GNU GPL v3 ou ultérieure](LICENSE) — Copyright (c) 2026 SecMet Contributors

Le code est libre : toute version redistribuée, modifiée ou intégrée doit rester sous GPL avec ses sources. Le nom **SecMet** et le lion du logo sont des marques : ils identifient le logiciel officiel et ne sont pas couverts par la GPL (voir [content/LICENSE](content/LICENSE)) ; une version modifiée doit être renommée.

SecMet s'appuie sur des composants open source (GTK 4, crates Rust, TestDisk embarqué). Détail des licences tierces et conformité distribution : [docs/Annexes/Annexe_J_licences_tierces.md](docs/Annexes/Annexe_J_licences_tierces.md). Annexes : [docs/Annexes/README.md](docs/Annexes/README.md).

Le module d’infos disque SMART s’inspire conceptuellement de [QDiskInfo](https://github.com/edisionnano/QDiskInfo) (GPL-3.0) ; la logique et l’UI sont une réécriture indépendante en Rust/GTK4, sans inclusion de sources QDiskInfo.
