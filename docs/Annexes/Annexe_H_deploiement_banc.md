# Annexe H — Déploiement du banc

> Documentation SecMet — v0.4.0

Ce guide s'adresse à l'**administrateur système** ou à l'**intégrateur** qui met en place le poste atelier. Il complète le [README](../../README.md) (installation du paquet) et le [guide utilisateur](Annexe_G_guide_utilisateur.md) (usage quotidien par l'opérateur).

Chaque release de SecMet publie : un **paquet Debian** (`.deb`, variantes Ubuntu 24.04 et 22.04), un **paquet RPM** (`.rpm`, construit pour Fedora 42), une **AppImage**, l'**image de la clé USB SecMet** (`secmet-key_<version>_x86_64.img.xz`), le livrable de fabrication de la clé (`secmet-live_<version>_x86_64.tar`), l'outil **SecMet Clé USB** qui écrit la clé (`secmet-key-writer_<version>_x86_64.AppImage` pour Linux, `secmet-key-writer_<version>_x86_64.exe` pour Windows), et les fichiers signés `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json`. Le produit est le même dans tous les modes ci-dessous : seule la façon de provisionner le système Linux hôte change.

### Pourquoi une clé USB (ou PXE) ?

Le **banc d'effacement dédié** reste la méthode de **référence** pour traiter des disques en volume (supports branchés en SATA/USB/NVMe sur un poste fixe).

En revanche, pour un **SSD interne** (laptop, ultrabook, all-in-one), **démonter la machine** pour extraire le disque et le brancher sur le banc est souvent **plus long** que de démarrer la machine sur la **clé USB SecMet** (ou un live équivalent, ou en **PXE**), lancer SecMet sur place et effacer le disque interne **sans le sortir du châssis**.

---

## 1. Vue d'ensemble

| Mode | Description |
|------|-------------|
| **Machine dédiée** | PC fixe avec Linux installé sur disque interne |
| **Clé SecMet** | Clé USB écrite depuis l'image publiée : système en lecture seule, partition de données persistante, lots |
| **Live générique** | Système Linux live construit par l'intégrateur, avec SecMet ajouté |
| **PXE** | Poste démarré sur le réseau local (boot PXE), puis exécution locale |

Dans tous les cas, SecMet fonctionne **sur le banc** : aucun serveur applicatif ni service cloud n'est requis pour détecter les disques, lancer les effacements ou générer les certificats. En PXE, le réseau sert au **démarrage** de l'environnement, pas à l'exécution de SecMet.

---

## 2. Comparatif rapide

| Critère | Machine dédiée | Clé SecMet | Live générique | PXE |
|---------|----------------|------------|----------------|-----|
| **Usage typique** | Atelier fixe, production quotidienne, disques **branchés** au banc | **Effacement sur place** (laptops), lots d'une machine à l'autre | Live maison de l'intégrateur | Parc homogène ; **effacement sur place** en réseau d'atelier |
| **Démontage du poste cible** | Souvent **requis** | **Non** | **Non** | **Non** |
| **Livrable** | `.deb` / `.rpm` (ou AppImage) | Image `secmet-key_<version>_x86_64.img.xz`, écrite avec SecMet Clé USB | AppImage copiée sur la clé (ou `.deb` intégré à l'image) | AppImage ou `.deb` dans l'image netboot |
| **Persistance journaux / certificats** | Oui (`~/.secmet_logs/`) | Oui : partition `SECMET-DATA`, livrables sur `SECMET-EXP` | Selon persistance de la clé ; copier les certificats avant arrêt | Selon stockage local ou persistance configurée |
| **Clé de signature des certificats** | Stable (`~/.config/secmet/attestation-key.hex`) | Stable : une par clé USB, créée au premier démarrage | **Régénérée à chaque session sans persistance** : prévoir une persistance ou distribuer la clé publique de chaque session | Idem live générique |
| **Complexité de mise en place** | Faible | Faible (écrire la clé) | Moyenne | Élevée |
| **Recette ([Annexe C](Annexe_C_plan_de_recette.md), R-01, R-18)** | Modèle de référence | Compatible | Compatible | Compatible si image post-boot identique |

---

## 3. Machine dédiée (référence)

1. Installer Debian/Ubuntu/Mint (amd64) sur le banc.
2. Installer le paquet, apt résolvant les dépendances (`libgtk-4-1`, `policykit-1`) et les recommandations (`testdisk`, lecteur PDF) :

   ```bash
   sudo apt install ./secmet_<version>.ubuntu24.04_amd64.deb   # fichier de la release (ubuntu22.04 pour une glibc < 2.39)
   sudo apt install ./dist/secmet_<version>_amd64.deb          # paquet construit localement (make deb)
   # équivalent du second : make install-deb
   ```

   Éviter `dpkg -i` seul, qui n'installe pas les dépendances ; `sudo apt-get install -f` répare le cas échéant. TestDisk est de toute façon embarqué sous `/usr/lib/secmet/bin/`.
3. Vérifier PolicyKit : `pkexec` setuid et `/usr/share/polkit-1/actions/secmet.policy` présent (`make verify-polkit` en environnement de développement).
4. Lancer SecMet depuis le menu applications.

**Fedora** : `sudo dnf install ./secmet-<version>-1.fc42.x86_64.rpm` (fichier de la release) ou `./dist/secmet-<version>-1.<distribution>.x86_64.rpm` (`make rpm`) ; dnf résout `gtk4` et `polkit`, TestDisk est embarqué comme dans le `.deb`.

**Alternative sans gestionnaire de paquets** : lancer l'**AppImage** (§6). Elle installe elle-même son composant système au premier lancement.

Avantages : persistance stable des journaux, des certificats et de la clé de signature ; identification cohérente du banc (DMI) sur les certificats ; mises à jour simples (bouton **Installer la mise à jour** des Réglages, ou `apt` / `dnf` avec le nouveau paquet, [Annexe N](Annexe_N_mises_a_jour.md)). C'est le scénario de la recette **R-01** ([Annexe C](Annexe_C_plan_de_recette.md)).

---

## 4. Clé USB SecMet et live générique

Le poste démarre sur une **clé USB** contenant SecMet. Sur un banc fixe, la clé est le système et les disques à traiter sont branchés en plus ; sur un laptop, le **disque interne** devient la cible, sans démontage.

### 4.1 Clé USB SecMet (livrée prête à écrire)

Chaque release publie l'image `secmet-key_<version>_x86_64.img.xz` : Ubuntu 24.04 minimal, bureau XFCE, SecMet lancé à l'ouverture de la session, chargeur signé pour Secure Boot et démarrage BIOS, partitions `SECMET-DATA` (données persistantes, clé de signature propre à la clé) et `SECMET-EXP` (livrables, exFAT). Détail : [Annexe M](Annexe_M_cle_live_lots.md).

1. **Écrire la clé** avec **SecMet Clé USB** (Windows ou Linux) : l'outil télécharge l'image de la version courante (ou reprend celle déjà téléchargée), vérifie la signature de la release puis l'empreinte de l'image, ne propose que des clés amovibles de 12 Go ou plus, écrit la clé, la relit et complète la table GPT. Sous Windows, il demande les droits d'administrateur (UAC) ; au premier lancement, SmartScreen peut bloquer l'exécutable non signé (« Informations complémentaires » puis « Exécuter quand même »). À défaut, Raspberry Pi Imager, Rufus ou `dd` écrivent l'image telle quelle. Procédure opérateur : [Fabriquer sa clé](../Fabriquer_sa_cle.md).
2. **Premier démarrage** : la clé crée sa clé de signature (clé publique et `key_id` au journal) et agrandit la partition d'export jusqu'au bout de la clé. Remettre la clé publique (`config/attestation-key.pub` de `SECMET-DATA`) aux vérificateurs.
3. **Mot de passe** : l'invite d'administration demande le mot de passe du compte `secmet`, **`secmet`** sur l'image publiée, identique sur toutes les clés et public. Le changer dans la session ne dure que jusqu'à l'extinction ; pour un mot de passe propre à l'atelier, construire l'image depuis le dépôt avec `SECMET_OPERATOR_PASSWORD='…' make live` (le livrable `.tar` publié porte, comme l'image, le mot de passe par défaut ; [packaging/live/README.md](../../packaging/live/README.md)). Garder les clés en lieu sûr.
4. **Fabrication depuis le dépôt** (alternative) : `make live` puis `make live-key DEV=/dev/sdX`.
5. **Mises à jour** : SecMet se met à jour depuis ses Réglages (version conservée sur `SECMET-DATA/app/`, réinstallée à chaque démarrage) ; le système de la clé se met à jour en la réécrivant (sous Linux, `secmet-make-key.sh --system-only` du livrable `.tar` ne réécrit que la partition système et conserve données et clé de signature).

### 4.2 Effacement sur place (cas laptop / SSD interne)

1. Éteindre le poste cible, démarrer sur la clé (BIOS/UEFI : boot USB).
2. Lancer SecMet : le disque interne apparaît dans la liste (il n'est plus le disque système).
3. Sélectionner ce disque, lancer l'effacement, générer le certificat.
4. Sur la clé SecMet, les certificats restent dans le lot sur `SECMET-DATA` et sont copiés sur `SECMET-EXP` à la clôture. Sur un live générique sans persistance, **copier le certificat PDF et le JSON signé** (partage réseau, clé USB secondaire) avant extinction.

### 4.3 Points d'attention

| Sujet | Recommandation |
|-------|----------------|
| **Disque système protégé** | SecMet refuse d'effacer le disque hébergeant la racine `/` (BF-04, [Annexe A](Annexe_A_besoins_fonctionnels.md)). Sur un média live dont la racine est un overlay, l'inventaire ne peut pas rattacher la racine à un disque : SecMet l'indique par un bandeau et une réserve sur le certificat ; la clé reste protégée par ses montages (`/cdrom`, persistance) et par le verrou exclusif du noyau. Vérifier la sélection avec soin. |
| **Boot firmware** | Vérifier que le poste cible autorise le boot USB ; Secure Boot exige une image signée (c'est le cas de la clé SecMet ; à vérifier pour un live générique). |
| **SSD SATA frozen** | Sur laptop, l'état frozen est fréquent : bouton **Dégeler** (mise en veille). |
| **Persistance** (live générique) | Sans partition persistante, `~/.secmet_logs/`, les certificats et la clé de signature sont perdus au redémarrage. Prévoir une persistance ou une copie systématique ; ou lancer SecMet avec `SECMET_DATA_DIR=/chemin/vers/une/partition/montée` pour y placer toutes ses données ([Annexe M](Annexe_M_cle_live_lots.md)). |
| **Dépendances** (live générique) | Avec l'**AppImage** : seuls `pkexec` (PolicyKit) et FUSE 2 doivent être dans l'image ; GTK4 et TestDisk sont embarqués, la policy est installée au premier lancement. Avec le `.deb` : intégrer `libgtk-4-1`, `policykit-1`, `testdisk`. |
| **Mises à jour** (live générique) | Reconstruire l'image live (ou remplacer l'AppImage) à chaque nouvelle version. |

### 4.4 Live générique (live maison)

Un intégrateur qui préfère son propre système live (`live-build`, `casper`, clé Debian live modifiée…) y copie l'AppImage, ou y installe le `.deb`. Les points « live générique » ci-dessus s'appliquent ; la clé SecMet (§4.1) les règle d'office, dont la clé de signature régénérée à chaque démarrage (§8).

---

## 5. PXE

Le poste cible démarre via **PXE** sur une image Linux homogène contenant SecMet, puis SecMet s'exécute en local. Mêmes points d'attention que le live générique, plus :

| Sujet | Recommandation |
|-------|----------------|
| **Réseau** | Nécessaire au boot ; pas obligatoire pendant la session si l'image est complète en local. |
| **Homogénéité** | Standardiser la version de SecMet dans l'image PXE. |
| **Identification banc** | L'inventaire DMI du certificat reflète le matériel réel de chaque poste. |

SecMet **ne fournit pas** de recette PXE.

---

## 6. AppImage

### Construction

```bash
make appimage
# Artefact : dist/SecMet_<version>_x86_64.AppImage
```

`make appimage` assemble l'AppImage **dans un conteneur Ubuntu 24.04** (`packaging/appimage.Containerfile`, podman ou docker requis sur la machine de construction, cible cargo séparée `target-appimage/`). Raison : les bibliothèques embarquées (GTK4, glib, TestDisk) sont liées à la glibc de la machine de construction ; une AppImage assemblée sur une distribution plus récente (Fedora, Arch…) refuse de démarrer sur Mint 22 / Ubuntu 24.04 avec `version GLIBC_2.4x not found`. La glibc 2.39 d'Ubuntu 24.04 est la plus ancienne supportée par le banc. `SECMET_APPIMAGE_NATIVE=1` force un assemblage local, à ses risques.

Contenu : `secmet`, `secmet-helper`, GTK4 et ses chargeurs (dont SVG), TestDisk et ses bibliothèques privées, la policy PolicyKit, la documentation. `secmet-verify` et `secmet-release` ne sont pas dans l'AppImage : ils viennent avec le `.deb` et le `.rpm` (et sont présents sur la clé SecMet).

### Premier lancement

1. Copier l'AppImage sur le poste, la rendre exécutable (`chmod +x`), la lancer.
2. Si le composant système est absent ou différent, le dialogue **« Composant système requis »** (ou « Mise à jour ») s'affiche : accepter, saisir le mot de passe administrateur.
3. L'installeur, embarqué dans le binaire et exécuté par `pkexec`, **extrait le composant depuis le fichier AppImage** dans un répertoire root temporaire, puis installe `/usr/bin/secmet-helper`, `/usr/lib/secmet/` (TestDisk) et `/usr/share/polkit-1/actions/secmet.policy`. Il ne lit rien dans le montage FUSE de l'AppImage, qui n'est pas accessible à root.
4. L'application demande ensuite le mot de passe pour le helper et affiche l'inventaire. Les lancements suivants ne redemandent l'installation que si l'AppImage embarque un helper différent.

Prérequis côté hôte : `pkexec` (PolicyKit) et FUSE 2 (`libfuse2` ; sinon lancer avec `APPIMAGE_EXTRACT_AND_RUN=1`).

### Désinstallation du composant

```bash
sudo rm -rf /usr/bin/secmet-helper /usr/share/polkit-1/actions/secmet.policy /usr/lib/secmet
```

---

## 7. Exigences communes (tous modes)

| Composant | Rôle |
|-----------|------|
| Linux amd64 : Debian/Ubuntu/Mint (`.deb`), Fedora (`.rpm`) ; glibc ≥ 2.39 pour l'AppImage | Plateforme cible |
| GTK 4 (`libgtk-4-1`) | Interface graphique (embarquée dans l'AppImage) |
| PolicyKit / `pkexec` setuid | Élévation pour le helper |
| `testdisk` | Validation optionnelle (embarqué dans le `.deb`, le `.rpm`, l'AppImage et donc sur la clé SecMet) |
| Lecteur PDF (`evince`, `okular`, `atril`) | Consultation des certificats (recommandé) |

Aucun autre outil n'est requis : inventaire, SMART, commandes firmware, écriture hôte, DMI et rendu PDF sont réalisés nativement. Droits **administrateur** requis pour l'opérateur.

---

## 8. Impact sur la traçabilité

| Élément | Machine dédiée | Clé SecMet | Live générique / PXE (sans persistance) |
|---------|----------------|------------|-----------------------------------|
| Journaux | Persistants (`~/.secmet_logs/`) | Persistants (`SECMET-DATA`) | Perdus au reboot |
| Certificats PDF et JSON signés | Archivables localement | Dans le lot sur `SECMET-DATA`, copiés sur `SECMET-EXP` à la clôture, manifeste signé | Copier hors du banc avant arrêt |
| Clé de signature | Stable (`~/.config/secmet/attestation-key.hex`) : une clé publique à distribuer une fois | Stable, une par clé USB (`SECMET-DATA/config/`) : une clé publique par clé à distribuer une fois | Régénérée à chaque démarrage : distribuer la clé publique avec chaque lot de certificats, ou rendre `~/.config/secmet/` persistant |
| Identification du banc (DMI) | Stable | Celle de chaque machine traitée | Varie selon le poste |

Le client reste responsable de la procédure d'archivage des certificats et des manifestes de lot, et de la distribution des clés publiques aux vérificateurs. La clé privée d'une clé SecMet voyage avec elle : une clé USB perdue doit voir sa clé publique retirée des listes de confiance ([Annexe M, §M.6](Annexe_M_cle_live_lots.md#m6-clé-de-signature-et-confiance)).

---

## 9. Hors périmètre produit

- Infrastructure PXE (serveur DHCP/TFTP, recettes netboot) ;
- Supervision centralisée de plusieurs bancs.

---

## 10. Documents connexes

| Document | Public |
|----------|--------|
| [README.md](../../README.md) | Installation, prérequis |
| [Annexe G — Guide utilisateur](Annexe_G_guide_utilisateur.md) | Opérateur atelier |
| [Annexe F — Spécification technique](Annexe_F_specification_technique.md) | Architecture technique |
| [Annexe M — Clé live et lots persistants](Annexe_M_cle_live_lots.md) | Clé USB SecMet et lots |
| [Annexe N — Mises à jour et publication](Annexe_N_mises_a_jour.md) | Mises à jour signées, contenu d'une release |
| [Fabriquer sa clé](../Fabriquer_sa_cle.md) | Écriture de la clé avec SecMet Clé USB |
| [packaging/live/README.md](../../packaging/live/README.md) | Fabrication de la clé depuis le dépôt |
