<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="content/SecMet_logo_dark.svg">
    <img src="content/SecMet_logo.svg" alt="SecMet" width="360">
  </picture>
</p>

<p align="center"><b>Effacement sécurisé de disques sous Linux, conforme NIST SP 800-88, avec certificat signé.</b></p>

<p align="center">
  <a href="https://github.com/hclcdev12/secmet/releases/latest"><img alt="Dernière version" src="https://img.shields.io/github/v/release/hclcdev12/secmet?label=version"></a>
  <a href="https://github.com/hclcdev12/secmet/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hclcdev12/secmet/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="Licence GPL-3.0-or-later" src="https://img.shields.io/badge/licence-GPL--3.0--or--later-blue"></a>
</p>

<p align="center">
  <a href="#installation">Installer</a> ·
  <a href="#utilisation">Utiliser</a> ·
  <a href="#déploiement-en-atelier">Déployer en atelier</a> ·
  <a href="docs/Annexes/Annexe_M_cle_live_lots.md">Clé USB</a> ·
  <a href="#développement">Développer</a> ·
  <a href="docs/Annexes/README.md">Annexes</a>
</p>

---

SecMet est une application **Rust** (GTK4) pour l'effacement sécurisé de disques sous Linux. Les commandes firmware ATA/NVMe (sanitize, Security Erase, Format NVM) et l'écriture hôte vérifiée sont réalisées nativement par un helper privilégié, sans `hdparm`, `nvme-cli`, `smartctl`, `shred`, `lsblk` ni `blkid`. Elle est pensée pour un **atelier ou un banc de nettoyage** : détection des disques branchés, sélection multiple, effacements en parallèle avec suivi de progression, certificat signé par disque et par lot.

> **Attention** — Cette application détruit définitivement les données des disques sélectionnés. Vérifiez toujours les cibles avant de lancer une suppression. Le disque système est protégé, mais une erreur d'identification reste possible sur d'autres supports.

## Ce que fait SecMet

**Effacement**

- Inventaire des disques par sysfs (transport, protections système, LVM/RAID tracés) puis identification firmware (IDENTIFY DEVICE / Identify Controller) via le helper, rafraîchissement à chaud.
- Sélection multiple, effacements en parallèle de tous les disques cochés, arrêt d'une opération en cours (bouton **Stop**).
- Plan décidé par le moteur `secmet-core` : Purge firmware d'abord, replis explicites, écriture hôte en dernier. HDD sans sanitize : choix des passes dans le dialogue de confirmation (défaut **1 passe + zéros**, NIST Clear ; option « historique » 3 passes).
- **Dé-frozen** : dialogue dédié pour les disques SATA gelés (mise en veille système). **Déverrouiller** : levée d'un mot de passe ATA connu.
- Disques **USB** listés : écriture hôte uniquement (aucune commande firmware à travers un pont USB), catégorie Clear.
- Élévation des privilèges via **PolicyKit** (`pkexec`) et helper dédié ; un seul mot de passe par session.

**Preuve**

- Canaris ChaCha20 écrits avant chaque commande firmware et relus après, pour prendre en défaut un firmware qui acquitte sans effacer.
- Consultation **SMART** (table ATA avec seuils, page Health NVMe), vérification post-effacement par relecture d'échantillons et recherche de signatures résiduelles.
- **Certificat PDF** rendu nativement en Rust (`secmet-attest`, sans Chromium) : une page par disque, catégorie NIST SP 800-88 (Purge / Clear), attestation JSON signée **Ed25519** et QR code Base45 vérifiable hors ligne avec `secmet-verify`. Chaque certificat porte le **sort du support** décidé par le moteur (réemploi, destruction recommandée ou requise, à relancer) ; le PDF de lot ajoute un bordereau de destruction physique.
- Journalisation des opérations en JSON dans le répertoire utilisateur ; bandeau d'avertissement et réserve sur le certificat quand la racine de l'hôte n'est pas rattachée à un disque local (média live, ZFS).

**Atelier**

- **Lots** sur la clé USB SecMet : les machines traitées successivement sont regroupées, certificats générés automatiquement, journal chaîné, clôture avec certificat groupé et manifeste signé, copie sur la partition d'export.
- **Réglages** : langue de l'interface et des certificats (français, anglais, allemand), logo des certificats PDF, nom de l'opérateur pré-rempli, **mises à jour** vérifiées quotidiennement avec signature contrôlée (désactivable).

**Distribution**

- Paquets **`.deb`** (Ubuntu 24.04 et 22.04) et **`.rpm`** (Fedora), TestDisk embarqué si absent du système.
- **AppImage** autonome (GTK4 et TestDisk embarqués), composant système installé au premier lancement.
- **Image de clé USB** amorçable, avec mise à jour de l'application depuis la clé elle-même.

## Installation

### Paquets publiés

Les versions publiées sont sur la page [Releases](https://github.com/hclcdev12/secmet/releases) : paquets `.deb` (Ubuntu 24.04 / Mint 22 / Debian 13 ; Ubuntu 22.04 / Mint 21 / Debian 12), `.rpm` (Fedora 42), AppImage, avec `SHA256SUMS` signé et `latest.json`. Une seule installation à la main ; ensuite SecMet se met à jour depuis ses Réglages (téléchargement, vérification de la signature, installation).

```bash
# Ubuntu 24.04, Linux Mint 22, Debian 13 (remplacer <version>)
sudo apt install ./secmet_<version>.ubuntu24.04_amd64.deb
# Ubuntu 22.04, Linux Mint 21, Debian 12
sudo apt install ./secmet_<version>.ubuntu22.04_amd64.deb
# Fedora 42
sudo dnf install ./secmet-<version>-1.fc42.x86_64.rpm
```

Dépendances des paquets : `libgtk-4-1` et `policykit-1` (résolues par `apt` ou `dnf`, d'où la préférence sur `dpkg -i` seul ; après un `dpkg -i`, `sudo apt-get install -f` complète). Recommandés : `testdisk` (de toute façon embarqué sous `/usr/lib/secmet/bin/` et utilisé si le binaire système est absent) et un lecteur PDF (`evince`, `okular` ou `atril`). L'inventaire SMART, firmware et banc (DMI) est lu nativement par le helper : aucun outil externe.

### AppImage

Pour toute autre distribution. L'AppImage embarque **GTK4** et **TestDisk** : un seul fichier à copier, rien à installer via apt. Prérequis côté hôte : `pkexec` (PolicyKit) et FUSE 2 (`libfuse2`, présent sur Mint ; sinon `APPIMAGE_EXTRACT_AND_RUN=1`). Compatibilité : glibc ≥ 2.39, soit **Ubuntu 24.04+**, **Debian 13+**, **Mint 22+** et lives récents.

```bash
chmod +x SecMet_<version>_x86_64.AppImage
./SecMet_<version>_x86_64.AppImage
```

Au **premier lancement**, l'application propose d'installer son **composant système** (helper privilégié `/usr/bin/secmet-helper`, policy PolicyKit, TestDisk embarqué sous `/usr/lib/secmet/`) via `pkexec` : **un mot de passe**, une seule fois par machine, redemandé seulement si une AppImage plus récente embarque un helper différent. L'installeur, embarqué dans le binaire, extrait le composant depuis le **fichier** AppImage lui-même, car le montage FUSE d'une AppImage n'est pas lisible par root.

### Vérifier une version téléchargée

Chaque release est signée par la clé de version SecMet (identifiant `7854d9fe5a5e2be6`, clé publique Ed25519 `bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5`), distincte des clés d'attestation des certificats.

```bash
secmet-release verify <dossier des fichiers téléchargés> --trust <clé publique ou key_id>
```

Mécanisme détaillé des mises à jour : [Annexe N](docs/Annexes/Annexe_N_mises_a_jour.md).

## Utilisation

Le mot de passe demandé au démarrage est celui de votre compte : il protège les opérations sur les disques. Les disques effacés le sont définitivement : ne tester qu'avec des disques dédiés.

### Premiers pas

1. Lancer **SecMet** (menu applications ou `secmet`).
2. Saisir le mot de passe administrateur **une fois** au démarrage (la fenêtre reste réactive pendant l'invite). Si la session PolicyKit expire, le mot de passe peut être redemandé au clic sur une action.
3. Cliquer sur **Rafraîchir** si un disque vient d'être branché.
4. Sélectionner les disques à effacer (case à cocher ou **Tout sélectionner**).
5. Vérifier que les cibles ne sont pas le disque système ni un support critique.
6. Lancer **la suppression sécurisée** et confirmer.

### Pendant l'effacement

La progression de chaque disque est relayée en temps réel par le helper : étape du plan, phase, **pourcentage, débit et ETA** pour l'écriture hôte, pourcentage du disque pour un sanitize. Le bouton **Stop** annule entre deux blocs ou deux sondages (un Format NVM ou un SECURITY ERASE UNIT en cours ne s'interrompt pas) ; les autres disques restent grisés jusqu'à la fin.

### Certificat PDF

Quand **tous** les jobs de la session sont terminés : **Générer le PDF**. Saisir le nom de l'opérateur, cocher les disques ; un **seul PDF multi-pages** est créé sous `~/.secmet_logs/certificates/`, avec l'attestation JSON signée à côté. Exemples : [certificat d'un disque](docs/exemple_pdf.pdf) · [certificat de lot](docs/exemple_lot.pdf).

### Réglages

Icône engrenage dans la barre de titre.

- **Langue** de l'interface : français natif, anglais, allemand, appliquée immédiatement. Les certificats PDF générés ensuite sont rédigés dans la même langue ; les journaux restent en français.
- **Logo des certificats** : image PNG, JPEG, GIF, BMP, WebP ou TIFF, convertie en JPEG sur fond blanc dans `~/.config/secmet/pdf-logo.jpg` et placée en haut à gauche de chaque page jusqu'au retour au logo par défaut.
- **Opérateur** : nom pré-rempli dans le dialogue **Générer le PDF**.
- **Mises à jour** : version installée, dernière vérification, nouvelle version et ses notes si le canal en annonce une (`latest.json` signé par la clé de version intégrée, sinon ignoré), bouton **Vérifier maintenant** et lien vers la page de téléchargement. La vérification automatique (une requête HTTPS par jour, sans donnée personnelle) se désactive par une case. Un **point jaune** sur l'engrenage signale une version plus récente.

Les réglages sont persistants dans `~/.config/secmet/settings.json` (`check_updates`, `update_url` pour un miroir interne) ; `SECMET_UPDATE_URL` remplace l'adresse pour les essais.

### Lots et clé USB

`SECMET_DATA_DIR=/chemin/absolu` place réglages, logo, clé d'attestation et état des mises à jour dans `/chemin/config/`, et journaux, rapports et certificats dans `/chemin/logs/`, au lieu de `~/.config/secmet/` et `~/.secmet_logs/`. C'est le mécanisme de la clé USB SecMet : les données suivent la clé d'un poste à l'autre, clé de signature comprise.

Dans ce mode, SecMet travaille par **lots** : un lot regroupe les machines traitées successivement (dialogue au démarrage, bandeau, certificats générés automatiquement en fin d'effacement, journal chaîné). Sa clôture produit le certificat groupé, un manifeste signé et une copie sur la partition d'export `SECMET-EXP` ; un lot sans disque effacé peut être abandonné. Les Réglages de la clé ajoutent la recopie des lots clôturés sur la partition d'export et le nettoyage de la clé. Tout est décrit dans l'[Annexe M](docs/Annexes/Annexe_M_cle_live_lots.md).

### Vérifier un certificat

`secmet-verify <certificat.json>` (ou `--qr "<texte scanné>"`) contrôle la signature Ed25519 et n'accepte que les clés de confiance : `--trust <clé publique ou key_id>`, variable `SECMET_TRUSTED_KEYS`, `/etc/secmet/trusted-keys`, `~/.config/secmet/attestation-key.pub`. La clé publique du banc est dans `~/.config/secmet/attestation-key.pub` ; distribuez-la aux vérificateurs. `--allow-untrusted` affiche le résultat pour une clé inconnue sans l'accepter comme preuve.

`secmet-verify --manifest <lot>-manifest.json` vérifie le **manifeste signé d'un lot** et le confronte aux attestations `.json` du même dossier : certificat manquant, modifié ou en trop → code 1.

### Déverrouiller

Un disque ATA verrouillé, ou portant un mot de passe actif, peut être libéré en saisissant le mot de passe connu du propriétaire (utilisateur ou maître) : le helper déverrouille puis lève le mot de passe. Sans le bon mot de passe, le disque refuse et rien n'est modifié.

Guide détaillé : [Annexe G, guide utilisateur](docs/Annexes/Annexe_G_guide_utilisateur.md).

## Déploiement en atelier

SecMet se déploie sur un **banc Linux** : paquet `.deb` sur une machine dédiée, AppImage à copier-lancer, ou **clé USB SecMet** amorçable. Trois modes courants :

| Mode | Usage typique | Démontage du disque |
|------|---------------|---------------------|
| **Machine dédiée** | Atelier fixe, volume (référence R-01) | Souvent requis (disque branché au banc) |
| **Live USB** | Laptop / SSD interne **sur place** | **Non** — boot USB, effacement in situ |
| **PXE** | Parc homogène, effacement **sur place** | **Non** — boot réseau, effacement in situ |

Le banc dédié reste la **référence** pour le traitement en volume. Live USB et PXE permettent souvent de traiter un SSD interne **sans extraire** le disque du châssis, plus vite qu'un démontage sur laptop. Le réseau PXE sert au **démarrage** du poste, pas à l'exécution de SecMet, qui reste une application locale.

- Guide de déploiement : [Annexe H](docs/Annexes/Annexe_H_deploiement_banc.md).
- Clé USB SecMet (image live, lots, partition d'export, mises à jour depuis la clé) : [Annexe M](docs/Annexes/Annexe_M_cle_live_lots.md) et [packaging/live/README.md](packaging/live/README.md).

## Méthodes d'effacement et preuve

Le moteur `secmet-core` décide d'un **plan** par disque à partir des capacités relues par le helper (IDENTIFY DEVICE, Identify Controller, journal sanitize, protections système) :

1. **Purge firmware** d'abord : NVMe Sanitize (block erase, crypto erase), Format NVM ses=2/1, ATA SANITIZE (block erase, crypto scramble, overwrite sur HDD), SECURITY ERASE UNIT (mot de passe aléatoire enregistré durablement dans `/var/lib/secmet/ata-security/` avant usage, enhanced si supporté ; si l'effacement est interrompu, le scan suivant lève automatiquement ce mot de passe et nettoie le coffre).
2. **Canaris** : avant toute commande firmware, un motif unique par bloc (ChaCha20, seed de session) est écrit sur un échantillon de 1 000 blocs ; après, aucun ne doit subsister. Un firmware qui acquitte sans effacer est pris en défaut.
3. **Replis explicites** : chaque méthode écartée (frozen, mot de passe tiers, pont USB) est consignée avec sa consigne ; si la meilleure méthode retenue est Clear alors qu'un Purge existait, le plan le dit (dégradation).
4. **Écriture hôte** en dernier (`O_EXCL | O_DIRECT`) : passes pseudo-aléatoires recomputables par LBA puis zéros, validées par relecture d'échantillons ; un contrôleur paresseux est détecté.
5. **Signatures résiduelles** : table de partitions et systèmes de fichiers recherchés en tête et en queue de disque après chaque étape.

Le certificat indique la **méthode réellement obtenue** et sa **catégorie NIST SP 800-88** (Purge, Clear, ou non classé en cas d'échec), avec le seed, le nombre de blocs relus et les anomalies. Chaque session génère un journal lisible et un rapport JSON dans `~/.secmet_logs/`, et chaque disque un rapport détaillé des étapes.

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

## Dépannage

| Symptôme | Action |
|----------|--------|
| Avertissement / refus « pkexec sans bit setuid » au démarrage | `make fix-pkexec` (ou contacter l'admin système) ; **pas de repli sudo**, l'application refuse de démarrer (R-12) |
| Échec d'ouverture du PDF | Installer un lecteur PDF (`evince`, `okular`, `atril`) ; la génération elle-même n'a aucune dépendance externe |
| Dépendances manquantes après un `dpkg -i` | `sudo apt-get install -f` |
| Disque USB | Écriture hôte uniquement (Clear) ; voir [Limitations](#limitations) et l'[Annexe G](docs/Annexes/Annexe_G_guide_utilisateur.md) |
| Anciens journaux introuvables après mise à jour | L'ancien paquet `shred-graphique` est remplacé par `secmet` ; au premier lancement, `~/.shred_graphique_logs/` est migré vers `~/.secmet_logs/` (fusion si les deux existent, ancien dossier renommé en `~/.shred_graphique_logs.bak`) |

Après installation ou mise à jour de la policy Polkit : `make dev` puis `make verify-polkit`.

## Développement

### Prérequis

| Composant | Rôle |
|-----------|------|
| Linux (Debian/Ubuntu pour `make deps`) | Plateforme cible |
| Rust (stable, via rustup) | Compilation |
| GTK 4 | Interface graphique |
| `policykit-1` / `pkexec` | Droits administrateur |
| `testdisk` | Validation post-effacement (embarqué dans les paquets s'il est présent sur la machine de build) |
| podman ou docker | Paquets, AppImage et image live construits en conteneurs |

### Depuis les sources

```bash
git clone https://github.com/hclcdev12/secmet.git
cd secmet

make deps    # installe et vérifie les dépendances (apt : GTK4, PolicyKit, testdisk ; Rust par rustup)
make dev     # compile en release + installe sous /usr/local (sudo)
make run     # lance l'application
```

Rust est installé par **rustup** dans `~/.cargo/bin`, pas via `apt`. **`make`** charge déjà `~/.cargo/env`, un terminal nu souvent non : après `make deps`, ouvrir un nouveau terminal ou `source ~/.bashrc` (le script ajoute la ligne si elle manquait), ou `source scripts/env.sh` depuis le dépôt. En pratique : **`make clean`**, **`make dev`**, **`make test`**.

Installation dans un autre préfixe : `make dev PREFIX=$HOME/.local` puis `make run PREFIX=$HOME/.local`. En production, le helper est installé sous `/usr/bin/secmet-helper` ; en développement, `make dev` l'installe sous `PREFIX` et `make run` définit `SECMET_HELPER` en conséquence.

### Commandes Make

| Commande | Description |
|----------|-------------|
| `make help` | Aide des cibles |
| `make deps` | Dépendances système + Rust (vérifie, installe si besoin) |
| `make dev` | Build release + installation locale (binaires + polkit) |
| `make run` | Lance l'app installée sous `PREFIX` |
| `make build` | Compile en release sans installation |
| `make test` | Tests unitaires Rust |
| `make deb` | Construit le paquet `.deb` (`SECMET_DEB_DISTRO=ubuntu22.04` ajoute un suffixe de version) |
| `make install-deb` | Installe le dernier `.deb` de `dist/` via apt |
| `make rpm` | Construit le paquet `.rpm` (Fedora : rpm-build, gtk4-devel, testdisk) |
| `make appimage` | Construit l'AppImage dans un conteneur Ubuntu 24.04 |
| `make live` | Construit l'image de la clé USB SecMet dans un conteneur privilégié Ubuntu 24.04 → `dist/live/` ([packaging/live/README.md](packaging/live/README.md)) |
| `make live-bundle` | Livrable de la clé USB `dist/secmet-live_<version>_x86_64.tar` (après `make live`) |
| `make live-check` | Contrôle QEMU sans écran : la clé virtuelle déploie la version la plus récente de son magasin `app/` |
| `make live-key DEV=/dev/sdX` | Écrit une clé USB SecMet (root ; efface la clé) |
| `make release` | Chaîne de publication : contrôles, 4 paquets construits et fumés en conteneurs, `SHA256SUMS` et `latest.json` signés avec la clé de version ; rien n'est publié |
| `make publish` | Comme `make release`, puis publication de la release GitHub après confirmation |
| `make fuzz-check` | Compile les cibles de fuzz (`fuzz/`) sans instrumentation |
| `make clean` | Nettoie `target/` (via rustup, pas besoin de `cargo` dans le PATH) |
| `make fix-pkexec` | Restaure le bit setuid de `pkexec` (PolicyKit) |
| `make verify-polkit` | Vérifie pkexec, helper et policy Polkit (cohérence des chemins) |

### Paquets

- **`.deb`** : `make deb` produit `dist/secmet_<version>_<arch>.deb` ; `sudo apt install ./dist/secmet_*.deb` résout les `Depends`. Les dépendances couvrent GTK et PolicyKit ; `testdisk` est recommandé et de toute façon embarqué sous `/usr/lib/secmet/bin/`, donc `dpkg -i` seul fonctionne aussi.
- **`.rpm`** : `make rpm` (spec `packaging/secmet.spec`).
- **AppImage** : `make appimage` produit `dist/SecMet_<version>_<arch>.AppImage`. L'assemblage se fait **dans un conteneur Ubuntu 24.04** (`packaging/appimage.Containerfile`) : les bibliothèques embarquées sont ainsi liées à la glibc 2.39, la plus ancienne supportée par le banc. Une AppImage assemblée directement sur une distribution plus récente (Fedora, Arch…) refuse de démarrer sur Mint 22 / Ubuntu 24.04 (`GLIBC_2.4x not found`) ; `SECMET_APPIMAGE_NATIVE=1 make appimage` force malgré tout un assemblage local.
- **Clé USB** : `make live` puis `make live-key DEV=/dev/sdX` ; détail dans [packaging/live/README.md](packaging/live/README.md).

### Intégration continue

| Workflow | Déclencheur | Contenu |
|----------|-------------|---------|
| `ci.yml` | push / PR sur `main` et `I/O_SecMet` | `cargo fmt`, `clippy -D warnings`, tests du workspace et des crates sans GTK, `cargo audit`, shellcheck des scripts, compilation des cibles de fuzz |
| `fuzz.yml` | push / PR touchant `crates/` ou `fuzz/` (60 s par cible), hebdomadaire (30 min), manuel (durée au choix) | cargo-fuzz (nightly, libFuzzer) sur neuf décodeurs purs : sense ATA, IDENTIFY, SMART, NVMe, trames IPC, QR et JSON d'attestation, mountinfo, canaris ; entrées fautives publiées en artefact |
| `package.yml` | push sur `main`, tags `vX.Y.Z`, manuel | `.deb` Ubuntu 24.04 et 22.04 (version `<version>~ubuntu22.04`, publiée sous `secmet_<version>.ubuntu22.04_amd64.deb`), `.rpm` Fedora 42, AppImage ; chaque paquet installé et fumé dans son conteneur ; sur un tag ou à la main, image de la clé USB et son livrable `.tar` ; sur un tag, signature et release GitHub |

Les cibles de fuzz vivent dans `fuzz/` (crate hors workspace) : `cargo +nightly fuzz run ata_sense` en local après `cargo install cargo-fuzz`.

### Publication d'une version

Le push d'un tag `vX.Y.Z` suffit : `package.yml` construit les quatre paquets et l'image de la clé USB (livrable `secmet-live_<version>_x86_64.tar` : image, `secmet-make-key.sh`, `secmet-release`), signe le tout avec la **clé de version** (secret du dépôt, distincte des clés d'attestation des certificats) et crée la release. La même chaîne tourne hors ligne sur le poste du responsable des versions (`packaging/release.sh`, podman ou docker requis), où l'image passe en plus le contrôle QEMU `make live-check`. Détail dans l'[Annexe N](docs/Annexes/Annexe_N_mises_a_jour.md).

```bash
secmet-release keygen ~/.config/secmet/release-key.hex   # une fois ; sauvegarder la clé hors ligne
git tag -a v0.3.2 -m "SecMet 0.3.2" && git push origin main v0.3.2
make release      # dist/release/0.3.2/ : paquets, SHA256SUMS, SHA256SUMS.sig, latest.json
make publish      # release GitHub sur https://github.com/hclcdev12/secmet (confirmation)
secmet-release verify dist/release/0.3.2 --trust <clé publique ou key_id>
```

`SECMET_RELEASE_KEY`, `SECMET_RELEASES_REPO` et `SECMET_RELEASES_URL` remplacent les valeurs par défaut. L'application lit `https://github.com/<dépôt>/releases/latest/download/latest.json` et n'accepte que les fichiers signés par une clé listée dans `src/update.rs` (`RELEASE_PUBLIC_KEYS`).

### Architecture

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
  lot.rs               # Lots de la clé USB : journal chaîné, clôture, archive, export
  key_cleanup.rs       # Nettoyage de la clé USB (inventaire, suppression, journal)
  i18n/                # Traductions de l'interface (fr natif, en, de)
  settings.rs          # ~/.config/secmet/settings.json (passes HDD, langue, logo, opérateur, mises à jour)
  update.rs            # Vérification des mises à jour : latest.json signé, clé de version intégrée, état
  update_install.rs    # Installation d'une mise à jour (poste) ou dépôt sur la clé (app/<version>)
  pdf_logo.rs          # Conversion et stockage du logo personnalisé des certificats
  system_install.rs    # AppImage : installation du composant système au premier lancement
  paths.rs             # Chemins (helper, SECMET_DATA_DIR)
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

Autres dossiers : `fuzz/` (cibles cargo-fuzz, hors workspace), `docs/` (annexes, certificats d'exemple). Documentation développeur : [docs/DEV_CONTEXT.md](docs/DEV_CONTEXT.md).

## Sécurité

- Les opérations sur disques passent par **pkexec** et le daemon `secmet-helper serve` déclaré dans une policy Polkit : un seul prompt par session, jeton par disque lié au numéro de périphérique, au numéro de série et à l'empreinte firmware, ouverture exclusive et ré-identification avant toute écriture.
- Le disque hébergeant la racine du système est **refusé** pour l'effacement (UI et helper).
- Sans `pkexec` setuid, l'application **refuse de démarrer** (pas de repli `sudo`).
- Les effacements sont **irréversibles** ; le bouton Stop interrompt une écriture hôte ou un sanitize suivi par sondage, jamais un Format NVM ou un SECURITY ERASE UNIT déjà lancé.

## Licence

[GNU GPL v3 ou ultérieure](LICENSE) — Copyright (c) 2026 SecMet Contributors

Le code est libre : toute version redistribuée, modifiée ou intégrée doit rester sous GPL avec ses sources. Le nom **SecMet** et le lion du logo sont des marques : ils identifient le logiciel officiel et ne sont pas couverts par la GPL (voir [content/LICENSE](content/LICENSE)) ; une version modifiée doit être renommée.

SecMet s'appuie sur des composants open source (GTK 4, crates Rust, TestDisk embarqué). Détail des licences tierces et conformité distribution : [Annexe J](docs/Annexes/Annexe_J_licences_tierces.md). Annexes : [docs/Annexes/README.md](docs/Annexes/README.md).

Le module d'infos disque SMART s'inspire conceptuellement de [QDiskInfo](https://github.com/edisionnano/QDiskInfo) (GPL-3.0) ; la logique et l'UI sont une réécriture indépendante en Rust/GTK4, sans inclusion de sources QDiskInfo.
