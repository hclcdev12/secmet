# Annexe N — Mises à jour et publication des versions

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1
>
> **Statut** : spécification validée et **réalisée** pour l'application : chaîne `make release` / `make publish`, binaire `secmet-release`, releases GitHub du dépôt du code (§N.3, §N.7 ; `secmet-releases` jusqu'à 0.3.5), vérification dans l'application avec point jaune, version annoncée et lien vers la page de téléchargement (§N.2). **Décision révisée (2026-11-03)** : bouton **Installer la mise à jour** pour les paquets `.deb` et `.rpm` (§N.5), afin que les testeurs se mettent à jour depuis l'application ; l'AppImage reste manuelle. **Réalisé (0.3.4)** : versions sur la clé live (§N.6).

---

## N.1 Principes

1. **Seules les versions publiées sont proposées** : une release taguée (`v0.3.2`…), jamais un simple push sur `main`.
2. **Les mises à jour sont signées** : l'application refuse tout paquet dont la signature ou l'empreinte ne correspond pas. Le HTTPS ne suffit pas : un compte d'hébergement compromis ne doit pas permettre de diffuser un helper root piégé.
3. **Le hors ligne ne bloque jamais** : sans réseau, SecMet fonctionne avec la version installée, sans message d'erreur.
4. **L'opérateur décide** : une mise à jour est signalée, jamais imposée.
5. **Jamais au mauvais moment** : l'annonce n'interrompt rien ; en mode clé live, une nouvelle version n'est activée qu'après la clôture du lot ([Annexe M](Annexe_M_cle_live_lots.md)). Un même lot ne mélange donc pas deux versions.

## N.2 Ce que voit l'opérateur

- Au démarrage puis toutes les 24 h, SecMet vérifie en arrière-plan si une version plus récente est publiée (délai court, échec silencieux).
- Si oui, un **point jaune** apparaît sur l'icône **Réglages**.
- Les Réglages affichent une section **Mises à jour** : version installée, version disponible, sa date et ses notes, date de la dernière vérification, bouton **Vérifier maintenant** et bouton **Ouvrir la page de téléchargement**.
- **Pas de bouton Installer** : l'application annonce, elle n'installe pas. Un téléchargement suivi d'une installation depuis l'application aurait demandé une commande privilégiée de plus dans le helper pour un gain limité ; l'installation suit les voies normales de chaque livrable (§N.5).
- La case « Vérifier automatiquement les mises à jour » désactive la requête quotidienne ; « Vérifier maintenant » reste possible.

### Réalisation (v0.3.1)

- `src/update.rs` : adresse du canal (`SECMET_UPDATE_URL`, réglage `update_url`, `moved_to` signé, défaut), téléchargement avec délai de 8 s et corps borné à 64 Kio, `latest.json` accepté seulement si signé par une clé de `RELEASE_PUBLIC_KEYS`, état dans `~/.config/secmet/update-state.json` (`checked_at`, dernier `latest` valide, erreur).
- `src/ui/update_ui.rs` : vérification sur un fil dédié au démarrage si plus de 24 h se sont écoulées, minuteur horaire (robuste à la veille), pastille `update-dot` en surimpression de l'engrenage.
- Client HTTPS : `ureq` avec `rustls` et les racines `webpki-roots` (pas d'OpenSSL de l'hôte, compatible AppImage).

## N.3 Canal de publication

| Phase | Emplacement |
|-------|-------------|
| Développement | Releases GitHub du dépôt du code **`hclcdev12/secmet`** (public, GPL-3.0-or-later). Jusqu'à la 0.3.5 le canal était `hclcdev12/secmet-releases` ; sa dernière release ne contient qu'un `latest.json` signé dont `sha256sums_url` et `moved_to` renvoient vers `secmet`, ce qui fait basculer les installations existantes (vérifié sur le banc le 3 novembre 2026 : 0.3.4 → 0.3.5 par le pointeur). |
| Production | Dépôt public de l'entreprise (code ou releases), à décider au lancement |

Chaque release contient :

| Fichier | Rôle |
|---------|------|
| `SecMet_<version>_x86_64.AppImage` | AppImage |
| `secmet_<version>.ubuntu24.04_amd64.deb`, `secmet_<version>.ubuntu22.04_amd64.deb` | Paquets Debian / Ubuntu / Mint (version Debian `<version>~<distro>` dans le paquet ; nom publié selon la convention de GitHub, qui remplace `~` par `.` dans les noms de fichiers d'une release) |
| `secmet-<version>-1.fc42.x86_64.rpm` | Paquet Fedora |
| `SHA256SUMS` | Empreintes de tous les paquets |
| `SHA256SUMS.sig` | Signature des empreintes avec la clé de version (§N.4) : JSON `{alg, key_id, public_key, sha256, signature}` |

Le dépôt publie aussi un fichier **`latest.json`** signé, que l'application consulte :

```json
{
  "alg": "Ed25519", "key_id": "…", "public_key": "…",
  "payload": {
    "version": "0.3.2",
    "published_at": "2026-11-15",
    "notes": { "fr": "…", "en": "…", "de": "…" },
    "sha256sums_url": "https://github.com/<dépôt>/releases/download/v0.3.2/SHA256SUMS",
    "moved_to": null
  },
  "signature": "…"
}
```

La signature couvre le JSON canonique de `payload`, précédé du séparateur de domaine `secmet-release-v1` et de l'étiquette du document (`latest`, ou `sums` pour `SHA256SUMS`) : une signature de certificat ne peut pas être rejouée comme signature de version, ni l'inverse. L'application lira `https://github.com/<dépôt>/releases/latest/download/latest.json`, qui pointe toujours sur la dernière release.

### Adresse de vérification

L'application contient l'adresse de `latest.json`. Le dépôt changera d'emplacement au lancement (compte de l'entreprise) ; les applications déjà installées doivent continuer à trouver les mises à jour :

- l'adresse vise de préférence un **domaine contrôlé** par l'entreprise (redirigé vers l'hébergement réel, GitHub Pages par exemple) ;
- `latest.json` peut annoncer un nouvel emplacement (`moved_to`), suivi seulement s'il est signé ;
- l'adresse peut être remplacée dans un fichier de configuration, pour les cas particuliers.

Changer d'hébergement ne change pas la confiance : seule compte la clé de version.

## N.4 Clé de signature des versions

- Paire Ed25519 **distincte** des clés de signature des certificats.
- La clé privée reste **hors ligne**, chez le responsable des versions ; elle ne va jamais sur GitHub ni dans une CI.
- La clé publique est **intégrée à l'application** à la compilation.
- **Sauvegarde hors ligne obligatoire** : sans elle, plus aucune mise à jour automatique n'est possible sans réinstallation manuelle.
- **Changement de clé** : la nouvelle clé publique est livrée par une version signée avec l'ancienne.

## N.5 Installation selon le livrable

**Depuis l'application** (`.deb`, `.rpm`) : la section Mises à jour affiche **Installer la mise à jour** quand une version plus récente est connue. SecMet télécharge `SHA256SUMS` et `SHA256SUMS.sig` depuis le dossier de la version, vérifie la signature (clé de version embarquée), télécharge le paquet de la plateforme (`.deb` Ubuntu 24.04 si la glibc est ≥ 2.39, sinon 22.04 ; `.rpm` si `dnf` est présent) dans `/tmp/secmet-update-<uid>/`, compare son empreinte à `SHA256SUMS`, puis lance `pkexec apt-get install -y --allow-downgrades <paquet>` (ou `pkexec dnf install -y`) : PolicyKit demande le mot de passe administrateur. Rien n'est exécuté sans empreinte vérifiée. En cas de succès, **Relancer SecMet** démarre la nouvelle version. Module : `src/update_install.rs`. **Essai sans publier** : un canal servi sur la boucle locale (`http://127.0.0.1:<port>/`) est accepté par la validation de `latest.json` et par `secmet-release sign --sums-url`, uniquement pour cette adresse ; avec `SECMET_UPDATE_URL=http://127.0.0.1:<port>/latest.json`, le cycle complet (point jaune, téléchargement, vérification, installation) se teste sur un poste avant tout `make publish`.

**À la main** : l'opérateur ou l'administrateur suit le lien de la section Mises à jour, télécharge le livrable et vérifie l'empreinte (`sha256sum -c SHA256SUMS`, ou `secmet-release verify`), puis :

| Livrable | Installation |
|----------|--------------|
| Banc fixe, `.deb` | `sudo apt install ./secmet_<version>.ubuntu24.04_amd64.deb`. Évolution possible si les bancs installés se multiplient : dépôt APT signé, et `apt upgrade` fait le reste |
| `.rpm` | `sudo dnf install ./secmet-<version>-1.fc42.x86_64.rpm` |
| AppImage | Remplacer le fichier par la nouvelle AppImage ; au lancement suivant, le dialogue « Mise à jour du composant système » met le helper à jour (un mot de passe administrateur) |

## N.6 Clé live

Le système de la clé est en lecture seule ; SecMet fait partie de l'image. Deux niveaux :

1. **Mises à jour de SecMet** — sur la partition de données (**réalisé en 0.3.4**) :
   - le bouton **Installer la mise à jour** fonctionne comme sur un poste (installation pour la session, en mémoire) et, sur la clé, range en plus le paquet vérifié avec `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` dans `SECMET-DATA/app/<version>/` ; seules les deux versions les plus récentes sont gardées ; le dossier `app/` appartient à l'opérateur (créé par `secmet-make-key`, et au démarrage par `secmet-app-update` s'il manque : une clé fabriquée sans lui renvoyait « Permission non accordée » à l'installation, constaté le 2026-11-05) ;
   - au démarrage, avant la session (`secmet-app-update.service`, après le montage de la partition, avant LightDM), un script parcourt `app/` de la version la plus récente à la plus ancienne : si elle est plus récente que celle de l'image, `secmet-release verify --file` contrôle la signature de `SHA256SUMS` avec la clé de version (codée dans le script, la même que dans l'application) et l'empreinte du paquet, puis `dpkg-deb -x` déploie le paquet par-dessus le système en mémoire, sans passer par dpkg (les dépendances sont celles de l'image ; `apt-get install` et ses déclencheurs retardaient la session de près de 30 s, et la seule lecture de la base dpkg depuis le squashfs coûtait encore 17 à 21 s sur un portable modeste, mesuré le 2026-11-05 ; la base dpkg garde la version de l'image, sans conséquence sur un système qui repart de l'image à chaque démarrage) ; signature invalide ou installation échouée : version suivante, et à défaut celle de l'image ; chaque issue est tracée dans le journal (`secmet-app-update`) ;
   - `secmet-release` est livré dans les paquets `.deb` et `.rpm` à cet effet.
2. **Mises à jour du système** (noyau, bibliothèques, correctifs de sécurité) — en refaisant la clé :
   - pas de mise à jour du système live lui-même ;
   - l'image est reconstruite environ tous les 6 mois ou lors d'une faille importante ;
   - `secmet-make-key` réécrit la partition système seulement ; lot en cours, clé de signature et livrables sont conservés.

## N.7 Chaîne de publication : `make release`

La publication ne dépend pas de GitHub Actions : elle tourne sur le poste du responsable des versions, dans des conteneurs (podman ou docker). Elle reproduit le workflow `package.yml`.

Script : `packaging/release.sh` (`make release`, `make publish`). Il refuse un arbre de travail modifié ou un `HEAD` sans le tag `vX.Y.Z` ou `vX.Y.Z-rcN` (options `--allow-dirty`, `--allow-untagged` pour les essais) ; sur un tag `-rcN`, `--publish` est refusé.

1. Contrôles : `cargo fmt --check`, `clippy -D warnings`, tests du workspace, `cargo audit`, shellcheck.
2. Construction en conteneurs : `.deb` Ubuntu 24.04 et 22.04, `.rpm` Fedora 42, AppImage (Ubuntu 24.04).
3. Installation et contrôle de chaque paquet dans son conteneur (binaires présents, helper refusant d'être lancé hors protocole, TestDisk embarqué, policy PolicyKit) ; contrôle de la glibc requise par l'AppImage (≤ 2.39).
4. `secmet-release sign` : `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` signés avec la clé de version, puis `secmet-release verify` avec la clé publique.
5. `--publish` seulement : `gh release create vX.Y.Z` sur le dépôt avec les 7 fichiers, après confirmation explicite (`oui`). Une release déjà existante est refusée.

Sortie : `dist/release/<version>/`, avec les journaux de construction sous `logs/`. Le tag (`git tag -a vX.Y.Z`) et le push du code restent des gestes explicites du responsable des versions.

**Convention de version et de tag (décidée le 2026-11-12).**

- **Un seul numéro `X.Y.Z`** pour tout ce qui sort d'une release : application, helper, image de la clé, SecMet Clé USB (AppImage et `.exe`). C'est celui du `Cargo.toml` de la racine, repris par les six crates et par le nom de chaque fichier publié ; `release.sh` et `package.yml` refusent un tag qui ne lui correspond pas. SecMet Clé USB n'a pas de numéro propre : il est publié à chaque version, qu'il ait changé ou non, et vérifie l'image par sa propre version (`latest.json`), pas par la sienne.
- **Quand incrémenter.** `Z` : corrections et petites évolutions sans changement de comportement visible. `Y` : nouvel outil, nouveau format de clé, nouveau menu ou parcours de l'application, changement de ce que l'opérateur voit ou fait (`0.4.0` : SecMet Clé USB sous Windows et complétion de la GPT). `X` : pas de critère fixé pour `1.0.0` à ce jour.
- **Tag** : `vX.Y.Z`, annoté (`git tag -a vX.Y.Z`), posé sur `main`. La première ligne du message est « SecMet X.Y.Z » ; le corps, s'il y en a un, est le résumé des changements pour l'opérateur : il devient les notes de la release GitHub et les notes de `latest.json` (affichées par SecMet dans ses Réglages quand une nouvelle version est annoncée), en tête du texte générique (liste des fichiers, clé de version). Un tag léger ou sans corps donne le texte générique seul ; `release.sh --notes <fr.md>` a priorité.
- **Répétition générale** : un tag `vX.Y.Z-rcN` (même `Cargo.toml` que la version visée) déclenche `package.yml` en entier, image de la clé et `.exe` compris, mais le travail de publication est sauté ; les fichiers restent des artefacts du run. `release.sh` accepte ce tag et refuse `--publish`. Les tags `-rc` ne comptent pas comme release précédente pour les « deux cadences » (`live-mode.sh` ne lit que les releases publiées). Une fois l'essai concluant, le tag définitif `vX.Y.Z` est posé sur le même commit.

Outil `secmet-release` (crate `secmet-attest`, non livré dans les paquets) :

| Commande | Rôle |
|----------|------|
| `keygen <clé.hex>` | Crée la clé de version (fichier `0600`, clé publique à côté), affiche `public_key` et `key_id` ; refuse d'écraser |
| `sign <dossier> --key … --version … --date … --sums-url …` | Écrit `SHA256SUMS`, `SHA256SUMS.sig`, `latest.json` |
| `verify <dossier> [--trust …]` | Vérifie les deux signatures, l'empreinte de chaque fichier et la cohérence de la version ; code 0 seulement avec une clé de confiance |

Prérequis GitHub : une release exige un tag, donc le tag `vX.Y.Z` doit exister sur le dépôt ; `package.yml` (GitHub Actions, gratuit sur un dépôt public) construit, signe et publie à chaque push de tag, `make release` restant la voie locale.

**Image de la clé USB dans la release (réalisé 2026-11-06).** La chaîne construit aussi l'image de la clé (`make live`, conteneur privilégié) et son livrable `secmet-live_<version>_x86_64.tar` (`packaging/live/bundle_live.sh` : image, `secmet-make-key.sh`, `grub.cfg`, `secmet-release` pour Ubuntu 24.04, `LISEZMOI.txt`), refusé si l'image n'est pas de la version de `Cargo.toml`. Le livrable est listé dans `SHA256SUMS` signé comme les paquets (`secmet-release` accepte `.tar`) ; l'application, qui cherche ses paquets par nom, l'ignore. Dans `package.yml`, le travail `live` tourne sur un tag ou à la main (`docker --privileged`, 20 à 30 min) et `publish` l'attend. Sur le poste du responsable des versions, `packaging/release.sh` fait de même (`--skip-live` pour s'en passer) et, si KVM est disponible, exige que la clé virtuelle déploie la version la plus récente de son magasin au démarrage (`packaging/live/qemu-check.sh`, voir `packaging/live/README.md`). Ainsi chaque clé fabriquée depuis une release part avec la version publiée, et le script de mise à jour au démarrage est testé avant publication.

**Image prête à écrire (réalisé 2026-11-06).** Le même travail produit `secmet-key_<version>_x86_64.img.xz` (`make live-dist`, `packaging/live/make_key_image.sh --dist`) : l'image GPT complète de la clé, compressée (≈ 850 Mo), listée dans `SHA256SUMS` comme les autres fichiers. L'opérateur l'écrit avec SecMet Clé USB (ci-dessous), Raspberry Pi Imager, Rufus ou `dd` ([Fabriquer sa clé](../Fabriquer_sa_cle.md)) ; GRUB BIOS y est embarqué sans `grub-install` (secteur d'amorçage et partition « BIOS boot » écrits à la main dans le fichier), la clé de signature est créée au premier démarrage et la partition d'export agrandie. Sur le poste du responsable des versions, avec KVM, `packaging/live/image-check.sh` (`make live-dist-check`) démarre l'image en BIOS dans QEMU sur une clé virtuelle de 16 Go et vérifie ces deux actions depuis le journal et la partition de données.

**Deux cadences (réalisé 2026-11-09).** Le système de la clé (squashfs, noyau, GRUB, schéma de l'image) change rarement, le paquet à chaque version. `packaging/live/live-mode.sh` compare, entre le tag publié précédent et le tag à publier, les fichiers qui entrent dans ce système (`live-mode.sh --inputs`). Inchangés : `package.yml` saute le travail `live` et, dans `publish`, `packaging/live/refresh_live.sh` télécharge l'image et le livrable de la release précédente, les vérifie avec la clé de version, refait la seule partition de données de l'image (même schéma, sans clé de signature) avec un magasin `app/<version>/` contenant le nouveau `.deb` Ubuntu 24.04 signé, recompresse et refait le livrable (`dist/live/app/<version>/` dedans, copié par `secmet-make-key.sh`) ; la clé déploie la version au démarrage (§N.6), exactement comme après « Installer la mise à jour ». La release passe ainsi d'un quart d'heure à quelques minutes, sans conteneur privilégié ni cache à entretenir. Un fichier du système modifié, ou `full_live` à la main (pour rafraîchir la base Ubuntu), déclenche la construction complète. La comparaison de tags remplace tout registre : une release a toujours été construite ou reprise depuis ces fichiers dans l'état de son tag. `packaging/release.sh` suit la même règle (`--full-live` pour forcer) et, avec KVM, `image-check.sh` vérifie en plus que la version du magasin a été déployée.

**SecMet Clé USB (réalisé 2026-11-10 ; Windows validé le 2026-11-11 en machine virtuelle Windows 11 avec une clé réelle).** balenaEtcher échouant sur les images `.img.xz` (problème connu, non corrigé) et les autres outils n'offrant aucune vérification de signature, chaque release publie aussi `secmet-key-writer_<version>_x86_64.AppImage` (`crates/secmet-key-writer`, Rust, interface egui, un seul binaire sans bibliothèque embarquée ; `make key-writer-appimage`, travail `key-writer` de `package.yml`, listé dans `SHA256SUMS`). L'outil lit `latest.json` (signé) pour connaître la version courante, télécharge l'image ou prend un fichier local (l'image de la version courante déjà dans le dossier des téléchargements est prise d'office au lancement, sans clic ; après un téléchargement, vérification puis choix de la clé s'enchaînent sans clic non plus), vérifie `SHA256SUMS.sig` avec la clé de version intégrée puis l'empreinte de l'image, ne propose que les disques amovibles de 12 Go ou plus sans partition système, puis relance le même exécutable en `--worker` par `pkexec` : démontage, ouverture exclusive du disque (`O_EXCL`), décompression xz en Rust pur (blocs décodés en parallèle), écriture, `fsync`, vidage du cache des blocs (`BLKFLSBUF`), relecture complète et comparaison des empreintes, relecture de la table de partitions. L'image publiée doit se nommer `secmet-key_<version>_x86_64.img.xz` (la version en est tirée pour retrouver `SHA256SUMS`), sauf si `SHA256SUMS` et `SHA256SUMS.sig` sont à côté du fichier. `secmet-key-writer --verify <image>` fait la même vérification en ligne de commande. Windows : même exécutable (`secmet-key-writer_<version>_x86_64.exe`, travail `key-writer-windows` de `package.yml` sur un runner Windows, ou `make key-writer-exe` en compilation croisée MinGW, sur la machine ou dans le conteneur Ubuntu 24.04 de l'AppImage), sans console ni bibliothèque à côté (TLS Schannel). Rendu par wgpu sur Direct3D 12 (dorsal `dx12` seul) et non OpenGL : le pilote d'affichage de base de Windows (machines virtuelles, bureau à distance, postes sans pilote graphique) ne fournit qu'OpenGL 1.1, où egui refuse de démarrer, alors que Direct3D 12 a toujours le repli logiciel WARP. Les coches et croix de l'interface sont ✔ et ✖ (U+2714, U+2716), seuls glyphes de ce type dans les polices embarquées d'egui. L'interface se relance en administrateur au lancement (`ShellExecuteEx` avec le verbe `runas`, invite UAC, comme Rufus) et le processus `--worker` en hérite ; disques par `IOCTL_STORAGE_QUERY_PROPERTY` et `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` sur `\\.\PhysicalDriveN` (amovible = bus USB, SD, MMC ou média amovible, pas le bit hotplug que portent les ports SATA internes ; volumes et lettres par `FindFirstVolume` et `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` ; le lecteur `%SystemDrive%` marque le disque système) ; avant l'écriture chaque volume de la clé est verrouillé (`FSCTL_LOCK_VOLUME`, dix essais) et démonté (`FSCTL_DISMOUNT_VOLUME`), poignées gardées ouvertes jusqu'à la fin pour que Windows ne remonte rien ; écriture et relecture sans cache (`FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`, lectures de taille alignée), puis `IOCTL_DISK_UPDATE_PROPERTIES`. **GPT complétée pendant l'écriture (`gpt.rs`, Linux comme Windows) :** l'image fait 11 Gio, sa GPT de secours est donc au bout de l'image et non de la clé ; Linux s'en contente, mais Windows, dès qu'il ré-analyse le disque (fermeture d'une poignée d'écriture, relâchement des verrous, `IOCTL_DISK_UPDATE_PROPERTIES`), « répare » la table à sa façon — en-tête primaire réécrit, entrées déplacées, et la signature `55 AA` du MBR protecteur écrasée par `EFI ` (constaté sur Windows 11 25H2, reproduit en PowerShell : le secteur 0 est intact tant que la GPT n'est pas ré-analysée) : la clé devient illisible pour Linux et non démarrable. Le worker écrit donc une table complète d'emblée : dès le premier bloc décodé, il écrit les entrées et l'en-tête de secours au bout du disque (avant le premier bloc, pour qu'aucune ré-analyse ne trouve jamais une table incomplète), puis le bloc 0 avec l'en-tête primaire corrigé (`AlternateLBA`, `LastUsableLBA`, CRC) et l'entrée protectrice étendue au disque ; la relecture compare ces secteurs à part et l'empreinte affichée reste celle de l'image. Avec une GPT complète Windows ne touche à rien (vérifié en PowerShell puis avec l'outil). `secmet-first-boot` relocalise de toute façon la GPT de secours (`sfdisk --relocate gpt-bak-std`) avant d'agrandir l'export. SmartScreen avertit (éditeur inconnu, pas de certificat de signature de code) : *Informations complémentaires* → *Exécuter quand même*. Le glisser-déposer d'un fichier vers une fenêtre élevée est bloqué par Windows (UIPI) : passer par *Choisir le fichier*. En ligne de commande (`--list-disks`, `--verify`), PowerShell et cmd rendent la main avant la sortie d'un exécutable sans console : `.\secmet-key-writer.exe --list-disks | Out-Host` pour l'attendre.

### Intégration continue

- Les workflows GitHub Actions (`ci.yml`, `fuzz.yml`, `package.yml`) restent dans le dépôt ; ils ne sont **pas** nécessaires pour publier.
- Aucun abonnement payant n'est prévu. Quand le dépôt du code sera public, les Actions redeviendront gratuites (machines standard de GitHub) pour les tests à chaque push et pull request ; la signature des versions restera locale.
- Un runner auto-hébergé ou une forge locale (Forgejo, Gitea) restent possibles, mais ajoutent un service à maintenir : non retenus en v1.

## N.8 Confidentialité

La vérification ne transmet que l'adresse IP de l'atelier et la version utilisée (requête HTTPS vers l'hébergement des releases). Aucune donnée de disque, de certificat ou d'opérateur n'est envoyée.

## N.9 Recette prévue

| Scénario | Résultat attendu |
|----------|------------------|
| Poste hors ligne | SecMet démarre normalement ; « dernière vérification » inchangée ; aucun message d'erreur |
| Nouvelle version publiée | Point jaune sur Réglages ; version, date et notes affichées ; le lien ouvre la page de la version |
| `latest.json` altéré ou signé par une autre clé | Ignoré : aucune annonce, « Vérification impossible » dans les Réglages |
| Case « Vérifier automatiquement » décochée | Aucune requête réseau ; « Vérifier maintenant » reste possible |
| Clé live, lot ouvert | Mise à jour proposée, appliquée après la clôture |
| Clé live, nouvelle version défaillante | Retour automatique à la version précédente au démarrage suivant |

## N.10 Estimation

| Bloc | Charge |
|------|--------|
| `make release`, releases GitHub, `latest.json` signé | 1 à 2 j |
| Vérification en arrière-plan, point jaune, section Mises à jour | 1 à 2 j |
| ~~Téléchargement et installation depuis l'application~~ | abandonné (pas de bouton Installer) |
| Clé live : versions sur la partition de données, service de démarrage, retour arrière | 2 à 3 j |
| **Total** | **1 à 1,5 semaine** |

---

## Documents connexes

| Document | Objet |
|----------|-------|
| [Annexe M — Clé live et lots persistants](Annexe_M_cle_live_lots.md) | Structure de la clé, lots |
| [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md) | Installation et mise à jour manuelles actuelles |
| [README](../../README.md) | Commandes Make et intégration continue |
