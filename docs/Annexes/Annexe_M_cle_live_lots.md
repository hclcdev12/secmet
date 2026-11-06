# Annexe M — Clé live et lots persistants

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1
>
> **Statut** : spécification validée pour la mise en production, **non implémentée** en v0.3.1. Les comportements décrits ici sont des cibles ; l'état actuel du produit est décrit dans les annexes [F](Annexe_F_specification_technique.md), [G](Annexe_G_guide_utilisateur.md) et [H](Annexe_H_deploiement_banc.md).

---

## M.1 Besoin

Le client récupère des **lots de laptops**. Leurs disques ne sont pas forcément démontés : le laptop démarre sur une **clé USB SecMet**, efface son disque interne, puis s'éteint ; l'opérateur passe au laptop suivant avec la même clé. Un **lot** couvre ainsi plusieurs machines traitées successivement. À la fin, l'opérateur clôture le lot et obtient un **certificat groupé** couvrant toutes les machines.

L'état du lot ne peut pas vivre en mémoire (chaque laptop redémarre la clé) ni sur le réseau (souvent absent) : il voyage **avec la clé**, sur une partition de données persistante.

## M.2 Décisions v1

| Sujet | Décision |
|-------|----------|
| Système de la clé | Ubuntu 24.04 LTS minimal (noyau HWE pour le matériel récent), **bureau XFCE dépouillé** (gestionnaire de fichiers, terminal dans le menu, lecteur PDF Zathura, réglages réseau, extinction) avec connexion automatique, SecMet lancée maximisée au démarrage ; SecMet installé depuis le `.deb` `ubuntu24.04` (pas d'AppImage, pas de FUSE). Choix validé : le bureau est la solution la plus simple pour l'opérateur, car il garde des issues (relire un PDF, brancher le réseau, éteindre) sans rien imposer |
| Droits administrateur | **Conservés** : le mot de passe PolicyKit est demandé comme sur le banc |
| Lot et clé | **Un lot = une clé**. Plusieurs clés sur un même lot : évolution ultérieure (§M.10) |
| Données de travail | Partition **ext4** dédiée sur la clé (journal du lot, journaux, attestations, PDF, clé de signature, réglages) |
| Livrables | Partition **exFAT** d'export, lisible sous Windows et macOS, remplie à la clôture du lot |
| Clé de signature des certificats | **Une par clé USB**, stockée sur la partition de données ; clé publique enregistrée chez le client à la fabrication |
| Identité de la machine sur un lot | Une seule machine : bloc « machine » comme aujourd'hui ; plusieurs machines : bloc remplacé par « Machines traitées : N » et numéro de série du laptop sur chaque ligne du tableau |

## M.3 Structure de la clé

Table de partitions **GPT** :

| Partition | Type GPT | Format | Étiquette | Contenu |
|-----------|----------|--------|-----------|---------|
| 1 | Système EFI / données de démarrage | FAT + squashfs | — | Système live en lecture seule (chargeur signé pour Secure Boot, noyau, squashfs) |
| 2 | Linux filesystem | ext4 | `SECMET-DATA` | Données de travail (ci-dessous) |
| 3 | Microsoft basic data | exFAT | `SECMET-EXP` (nom GPT `SECMET-EXPORT` ; l'exFAT limite l'étiquette de volume à 11 caractères) | Livrables des lots clôturés |

- **ext4 pour les données de travail** : journal du système de fichiers (robustesse aux coupures), droits Unix (`0600` sur la clé de signature). exFAT n'a ni l'un ni l'autre.
- **Windows** ne lit pas l'ext4 et propose de **formater** une partition qu'il ne reconnaît pas. Le type GPT « Linux filesystem » n'est normalement pas monté par Windows, ce qui évite la proposition ; **à vérifier** sur la clé de test. La documentation opérateur rappellera de ne jamais accepter ce formatage.
- **Fabrication (réalisé)** : `make live` construit l'image dans un conteneur privilégié (`packaging/live/` : debootstrap Ubuntu 24.04, noyau HWE, `live-boot`, XFCE dépouillé, LightDM en connexion automatique, SecMet depuis le `.deb` du dépôt, opérateur `secmet`) ; `make live-key DEV=/dev/sdX` (`secmet-make-key.sh`) crée les trois partitions, installe GRUB (chaîne UEFI signée Ubuntu pour Secure Boot, et BIOS), initialise la partition de données avec le schéma de propriétés, crée la clé de signature et affiche son `key_id`. Détail dans [packaging/live/README.md](../../packaging/live/README.md).
- **Image prête à écrire (réalisé 2026-11-06)** : chaque release publie aussi `secmet-key_<version>_x86_64.img.xz` (`make live-dist`), la même clé sous forme de fichier image GPT (système 6 Gio, données 4 Gio, export 1 Gio, GRUB UEFI signé et BIOS), à écrire telle quelle avec balenaEtcher, Rufus ou `dd` : aucun outil SecMet ni Linux chez l'opérateur ([Fabriquer sa clé](../Fabriquer_sa_cle.md)). L'image ne contient **aucune clé de signature** ; au premier démarrage, `secmet-first-boot` en crée une sur la partition de données (clé publique et `key_id` au journal) et agrandit la partition d'export jusqu'au bout de la clé. Environ 850 Mo compressés, 11 Go écrits.
- **Mise à jour du système** : le script réécrit **la partition 1 seulement** ; les partitions 2 et 3 (lot en cours, clé de signature, livrables) sont conservées. Voir l'[Annexe N](Annexe_N_mises_a_jour.md).

### Arborescence de `SECMET-DATA`

```
SECMET-DATA/                     # racine : root, 0755 (jamais inscriptible par l'opérateur)
├── vault/                       # root, 0700 : mots de passe ATA en attente (helper)
├── logs/                        # opérateur ; sessions, rapports et certificats hors lot
├── config/                      # opérateur
│   ├── settings.json            # langue, logo, opérateur, passes HDD
│   ├── pdf-logo.jpg
│   ├── attestation-key.hex      # clé privée de la clé USB (0600)
│   ├── attestation-key.pub
│   └── trusted-keys
├── lots/<LOT-ID>/               # lot ouvert
│   ├── lot.json                 # état du lot
│   ├── journal.ndjson           # entrées chaînées (§M.4)
│   ├── certificates/            # PDF et attestations JSON par machine
│   └── sessions/                # journaux de session par machine
├── archive/<LOT-ID>/            # lots clôturés, en lecture seule
└── app/                         # versions de SecMet téléchargées (Annexe N)
```

### Dossier de données configurable (réalisé)

La variable **`SECMET_DATA_DIR`** (chemin absolu, renseignée par la session live) place toutes les données de l'application dans le dossier indiqué : `config/` (réglages, logo, `attestation-key.hex`, `trusted-keys`, `update-state.json`) et `logs/` (sessions, rapports, certificats, tant que les lots du §M.4 n'existent pas). Sans la variable, le comportement historique (`~/.config/secmet`, `~/.secmet_logs`) est inchangé, et la migration depuis `shred-graphique` n'est tentée que dans ce cas. `secmet-verify` lit aussi la clé publique de `SECMET_DATA_DIR/config/`. Tout chemin de données passe par `src/paths.rs` (`layout()`).

**Coffre ATA sur la clé (réalisé)** : le coffre des mots de passe ATA posés par SecMet (`/var/lib/secmet/ata-security/`, côté root) serait en mémoire sur un système live, et un SECURITY ERASE interrompu par une coupure laisserait le disque verrouillé avec un mot de passe perdu. Le helper utilise donc `SECMET-DATA/vault/` sur le système de la clé (marqueur `/usr/local/bin/secmet-live`, présent seulement dans l'image) dès que cette partition est montée (détection par étiquette, résolue à chaque usage), à condition que le dossier soit sûr. Sur un poste installé, une clé branchée n'est qu'un disque parmi d'autres : le coffre reste sur le poste, sinon un mot de passe posé pendant la présence de la clé serait introuvable une fois la clé retirée (constat du 2026-11-05 sur le banc, où la clôture d'un lot livrait aussi sur `SECMET-EXP`) : racine du montage propriété de root et non inscriptible par d'autres, `vault/` ni lien symbolique ni propriété d'un utilisateur, mode `0700` (créé ainsi par le helper). Sinon, repli sur `/var/lib` avec un avertissement. Aucune information du client n'entre dans ce choix. D'où le schéma de propriétés ci-dessous, à respecter par `secmet-make-key`.

## M.4 Cycle de vie d'un lot

> **Réalisé** : modèle (`src/lot.rs` : `lot.json`, journal chaîné `journal.ndjson`, écritures sûres, reprise, doublons, clôture, archivage), manifeste signé (`secmet-attest::manifest`, `reconcile`), rendu du PDF groupé à partir d'attestations déjà signées, **règle d'identité du PDF** (§M.5) et **interface** (`src/ui/lot_ui.rs`) : dialogue au démarrage (Continuer / Clôturer / Nouveau, ou Ouvrir / Sans lot), bandeau de lot avec bouton Clôturer, contrôle de doublon avant l'effacement (Remplacer / Ajouter quand même / Annuler), entrée écrite au lancement, certificats générés automatiquement en fin d'effacement dans `lots/<id>/certificates/` (opérateur du lot, une page par disque), clôture (PDF groupé, manifeste signé, copie sur `SECMET-EXP` si montée **et seulement depuis la clé live** : sur un poste installé, une clé branchée n'est qu'un disque parmi d'autres et les livrables restent dans l'archive du poste, archivage). Le mode lot est toujours actif avec `SECMET_DATA_DIR` (clé live) et, depuis le 2026-11-02, activable sur un poste installé par le réglage « Mode lot » ; chaque lot porte une **portée** enregistrée à l'ouverture : `machines` (clé live : une entrée par portable, doublon = même série DMI ou mêmes disques) ou `disks` (poste installé : la machine est toujours le banc, une entrée par session de disques, doublon = mêmes numéros de série de disques seulement, comptage et libellés en disques ; le PDF groupé affiche le banc comme machine unique et une ligne par disque) ; les journaux de session et le rapport vont dans `lots/<id>/sessions/`. **Non retenu pour l'instant** : le bouton « Éteindre ce PC » (dépend de l'environnement de bureau de la clé, à voir avec l'image). **À faire** : horloge (§M.7), coffre ATA sur la clé, image de la clé.


États : **aucun lot** → **ouvert** → **clôturé** (archivé, lecture seule), ou **abandonné** quand aucun disque n'a été effacé : sans certificat il n'y a rien à grouper ni à signer, le bouton Clôturer propose alors « Abandonner le lot » (réalisé 2026-11-05). L'abandon est journalisé (`lot_abandoned`, avec le nombre d'entrées en échec ou interrompues), le lot passe dans `archive/` tel quel, avec son journal mais sans certificat groupé ni manifeste, et rien n'est copié sur la partition d'export. Dès qu'un certificat est enregistré, seule la clôture reste possible.

1. **Démarrage** : si un lot est ouvert, SecMet affiche « Lot L-20261030-A — opérateur X — 7 machines — ouvert le … » avec **Continuer**, **Clôturer** ou **Nouveau lot** (ce dernier exige d'abord la clôture). Sans lot ouvert, il propose d'en ouvrir un (opérateur, référence client facultative).
2. **Avant l'effacement**, une entrée « en cours » est ajoutée au journal : machine (fabricant, modèle, numéro de série DMI), disques (nom, modèle, numéro de série), version de SecMet.
3. **Après l'effacement**, le certificat et l'attestation du laptop sont écrits, puis l'entrée passe à « terminée » ou « échec » avec l'empreinte SHA-256 de chaque attestation. SecMet propose **Éteindre ce PC**. Le bouton **Générer le PDF** ne régénère plus les certificats d'une machine déjà au lot (il ouvre le dossier) : l'essai du 2026-11-02 a montré qu'une régénération écrase le fichier enregistré et bloque la clôture (« empreinte différente du journal du lot »). Pour un lot déjà touché, la clôture vérifie que le fichier actuel est signé par la clé du poste et désigne la même session, puis propose de l'enregistrer à nouveau : événement `certificate_updated` (ancienne et nouvelle empreintes) dans le journal chaîné.
4. **Reprise après incident** : une entrée restée « en cours » (coupure, plantage, clé arrachée) est signalée au démarrage suivant comme **interrompue** ; l'opérateur la refait ou la consigne en échec.
5. **Machine déjà présente** (même série DMI, mêmes disques) : SecMet demande s'il faut **remplacer** l'entrée précédente (par exemple après un échec dû à un disque gelé). L'ancienne entrée reste dans le journal avec le statut « remplacée ».
6. **Clôture** : voir §M.5.

### Écritures fiables

- `journal.ndjson` ne fait que croître : une ligne par événement, écrite puis forcée sur le support (`fsync`) avant de poursuivre.
- Chaque entrée contient l'empreinte SHA-256 de la précédente : supprimer ou modifier une entrée casse la chaîne.
- `lot.json`, les PDF et les attestations sont écrits dans un fichier temporaire, forcés sur le support, puis renommés (et le dossier forcé à son tour) : un fichier est soit absent, soit complet. L'export actuel procède déjà par fichier temporaire et renommage.

## M.5 Clôture et compilation

1. **PDF groupé régénéré** à partir des attestations signées du lot (pas de fusion de PDF) : synthèse, tableau du lot, pages suivantes si nécessaire. Le moteur actuel (`render_with_logo` avec un identifiant de lot) produit déjà ce format ; il est étendu pour la règle d'identité ci-dessous.
2. **Identité de la machine** :
   - une seule machine dans le lot : bloc « machine » de la synthèse comme aujourd'hui ;
   - plusieurs machines : bloc remplacé par « Machines traitées : N », et numéro de série du laptop affiché en deuxième ligne sous le modèle du disque, dans le tableau (le tableau est déjà plein en largeur A4).
   Aujourd'hui, la synthèse du lot reprend la machine de la première attestation (`first.bench`) : correct sur un banc fixe, faux sur un lot de laptops.
3. **Manifeste de lot signé** (`<LOT-ID>-manifest.json`) : identifiant du lot, opérateur, dates d'ouverture et de clôture, nombre de machines, liste des certificats (identifiant et empreinte SHA-256 de chaque attestation), empreinte finale du journal. Il est signé avec la clé de la clé USB et vérifié par `secmet-verify --manifest` (signature, clé de confiance, confrontation aux attestations du dossier : manquante, modifiée ou en trop). Le PDF groupé d'un lot clôturé comporte toujours la synthèse de lot, même pour une seule machine. Seuls les certificats listés au manifeste sont copiés sur la partition d'export ; ceux des entrées remplacées restent dans l'archive. Il comble une faiblesse du format actuel : `…-lot.json` est une suite d'attestations indépendantes, dont on peut retirer une sans que cela se voie.
4. **Export** : PDF groupé, manifeste, attestations et certificats individuels sont recopiés sur `SECMET-EXPORT`, dans `<LOT-ID>/`. L'archive sur la partition de données reste la référence ; l'export n'est qu'une copie, non bloquante (partition absente ou pleine : la clôture aboutit, le message le dit). Rattrapage (réalisé 2026-11-05) : bouton « Réessayer la copie » dans le compte rendu de clôture, et dans les Réglages de la clé « Recopier les lots clôturés sur la partition d'export », qui recopie depuis `archive/` les lots dont le manifeste manque dans l'export (`lot::export_archived`). **Nettoyage de la clé** (réalisé 2026-11-05, Réglages de la clé seulement, `src/key_cleanup.rs`) : trois groupes à cocher, lots archivés, livrables de l'export (dossiers `L-…` et `SEC-…` seulement), certificats hors lot avec rapports et journaux de session ; inventaire affiché avec les comptes et la taille, confirmation, refus tant qu'un lot est ouvert, refus du groupe « lots archivés » tant qu'un lot clôturé n'a pas sa copie sur l'export (blocage strict décidé le 2026-11-06 : l'archive est alors la seule copie signée, il faut d'abord la recopier ou décocher le groupe) ; `config/`, `app/`, `vault/` et `logs/system` ne sont jamais touchés ; chaque nettoyage est noté dans `logs/cleanup.log`. **Sans lot** sur la clé (réalisé 2026-11-05) : le certificat produit par « Générer le PDF » est copié dans `SECMET-EXPORT/<SESSION-ID>/` (PDF et attestations JSON) au moment de sa création, pour qu'aucun certificat ne reste enfermé sur la partition ext4. Sur un poste installé, rien n'est jamais copié vers une clé SecMet branchée.
5. **Archivage** : le lot passe dans `archive/` en lecture seule. Le client archive le manifeste de son côté (§M.6).

## M.6 Clé de signature et confiance

- **Principe** (inchangé) : chaque attestation est signée Ed25519 avec la clé privée du poste ; `secmet-verify` contrôle la signature et n'accepte que les **clés publiques de confiance** (`--trust`, `/etc/secmet/trusted-keys`…). Une signature valide prouve seulement que le document vient du détenteur de la clé.
- **Sur une clé live sans persistance**, une nouvelle clé serait créée à chaque démarrage : chaque laptop serait signé par une clé différente et inconnue. Avec `SECMET-DATA`, chaque clé USB a **sa** clé, stable, enregistrée une fois chez le client. Elle est créée par `secmet-make-key.sh` (clé fabriquée depuis le dépôt ou le livrable `.tar`) ou au premier démarrage par `secmet-first-boot` (clé écrite depuis l'image `.img.xz` publiée, qui n'en contient aucune : deux clés écrites depuis la même image n'ont jamais la même identité).
- **Risque** : la clé privée voyage sur la clé USB. Clé USB perdue ou volée = possibilité de signer de faux certificats qui passent la vérification, éventuellement antidatés (les dates sont déclarées par le signataire).
- **Parades v1** :
  - le client **archive le manifeste signé** de chaque lot clôturé ;
  - en cas de perte, la clé publique est retirée des listes de confiance ; seuls les certificats présents dans un manifeste archivé avant la perte restent crédibles.
- **Évolution** : chiffrement de la clé privée par une phrase de passe saisie à l'ouverture du lot (un code PIN court se casse hors ligne).

## M.7 Horloge (réalisé)

Les laptops de récupération ont souvent une pile de BIOS épuisée (date en 2000) ou une horloge réglée en heure locale par Windows, lue comme UTC par Linux (décalage d'une à deux heures). Les dates des certificats et l'ordre du lot en dépendent.

- `src/clock.rs` est la seule source d'heure de l'application (certificats, lots, journaux). Le contrôle ne tourne qu'en mode clé.
- Horloge synchronisée par le réseau (`systemd-timesyncd`) : acceptée.
- Sinon, la date doit être postérieure aux références connues : construction du logiciel (date de l'exécutable), dernière entrée du lot ouvert, dernier lot clôturé sur la clé. Une date antérieure ouvre un dialogue : **Garder l'horloge du poste** ou **Utiliser cette date** (saisie `AAAA-MM-JJ HH:MM`, appliquée par décalage dans SecMet, sans modifier l'horloge du système).
- Le certificat porte une réserve selon le cas : horloge non synchronisée (aucune source réseau), horloge douteuse conservée sur confirmation, ou date saisie par l'opérateur.
- Le décalage en heure locale/UTC (Windows) n'est pas détectable sans réseau : il reste couvert par la réserve « non synchronisée ».

## M.8 Adaptations de l'interface en mode clé live

- Détection du mode clé live (présence de `SECMET-DATA`).
- Le bandeau « Disque système non identifié », attendu sur un système live, est remplacé par un message propre au mode clé live, pour ne pas habituer les opérateurs à ignorer un avertissement.
- « Machine et équipement du banc » devient « Machine » sur les certificats.
- Dialogue de lot au démarrage, bouton **Éteindre ce PC** après l'enregistrement.
- Mises à jour : proposées mais appliquées seulement après la clôture du lot ([Annexe N](Annexe_N_mises_a_jour.md)).
- La clé USB reste protégée comme aujourd'hui : ses partitions sont montées, donc INDISPONIBLE.
- Bureau de la clé (réalisé) : XFCE sombre et plat (Arc-Dark, icônes elementary-xfce, police Ubuntu, menu Whisker avec recherche, barre translucide masquée en permanence (sort au bord haut de l'écran), fond SecMet, démarrage Plymouth sans messages ; ≈ 30 Mo de paquets), fenêtre SecMet maximisée, dialogue des réglages qui défile selon la hauteur de l'écran. Rien n'interrompt un effacement : pas d'extinction d'écran ni de veille d'inactivité, capot fermé ignoré (logind et gestionnaire d'énergie XFCE), pas de verrouillage au réveil.
- Dé-frozen (réalisé) : la mise en veille passe par logind (`systemctl suspend`) pour que la session soit prévenue et rallume l'écran au réveil ; un crochet `system-sleep` force en plus DPMS et `xrandr --auto`. Premier essai réel (2026-11-02, HP ProBook 455 G3) : poste figé au réveil (ni affichage ni clavier), avec la veille directe comme avec logind. Cause, lue dans le journal système recopié sur la clé : `nomodeset=0` sur la ligne noyau de `grub.cfg`, que le noyau lit comme `nomodeset` ; tous les pilotes graphiques étaient désactivés (amdgpu refusé, erreur -22) et le poste tournait sur le tampon du firmware (`simpledrm`), incapable de rallumer l'écran après une veille. Le noyau, lui, reprenait normalement, la clé USB restait attachée et libata filtrait le SECURITY FREEZE LOCK renvoyé par le BIOS à la reprise : c'est bien la veille qui lève le gel. Paramètre retiré ; `toram` (système copié en mémoire) avait été conservé par prudence pour la clé USB pendant la veille, puis retiré de l'entrée par défaut le 2026-11-05 après un réveil réussi avec la racine sur la clé : il reste disponible comme deuxième entrée du menu (clé retirable après l'ouverture de session, 4 Go de RAM). Pour le diagnostic, le journal système est recopié en texte sur la partition de données (`logs/system/<boot-id>.log`) toutes les minutes, avant une veille et à l'extinction ; un journal persistant écrit directement sur la clé bloquait journald jusqu'à 40 s à l'extinction (mesuré le 2026-11-02).

### Un seul logiciel, deux modes (réalisé)

Il n'existe pas de « version clé » et de « version poste » : le même code et le même paquet `.deb` servent aux deux, et la version déposée dans `app/<version>` sur la clé est celle installée par `apt` sur un banc. Ce qui change est décidé au lancement par la variable `SECMET_DATA_DIR`, que le lanceur `secmet-live` de l'image renseigne (`/media/secmet-data`) et qu'un poste n'a pas. Dans le code, `paths::dedicated_data_dir()` répond vrai sur la clé, et c'est ce test seul qui active ce qui n'a de sens que là :

- la section **Nettoyage de la clé** et le bouton **Recopier les lots clôturés sur la partition d'export** dans les Réglages ;
- les lots obligatoires au démarrage (sur un poste, ils restent optionnels, activés par le réglage `lots_enabled`) ;
- la copie des certificats et des lots sur la partition d'export `SECMET-EXP` ;
- le dépôt des mises à jour téléchargées dans `app/<version>` au lieu d'une installation système.

Sur un poste, lancé sans la variable, les données vont dans `~/.config/secmet` et `~/.secmet_logs`, il n'y a pas de partition d'export, et le nettoyage n'est pas proposé : il n'y a pas de clé à nettoyer, et effacer `~/.secmet_logs` reviendrait à supprimer les archives du poste sans copie ailleurs. Pour tester l'interface de la clé sur un banc, il suffit de lancer le binaire installé avec `SECMET_DATA_DIR` pointé sur la partition de données de la clé montée.

Les mises à jour suivent deux chemins pour une même release ([Annexe N](Annexe_N_mises_a_jour.md)). Le bouton **Installer la mise à jour** télécharge le paquet et vérifie sa signature dans les deux cas, puis : sur un poste, il l'installe par `apt-get` ou `dnf` sous `pkexec`, durablement ; sur la clé, dont le système vit en mémoire et repart de l'image à chaque démarrage, il dépose le paquet dans `app/<version>`, et le service `secmet-app-update` le déploie au démarrage suivant en gardant la version précédente en repli. L'image de la clé embarque toujours la version avec laquelle elle a été fabriquée ; elle ne sert plus que de base, `app/` fait la version courante. Clé et banc se mettent donc à jour indépendamment et peuvent différer d'une version sans conséquence, les données de la clé restant lisibles par toute version depuis la 0.3.9.

## M.9 Contraintes matérielles

| Situation | Conséquence | Repli |
|-----------|-------------|-------|
| Secure Boot actif | Démarre avec le chargeur et le noyau signés d'Ubuntu ; SecMet n'ajoute aucun module noyau | — |
| BIOS protégé par mot de passe, démarrage USB désactivé | La clé ne démarre pas | Démonter le disque et le traiter au banc |
| Mode Intel RST / « RAID On » | Le NVMe peut être invisible | Passer en AHCI dans le BIOS, ou banc |
| SSD SATA gelé (fréquent sur laptop) | SANITIZE et SECURITY ERASE refusés | Dé-frozen par mise en veille (logind) ; la sortie de veille varie selon les modèles : si l'écran reste noir, forcer l'extinction et relancer (le lot est conservé) ; à défaut, écriture hôte ou banc |
| Matériel très récent | Pilotes absents d'un noyau ancien | Noyau HWE d'Ubuntu 24.04 ; image reconstruite périodiquement |

## M.10 Hors périmètre v1

- Plusieurs clés USB sur un même lot, fusion de lots (parallélisme) : à concevoir après validation de la v1.
- Phrase de passe sur la clé de signature.
- Synchronisation réseau des lots ou des certificats.

## M.11 Recette prévue

Scénarios à intégrer à l'[Annexe C](Annexe_C_plan_de_recette.md) lors de l'implémentation :

| Scénario | Résultat attendu |
|----------|------------------|
| Démarrage de la clé sur 4 à 5 modèles de laptops, Secure Boot actif | SecMet s'ouvre, disque interne listé, clé USB INDISPONIBLE |
| Lot de 3 laptops, extinction entre chaque | Au démarrage, proposition de continuer le lot ; 3 entrées terminées |
| Coupure d'alimentation pendant un effacement | Entrée signalée interrompue au démarrage suivant ; lot intact |
| Horloge du BIOS en 2000 | Date refusée, confirmation demandée, réserve sur le certificat |
| Même laptop traité deux fois | Proposition de remplacer l'entrée ; ancienne entrée « remplacée » |
| Clôture | PDF groupé avec « Machines traitées : 3 » et séries par ligne ; manifeste valide avec `secmet-verify` |
| Attestation retirée du lot après clôture | Manifeste en désaccord, détecté par `secmet-verify` |
| Clé branchée sur un PC Windows | Partition d'export lisible ; aucune proposition de formatage |

## M.12 Estimation

| Bloc | Charge |
|------|--------|
| Dossier de données configurable | 0,5 à 1 j |
| Lot persistant : état, journal chaîné, dialogue de démarrage, reprise, doublons | 3 à 5 j |
| Clôture : PDF compilé, règle d'identité, manifeste signé, vérification | 2 à 4 j |
| Mode clé live : bandeau, libellé « Machine », horloge, bouton Éteindre | 1 à 2 j |
| Image et clé : recette live-build ou mkosi, partitions, démarrage automatique, script de fabrication, documentation | 4 à 6 j |
| Tests sur 4 à 5 modèles de laptops | 2 à 3 j |
| **Total** | **3 à 4 semaines** |

Un prototype (clé live, démarrage automatique, `HOME` sur la partition de données, sans lot persistant) se monte en 1 à 2 jours pour valider le démarrage sur les laptops du client.

---

## Documents connexes

| Document | Objet |
|----------|-------|
| [Annexe N — Mises à jour et publication](Annexe_N_mises_a_jour.md) | Versions sur la clé, mise à jour du système de la clé |
| [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md) | Modes de déploiement actuels (dédié, live USB, PXE, AppImage) |
| [Annexe F — Spécification technique](Annexe_F_specification_technique.md) | Architecture actuelle (attestation, PDF, helper) |
