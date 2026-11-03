# Annexe H — Déploiement du banc

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

Ce guide s'adresse à l'**administrateur système** ou à l'**intégrateur** qui met en place le poste atelier. Il complète le [README](../../README.md) (installation du paquet) et le [guide utilisateur](Annexe_G_guide_utilisateur.md) (usage quotidien par l'opérateur).

SecMet est livré sous trois formes : un **paquet Debian** (`.deb`, variantes Ubuntu 24.04 et 22.04), un **paquet RPM** (`.rpm`, Fedora) et une **AppImage**. Le produit est le même dans tous les modes ci-dessous : seule la façon de provisionner le système Linux hôte change.

### Pourquoi live USB et PXE ?

Le **banc d'effacement dédié** reste la méthode de **référence** pour traiter des disques en volume (supports branchés en SATA/USB/NVMe sur un poste fixe).

En revanche, pour un **SSD interne** (laptop, ultrabook, all-in-one), **démonter la machine** pour extraire le disque et le brancher sur le banc est souvent **plus long** que de démarrer la machine sur une **clé live USB** ou en **PXE**, lancer SecMet sur place et effacer le disque interne **sans le sortir du châssis**.

---

## 1. Vue d'ensemble

| Mode | Description |
|------|-------------|
| **Machine dédiée** | PC fixe avec Linux installé sur disque interne |
| **Live USB** | Système Linux démarré depuis une clé USB personnalisée |
| **PXE** | Poste démarré sur le réseau local (boot PXE), puis exécution locale |

Dans les trois cas, SecMet fonctionne **sur le banc** : aucun serveur applicatif ni service cloud n'est requis pour détecter les disques, lancer les effacements ou générer les certificats. En PXE, le réseau sert au **démarrage** de l'environnement, pas à l'exécution de SecMet.

---

## 2. Comparatif rapide

| Critère | Machine dédiée | Live USB | PXE |
|---------|----------------|----------|-----|
| **Usage typique** | Atelier fixe, production quotidienne, disques **branchés** au banc | Banc nomade ; **effacement sur place** (laptop, poste individuel) | Parc homogène ; **effacement sur place** en réseau d'atelier |
| **Démontage du poste cible** | Souvent **requis** | **Non** | **Non** |
| **Livrable** | `.deb` (ou AppImage) | AppImage copiée sur la clé (ou `.deb` intégré à l'image) | AppImage ou `.deb` dans l'image netboot |
| **Persistance journaux / certificats** | Oui (`~/.secmet_logs/`) | Selon persistance de la clé ; copier les certificats avant arrêt | Selon stockage local ou persistance configurée |
| **Clé de signature des certificats** | Stable (`~/.config/secmet/attestation-key.hex`) | **Régénérée à chaque session sans persistance** : prévoir une persistance ou distribuer la clé publique de chaque session | Idem |
| **Complexité de mise en place** | Faible | Moyenne | Élevée |
| **Recette CDC (R-01, R-18)** | Modèle de référence | Compatible | Compatible si image post-boot identique |

---

## 3. Machine dédiée (référence)

1. Installer Debian/Ubuntu/Mint (amd64) sur le banc.
2. Installer le paquet, apt résolvant les dépendances (`libgtk-4-1`, `policykit-1`) et les recommandations (`testdisk`, lecteur PDF) :

   ```bash
   sudo apt install ./dist/secmet_<version>_amd64.deb
   # équivalent : make install-deb
   ```

   Éviter `dpkg -i` seul, qui n'installe pas les dépendances ; `sudo apt-get install -f` répare le cas échéant. TestDisk est de toute façon embarqué sous `/usr/lib/secmet/bin/`.
3. Vérifier PolicyKit : `pkexec` setuid et `/usr/share/polkit-1/actions/secmet.policy` présent (`make verify-polkit` en environnement de développement).
4. Lancer SecMet depuis le menu applications.

**Fedora** : `sudo dnf install ./dist/secmet-<version>-1.<distribution>.x86_64.rpm` (`make rpm`) ; dnf résout `gtk4` et `polkit`, TestDisk est embarqué comme dans le `.deb`.

**Alternative sans gestionnaire de paquets** : lancer l'**AppImage** (§6). Elle installe elle-même son composant système au premier lancement.

Avantages : persistance stable des journaux, des certificats et de la clé de signature ; identification cohérente du banc (DMI) sur les certificats ; mises à jour simples (`apt` / nouveau `.deb`). C'est le scénario de la recette **R-01**.

---

## 4. Live USB

Le poste démarre sur une **image live** Linux contenant SecMet. Sur un banc fixe, la clé est le système et les disques à traiter sont branchés en plus ; sur un laptop, le **disque interne** devient la cible, sans démontage.

### Effacement sur place (cas laptop / SSD interne)

1. Éteindre le poste cible, démarrer sur la clé live SecMet (BIOS/UEFI : boot USB).
2. Lancer SecMet : le disque interne apparaît dans la liste (il n'est plus le disque système).
3. Sélectionner ce disque, lancer l'effacement, générer le certificat.
4. **Copier le certificat PDF et le JSON signé** (clé persistante, partage réseau, clé USB secondaire) avant extinction.

### Points d'attention

| Sujet | Recommandation |
|-------|----------------|
| **Disque système protégé** | SecMet refuse d'effacer le disque hébergeant la racine `/` (BF-04). Sur un média live dont la racine est un overlay, l'inventaire ne peut pas rattacher la racine à un disque : SecMet l'indique par un bandeau et une réserve sur le certificat ; la clé reste protégée par ses montages (`/cdrom`, persistance) et par le verrou exclusif du noyau. Vérifier la sélection avec soin. |
| **Boot firmware** | Vérifier que le poste cible autorise le boot USB ; Secure Boot peut exiger une image signée. |
| **SSD SATA frozen** | Sur laptop, l'état frozen est fréquent : bouton **Dé-frozen** (mise en veille). |
| **Persistance** | Sans partition persistante, `~/.secmet_logs/`, les certificats et la clé de signature sont perdus au redémarrage. Prévoir une persistance ou une copie systématique ; ou lancer SecMet avec `SECMET_DATA_DIR=/chemin/vers/une/partition/montée` pour y placer toutes ses données ([Annexe M](Annexe_M_cle_live_lots.md)). |
| **Dépendances** | Avec l'**AppImage** : seuls `pkexec` (PolicyKit) et FUSE 2 doivent être dans l'image ; GTK4 et TestDisk sont embarqués, la policy est installée au premier lancement. Avec le `.deb` : intégrer `libgtk-4-1`, `policykit-1`, `testdisk`. |
| **Mises à jour** | Reconstruire l'image live (ou remplacer l'AppImage) à chaque nouvelle version. |

### Construction de l'image (hors périmètre produit)

SecMet **ne fournit pas** d'ISO live clé en main. L'intégrateur construit son image (`live-build`, `casper`, clé Debian live modifiée…) et y copie l'AppImage, ou y installe le `.deb`.

> **Clé USB SecMet** — Une clé prête à l'emploi (Ubuntu 24.04 minimal, bureau XFCE, SecMet au démarrage, partition de données persistante, lots couvrant plusieurs laptops, certificat groupé à la clôture) est décrite dans l'[Annexe M](Annexe_M_cle_live_lots.md) et se fabrique avec `make live` puis `make live-key DEV=/dev/sdX` ([packaging/live/README.md](../../packaging/live/README.md)). Elle règle aussi la clé de signature régénérée à chaque démarrage (§8).

---

## 5. PXE

Le poste cible démarre via **PXE** sur une image Linux homogène contenant SecMet, puis SecMet s'exécute en local. Mêmes points d'attention que le live USB, plus :

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

Contenu : `secmet`, `secmet-helper`, `secmet-verify`, GTK4 et ses chargeurs (dont SVG), TestDisk et ses bibliothèques privées, la policy PolicyKit, la documentation.

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
| `testdisk` | Validation optionnelle (embarqué dans les deux livrables) |
| Lecteur PDF (`evince`, `okular`, `atril`) | Consultation des certificats (recommandé) |

Aucun autre outil n'est requis : inventaire, SMART, commandes firmware, écriture hôte, DMI et rendu PDF sont réalisés nativement. Droits **administrateur** requis pour l'opérateur.

---

## 8. Impact sur la traçabilité

| Élément | Machine dédiée | Live USB / PXE (sans persistance) |
|---------|----------------|-----------------------------------|
| Journaux `~/.secmet_logs/` | Persistants | Perdus au reboot |
| Certificats PDF et JSON signés | Archivables localement | Copier hors du banc avant arrêt |
| Clé de signature `~/.config/secmet/attestation-key.hex` | Stable : une clé publique à distribuer une fois | Régénérée à chaque démarrage : distribuer la clé publique avec chaque lot de certificats, ou rendre `~/.config/secmet/` persistant |
| Identification du banc (DMI) | Stable | Varie selon le poste |

Le client reste responsable de la procédure d'archivage des certificats et de la distribution des clés publiques aux vérificateurs (CDC §8).

---

## 9. Hors périmètre produit

- Image live USB préconfigurée ;
- Infrastructure PXE (serveur DHCP/TFTP, recettes netboot) ;
- Supervision centralisée de plusieurs bancs.

---

## 10. Documents connexes

| Document | Public |
|----------|--------|
| [README.md](../../README.md) | Installation, prérequis |
| [Annexe G — Guide utilisateur](Annexe_G_guide_utilisateur.md) | Opérateur atelier |
| [Annexe F — Spécification technique](Annexe_F_specification_technique.md) | Architecture technique |
| Cahier des charges (document interne, non publié) | Accord produit |
| [Annexe M — Clé live et lots persistants](Annexe_M_cle_live_lots.md) | Clé USB SecMet (spécification) |
| [Annexe N — Mises à jour et publication](Annexe_N_mises_a_jour.md) | Mises à jour signées (spécification) |
