# Annexe N — Mises à jour et publication des versions

> Documentation SecMet — v0.4.0
>
> **Statut** : **réalisée** (0.4.0, publiée le 2026-11-12) : releases GitHub du dépôt du code construites, signées et publiées par `package.yml` à chaque tag, chaîne locale `make release` / `make publish` (§N.7), binaire `secmet-release`, vérification dans l'application avec point jaune, version annoncée et lien vers la page de téléchargement (§N.2), bouton **Installer la mise à jour** pour les paquets `.deb` et `.rpm` (§N.5 ; l'AppImage reste manuelle, bouton masqué), versions de SecMet sur la clé live (§N.6), image de la clé et SecMet Clé USB dans chaque release (§N.7). Depuis la 0.4.1, l'installation depuis l'application est bloquée pendant toute opération et tant qu'un lot est ouvert (§N.1). Écarts avec la spécification initiale : la clé de version signe aussi en CI (§N.4). Depuis la 0.5.0, sur la clé, le déploiement au démarrage (`secmet-app-update`) applique la même garde : une version n'entre pas au milieu d'un lot ouvert sous une autre (§N.6).

---

## N.1 Principes

1. **Seules les versions publiées sont proposées** : une release taguée (`vX.Y.Z`), jamais un simple push sur `main`.
2. **Les mises à jour sont signées** : l'application refuse tout paquet dont la signature ou l'empreinte ne correspond pas. Le HTTPS ne suffit pas : un compte d'hébergement compromis ne doit pas permettre de diffuser un helper root piégé.
3. **Le hors ligne ne bloque jamais** : sans réseau, SecMet fonctionne avec la version installée, sans message d'erreur.
4. **L'opérateur décide** : une mise à jour est signalée, jamais imposée.
5. **Jamais au mauvais moment** (0.4.1, décision D14 : blocage partout, sur un poste comme sur la clé) : l'annonce n'interrompt rien, et le bouton **Installer la mise à jour** est masqué tant qu'une opération est en cours (effacement, analyse des disques, TestDisk, chargement de la liste des disques, génération d'un PDF, clôture de lot, déverrouillage, dé-frozen) **ou qu'un lot est ouvert**. L'état est relu en continu tant que les Réglages sont ouverts (toutes les demi-secondes) et revérifié au clic ; la raison s'affiche sous le bouton (« Installation impossible pour l'instant : effacement en cours. », « Lot L-… ouvert : clôturez-le avant d'installer la mise à jour… ») et, quand un lot est ouvert, un bouton **Clôturer le lot…** ouvre le dialogue de clôture, après quoi le bouton d'installation réapparaît. Pendant l'installation, ni les Réglages ni la fenêtre principale ne se ferment ; après une installation réussie, aucun lot ne s'ouvre sous l'ancien binaire (« Mode lot » coché dans les Réglages : le dialogue de lot attend la relance). Un lot n'est ainsi traité que par une seule version ; chaque attestation indique de toute façon la sienne (champ `software`). Module : `src/busy.rs` (gardes des opérations, fonction pure `install_blocker`). Sur la clé, le déploiement au démarrage applique la même règle depuis la 0.5.0 (§N.6).

## N.2 Ce que voit l'opérateur

- Au démarrage puis toutes les 24 h, SecMet vérifie en arrière-plan si une version plus récente est publiée (délai court, échec silencieux).
- Si oui, un **point jaune** apparaît sur l'icône **Réglages**.
- Les Réglages affichent une section **Mises à jour** : version installée, version disponible, sa date et ses notes, date de la dernière vérification, bouton **Vérifier maintenant** et bouton **Ouvrir la page de téléchargement**.
- Sur un poste qui a `apt-get` ou `dnf` (paquets `.deb` et `.rpm`, clé live), bouton **Installer la mise à jour**, puis **Relancer SecMet** une fois l'installation faite (§N.5, §N.6), sauf opération en cours ou lot ouvert (§N.1, principe 5). L'installation passe par `pkexec` et le gestionnaire de paquets, pas par le helper. **AppImage** (variable `APPIMAGE` posée par le runtime et fichier présent, décision D15) : pas de bouton, même sur Ubuntu ou Fedora — le paquet installé écraserait le composant système posé par l'AppImage, qui alternerait ensuite entre deux versions ; la section affiche « AppImage : téléchargez la nouvelle AppImage et remplacez le fichier ; le composant système sera mis à jour au lancement suivant » avec le lien de téléchargement. Sans `apt-get` ni `dnf`, seul le lien de téléchargement est proposé.
- La case « Vérifier automatiquement les mises à jour » désactive la requête quotidienne ; « Vérifier maintenant » reste possible.

### Réalisation

- `src/update.rs` : adresse du canal (`SECMET_UPDATE_URL`, réglage `update_url`, `moved_to` signé, défaut), téléchargement avec délai de 8 s et corps borné à 64 Kio, `latest.json` accepté seulement si signé par une clé de `RELEASE_PUBLIC_KEYS`, état dans `update-state.json` du dossier de configuration (`~/.config/secmet/`, ou `config/` de la partition de données sur la clé) (`checked_at`, dernier `latest` valide, erreur).
- `src/ui/update_ui.rs` : vérification sur un fil dédié au démarrage si plus de 24 h se sont écoulées, minuteur horaire (robuste à la veille), pastille `update-dot` en surimpression de l'engrenage.
- Client HTTPS : `ureq` avec `rustls` et les racines `webpki-roots` (pas d'OpenSSL de l'hôte, compatible AppImage).

## N.3 Canal de publication

| Phase | Emplacement |
|-------|-------------|
| Développement | Releases GitHub du dépôt du code **`hclcdev12/secmet`** (public, GPL-3.0-or-later). Jusqu'à la 0.3.5 le canal était `hclcdev12/secmet-releases` ; sa dernière release ne contient qu'un `latest.json` signé dont `sha256sums_url` et `moved_to` renvoient vers `secmet`, ce qui fait basculer les installations existantes. |
| Production | Le dépôt `hclcdev12/secmet` est l'emplacement actuel, **provisoire** ; le dépôt définitif de l'entreprise reste à décider au lancement. Le déménagement est prévu par `moved_to` (ci-dessous) |

Chaque release contient **14 fichiers** (11 en 0.4.0, avant le second `.rpm` et les binaires autonomes) :

| Fichier | Rôle |
|---------|------|
| `secmet_<version>.ubuntu24.04_amd64.deb`, `secmet_<version>.ubuntu22.04_amd64.deb` | Paquets Debian / Ubuntu / Mint (version Debian `<version>~<distro>` dans le paquet ; nom publié selon la convention de GitHub, qui remplace `~` par `.` dans les noms de fichiers d'une release) |
| `secmet-<version>-1.fc44.x86_64.rpm`, `secmet-<version>-1.fc42.x86_64.rpm` | Paquets Fedora : `fc44` pour Fedora 44 et 45 ; `fc42` publié en plus en 0.4.1, le temps de la transition (les SecMet 0.4.0 cherchent ce nom pour se mettre à jour), puis retiré |
| `SecMet_<version>_x86_64.AppImage` | AppImage (contient aussi `secmet-verify` et `secmet-release` : `./SecMet_<version>_x86_64.AppImage verify …`, `… release …`) |
| `secmet-verify_<version>_x86_64`, `secmet-release_<version>_x86_64` | Outils en ligne de commande en binaires Linux autonomes (construits avec le `.deb` Ubuntu 22.04, glibc ≥ 2.35) : vérification des certificats et des manifestes de lot, vérification d'une release ; `chmod +x` après téléchargement |
| `secmet-key_<version>_x86_64.img.xz` | Image de la clé USB prête à écrire ([Fabriquer sa clé](../Fabriquer_sa_cle.md)) |
| `secmet-live_<version>_x86_64.tar` | Livrable de fabrication de la clé sous Linux (`secmet-make-key.sh`, image, `secmet-release`) |
| `secmet-key-writer_<version>_x86_64.AppImage`, `secmet-key-writer_<version>_x86_64.exe` | SecMet Clé USB, outil d'écriture de la clé, pour Linux et Windows |
| `SHA256SUMS` | Empreintes de tous les fichiers ci-dessus |
| `SHA256SUMS.sig` | Signature des empreintes avec la clé de version (§N.4) : JSON `{alg, key_id, public_key, sha256, signature}` |
| `latest.json` | Annonce signée de la version (ci-dessous) |

Le fichier **`latest.json`** signé est celui que l'application consulte :

```json
{
  "alg": "Ed25519", "key_id": "…", "public_key": "…",
  "payload": {
    "version": "X.Y.Z",
    "published_at": "AAAA-MM-JJ",
    "notes": { "fr": "…", "en": "…", "de": "…" },
    "sha256sums_url": "https://github.com/<dépôt>/releases/download/vX.Y.Z/SHA256SUMS",
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
- **Où elle sert aujourd'hui** : à deux endroits.
  - **En CI** (voie normale de publication) : `package.yml` lit la clé privée dans le secret `SECMET_RELEASE_KEY` de l'environnement GitHub `release`, que seul le travail `publish` déclare ; les autres travaux n'ont que la lecture du dépôt, et toutes les actions sont épinglées par empreinte de commit. Le travail écrit la clé dans un fichier temporaire du runner et signe `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` (ainsi que le magasin `app/` de l'image de clé reprise, §N.7) à chaque push d'un tag `vX.Y.Z`, puis publie la release. L'environnement peut exiger une approbation manuelle avant la publication (facultatif). Les journaux, publics, n'affichent pas le secret. Décision du 2026-11-20 : la publication reste automatique au push d'un tag ; pas de signature à la main.
  - **En local** : `packaging/release.sh` (`make release`) signe sur le poste du responsable des versions avec la clé du fichier `SECMET_RELEASE_KEY` (par défaut dans `~/.config/secmet/`).
  - Dans les deux cas, la release signée est vérifiée avec la clé publique **attendue** (`SECMET_TRUST` de `package.yml`, la même que `RELEASE_PUBLIC_KEYS` de `src/update.rs`), pas avec celle que porte le `latest.json` produit : si le secret (ou le fichier local) contient une autre clé, le travail échoue avant toute publication (« la clé du secret SECMET_RELEASE_KEY ne correspond pas à la clé de version attendue »), au lieu de publier une release que toutes les installations et SecMet Clé USB refuseraient.
- **Risque** : la signature protège contre la compromission de l'hébergement des fichiers (un fichier remplacé sur la page de release ne passe pas la vérification). Elle **ne protège pas** contre la compromission du dépôt, d'un compte qui peut y pousser un tag ou modifier les workflows, ou de la CI : un tag poussé par un tiers produit une release signée que les applications installées acceptent. Parades en place : secret limité à l'environnement `release` et au travail `publish`, permissions minimales, actions épinglées ; à régler dans GitHub : création des tags `v*` réservée aux administrateurs, double authentification, approbation manuelle facultative.
- La clé publique est **intégrée à l'application** à la compilation.
- **Sauvegarde hors ligne obligatoire** : sans elle, plus aucune mise à jour automatique n'est possible sans réinstallation manuelle.
- **Changement de clé** : la nouvelle clé publique est livrée par une version signée avec l'ancienne. L'application (`RELEASE_PUBLIC_KEYS`, `src/update.rs`) et SecMet Clé USB (`crates/secmet-key-writer/src/release.rs`) acceptent une **liste** de clés : ajouter une clé, c'est ajouter une ligne aux deux listes (un test vérifie qu'elles concordent), l'ancienne restant listée le temps de la transition. Le script de démarrage de la clé `secmet-app-update` a sa propre liste (`TRUST_KEYS`, une clé par ligne, depuis la 0.5.0) : il passe chaque clé à `secmet-release verify --trust`, une signature par l'une d'elles suffit, et un test vérifie qu'elle est identique à `RELEASE_PUBLIC_KEYS`. Comme ce script est dans l'image, la nouvelle clé doit y figurer dans une image publiée avant la première version signée par elle. Décision D7 (2026-11-20) : pas de changement de clé pour l'instant.

## N.5 Installation selon le livrable

**Depuis l'application** (`.deb`, `.rpm`) : la section Mises à jour affiche **Installer la mise à jour** quand une version plus récente est connue, qu'aucune opération n'est en cours et qu'aucun lot n'est ouvert (§N.1). SecMet télécharge `SHA256SUMS` et `SHA256SUMS.sig` depuis le dossier de la version, vérifie la signature (clé de version embarquée), télécharge le paquet de la plateforme (`.deb` Ubuntu 24.04 si la glibc est ≥ 2.39, sinon 22.04 ; `.rpm` si `dnf` est présent, choisi parmi les `secmet-<version>-1.fc<N>.x86_64.rpm` listés par `SHA256SUMS` : celui de la Fedora du poste (`VERSION_ID` de `/etc/os-release`), sinon le plus récent qui ne la dépasse pas, sinon le plus ancien publié) dans `/tmp/secmet-update-<uid>/`, compare son empreinte à `SHA256SUMS`, puis lance `pkexec apt-get install -y --allow-downgrades <paquet>` (ou `pkexec dnf install -y`) : PolicyKit demande le mot de passe administrateur. Rien n'est exécuté sans empreinte vérifiée. En cas de succès, **Relancer SecMet** démarre la nouvelle version. Module : `src/update_install.rs`. **Essai sans publier** : un canal servi sur la boucle locale (`http://127.0.0.1:<port>/`) est accepté par la validation de `latest.json` et par `secmet-release sign --sums-url`, uniquement pour cette adresse ; avec `SECMET_UPDATE_URL=http://127.0.0.1:<port>/latest.json`, le cycle complet (point jaune, téléchargement, vérification, installation) se teste sur un poste avant tout `make publish`.

**À la main** : l'opérateur ou l'administrateur suit le lien de la section Mises à jour, télécharge le livrable et vérifie l'empreinte (`sha256sum -c SHA256SUMS`, ou `secmet-release verify`), puis :

| Livrable | Installation |
|----------|--------------|
| Banc fixe, `.deb` | `sudo apt install ./secmet_<version>.ubuntu24.04_amd64.deb`. Évolution possible si les bancs installés se multiplient : dépôt APT signé, et `apt upgrade` fait le reste |
| `.rpm` | `sudo dnf install ./secmet-<version>-1.fc44.x86_64.rpm` (Fedora 44 et 45 ; en 0.4.1, paquet `fc42` pour une Fedora 42 ou 43) |
| AppImage | Remplacer le fichier par la nouvelle AppImage (l'application ne propose pas de bouton **Installer** dans une AppImage) ; au lancement suivant, le dialogue « Mise à jour du composant système » met le helper à jour (un mot de passe administrateur) |

## N.6 Clé live

Le système de la clé est en lecture seule ; SecMet fait partie de l'image. Deux niveaux :

1. **Mises à jour de SecMet** — sur la partition de données (**réalisé en 0.3.4**) :
   - le bouton **Installer la mise à jour** fonctionne comme sur un poste (installation pour la session, en mémoire ; masqué tant qu'un lot est ouvert, §N.1) et, sur la clé, range en plus le paquet vérifié avec `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` dans `SECMET-DATA/app/<version>/` ; seules les deux versions les plus récentes sont gardées ; le dossier `app/` appartient à l'opérateur (créé par `secmet-make-key`, et au démarrage par `secmet-app-update` s'il manque : sans lui, l'installation échoue par « Permission non accordée ») ;
   - au démarrage, avant la session (`secmet-app-update.service`, après le montage de la partition, avant LightDM), un script parcourt `app/` de la version la plus récente à la plus ancienne : si elle est plus récente que celle de l'image, `secmet-release verify --file` contrôle la signature de `SHA256SUMS` avec la clé de version (codée dans le script, la même que dans l'application) et l'empreinte du paquet, puis `dpkg-deb -x` déploie le paquet par-dessus le système en mémoire, sans passer par dpkg (les dépendances sont celles de l'image ; `apt-get install` et ses déclencheurs retardaient la session de près de 30 s, et la seule lecture de la base dpkg depuis le squashfs coûte encore une vingtaine de secondes sur un portable modeste ; la base dpkg garde la version de l'image, sans conséquence sur un système qui repart de l'image à chaque démarrage) ; signature invalide ou installation échouée : version suivante, et à défaut celle de l'image ; chaque issue est tracée dans le journal (`secmet-app-update`) ; **garde « lot ouvert »** (0.5.0, décision D14) : un lot de portée « machines » survit aux redémarrages, et un paquet rangé dans `app/` dont l'installation pour la session a échoué (mot de passe refusé), suivi de l'ouverture d'un lot, entrerait sinon au milieu de ce lot ; le script lit donc chaque `lots/<id>/lot.json` (statut et champ `software`, écrits à l'ouverture du lot) et ne déploie la version V que si tous les lots ouverts portent `"software": "secmet V"` ; sinon V est **reportée** (« version V reportée : lot L-… ouvert sous secmet X ; déploiement au démarrage qui suivra sa clôture » au journal) et une version plus ancienne du magasin reste candidate, de sorte que le lot garde sa version (celle de l'image, ou celle du magasin sous laquelle il a été ouvert) ; un lot ouvert dont le statut ou la version ne se lit pas compte comme ouvert sous une version inconnue (rien n'est déployé) ; les lots clôturés ou abandonnés ne comptent pas ; la version entre au premier démarrage après la clôture ;
   - `secmet-release` est livré dans les paquets `.deb` et `.rpm` à cet effet (et aussi dans l'AppImage et en binaire autonome, pour vérifier une release téléchargée).
2. **Mises à jour du système** (noyau, bibliothèques, correctifs de sécurité) — en refaisant la clé :
   - pas de mise à jour du système live lui-même ;
   - l'image est reconstruite environ tous les 6 mois ou lors d'une faille importante ;
   - `secmet-make-key.sh --system-only` (dépôt ou livrable `.tar`, sous Linux) réécrit la partition système seulement ; lot en cours, clé de signature et livrables sont conservés. Réécrire l'image publiée `.img.xz` efface au contraire toute la clé ([Fabriquer sa clé](../Fabriquer_sa_cle.md), « Mettre à jour »).

## N.7 Chaîne de publication : `make release`

Deux voies produisent la même release. **En pratique**, c'est `package.yml` (GitHub Actions) qui construit, signe et publie, au push d'un tag `vX.Y.Z` (§N.4). **La voie locale**, ci-dessous, tourne sur le poste du responsable des versions, dans des conteneurs (podman ou docker), sans dépendre de GitHub Actions ; elle reproduit `package.yml` pour la construction, la signature et la publication.

Script : `packaging/release.sh` (`make release`, `make publish`). Il refuse un arbre de travail modifié ou un `HEAD` sans le tag `vX.Y.Z` ou `vX.Y.Z-rcN` (options `--allow-dirty`, `--allow-untagged` pour les essais) ; sur un tag `-rcN`, `--publish` est refusé.

1. Contrôles : `cargo fmt --check`, `clippy -D warnings`, tests du workspace, `cargo audit`, shellcheck.
2. Construction en conteneurs : `.deb` Ubuntu 24.04 et 22.04, `.rpm` Fedora 44 et 42, AppImage (Ubuntu 24.04), binaires autonomes `secmet-verify` et `secmet-release` (conteneur du `.deb` Ubuntu 22.04), SecMet Clé USB (AppImage et `.exe`), image de la clé et livrable `.tar` (sauf `--skip-live`, voir plus bas). Les fichiers d'une construction précédente (paquets, outils, image et `.tar`) sont d'abord effacés de `dist/release/<version>/`.
3. Installation et contrôle de chaque paquet dans son conteneur (binaires présents, helper refusant d'être lancé hors protocole, TestDisk embarqué, policy PolicyKit, documentation installée par `packaging/check-installed-docs.sh`) ; installation seule du `.deb` Ubuntu 24.04 dans `debian:13` et du `.deb` Ubuntu 22.04 dans `debian:12` (`packaging/tests/install-deb.sh` : dépendances résolues par `apt-get`, `pkexec` présent ; travail `deb-install` de `package.yml`, qui conditionne la publication) ; contrôle de la glibc requise par l'AppImage (≤ 2.39) et par les binaires autonomes (≤ 2.35) ; AppImage : `secmet-verify` et `secmet-release` présents et aiguillés par `AppRun`, copyright de chaque bibliothèque embarquée (`packaging/appimage-smoke.sh`).
4. `secmet-release sign` : `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` signés avec la clé de version, puis `secmet-release verify` avec la clé publique **attendue** (celle que l'application et SecMet Clé USB acceptent, `SECMET_RELEASE_TRUST` pour une autre), jamais avec celle du `latest.json` produit : une clé privée qui ne lui correspond pas arrête la chaîne. Les 11 livrables attendus sont contrôlés ; il en manque un (image de la clé avec `--skip-live`, par exemple) : avertissement, et `--publish` refusé.
5. `--publish` seulement, après confirmation explicite (`oui`) : la liste envoyée est tirée de `SHA256SUMS` (2e colonne) plus `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` (échec si un fichier manque), donc exactement ce qui est signé ; `gh release create vX.Y.Z --draft`, contrôle des fichiers du brouillon, puis `gh release edit --draft=false`. « latest » ne pointe ainsi jamais sur une release incomplète. Une release déjà existante (brouillon compris) est refusée. `package.yml` publie de la même façon ; un brouillon resté d'une exécution interrompue y est remplacé, une release publiée jamais touchée.

Sortie : `dist/release/<version>/`, avec les journaux de construction sous `logs/`. Le tag (`git tag -a vX.Y.Z`) et le push du code restent des gestes explicites du responsable des versions.

**Convention de version et de tag (décidée le 2026-11-12).**

- **Un seul numéro `X.Y.Z`** pour tout ce qui sort d'une release : application, helper, image de la clé, SecMet Clé USB (AppImage et `.exe`). C'est celui du `Cargo.toml` de la racine, repris par les six crates et par le nom de chaque fichier publié ; `release.sh` et `package.yml` refusent un tag qui ne lui correspond pas. SecMet Clé USB n'a pas de numéro propre : il est publié à chaque version, qu'il ait changé ou non, et vérifie l'image par sa propre version (`latest.json`), pas par la sienne.
- **Quand incrémenter.** `Z` : corrections et petites évolutions sans changement de comportement visible. `Y` : nouvel outil, nouveau format de clé, nouveau menu ou parcours de l'application, changement de ce que l'opérateur voit ou fait (`0.4.0` : SecMet Clé USB sous Windows et complétion de la GPT). `X` : pas de critère fixé pour `1.0.0` à ce jour.
- **Tag** : `vX.Y.Z`, annoté (`git tag -a vX.Y.Z`), posé sur `main`. La première ligne du message est « SecMet X.Y.Z » ; le corps, s'il y en a un, est le résumé des changements pour l'opérateur : il devient les notes de la release GitHub et les notes de `latest.json` (affichées par SecMet dans ses Réglages quand une nouvelle version est annoncée), en tête du texte générique (liste des fichiers, clé de version). Un tag léger ou sans corps donne le texte générique seul ; `release.sh --notes <fr.md>` a priorité.
- **Répétition générale** : un tag `vX.Y.Z-rcN` (même `Cargo.toml` que la version visée) déclenche `package.yml` en entier, image de la clé et `.exe` compris, mais le travail de publication est sauté ; les fichiers restent des artefacts du run. `release.sh` accepte ce tag et refuse `--publish`. Les tags `-rc` ne comptent pas comme release précédente pour les « deux cadences » (`live-mode.sh` ne lit que les releases publiées). Une fois l'essai concluant, le tag définitif `vX.Y.Z` est posé sur le même commit.

Outil `secmet-release` (crate `secmet-attest`, livré dans les `.deb` et `.rpm` en `/usr/bin/secmet-release`, et dans le livrable `.tar`) :

| Commande | Rôle |
|----------|------|
| `keygen <clé.hex>` | Crée la clé de version (fichier `0600`, clé publique à côté), affiche `public_key` et `key_id` ; refuse d'écraser |
| `sign <dossier> --key … --version … --date … --sums-url …` | Écrit `SHA256SUMS`, `SHA256SUMS.sig`, `latest.json` |
| `verify <dossier> [--trust …] [--file <nom>]` | Vérifie les deux signatures, l'empreinte de chaque fichier (ou du seul fichier nommé) et la cohérence de la version ; code 0 seulement avec une clé de confiance (clé publique ou `key_id`) |

Prérequis GitHub : une release exige un tag, donc le tag `vX.Y.Z` doit exister sur le dépôt ; `package.yml` (GitHub Actions, gratuit sur un dépôt public) construit, signe et publie à chaque push de tag `vX.Y.Z`, `make release` restant la voie locale.

**Image de la clé USB dans la release (réalisé 2026-11-06).** La chaîne construit aussi l'image de la clé (`make live`, conteneur privilégié) et son livrable `secmet-live_<version>_x86_64.tar` (`packaging/live/bundle_live.sh` : image, `secmet-make-key.sh`, `grub.cfg`, `secmet-release` pour Ubuntu 24.04, `LISEZMOI.txt`), refusé si l'image n'est pas de la version de `Cargo.toml`. Le livrable est listé dans `SHA256SUMS` signé comme les paquets (`secmet-release` accepte `.tar`) ; l'application, qui cherche ses paquets par nom, l'ignore. Dans `package.yml`, le travail `live` tourne sur un tag ou à la main (`docker --privileged`, 20 à 30 min) et `publish` l'attend. Sur le poste du responsable des versions, `packaging/release.sh` fait de même (`--skip-live` pour s'en passer) et, si KVM est disponible, exige que la clé virtuelle déploie la version la plus récente de son magasin au démarrage (`packaging/live/qemu-check.sh`, voir `packaging/live/README.md`). Ainsi chaque clé fabriquée depuis une release part avec la version publiée, et le script de mise à jour au démarrage est testé avant publication.

**Image prête à écrire (réalisé 2026-11-06).** Le même travail produit `secmet-key_<version>_x86_64.img.xz` (`make live-dist`, `packaging/live/make_key_image.sh --dist`) : l'image GPT complète de la clé, compressée (≈ 850 Mo), listée dans `SHA256SUMS` comme les autres fichiers. L'opérateur l'écrit avec SecMet Clé USB (ci-dessous), Raspberry Pi Imager, Rufus ou `dd` ([Fabriquer sa clé](../Fabriquer_sa_cle.md)) ; GRUB BIOS y est embarqué sans `grub-install` (secteur d'amorçage et partition « BIOS boot » écrits à la main dans le fichier), la clé de signature est créée au premier démarrage et la partition d'export agrandie. Sur le poste du responsable des versions, avec KVM, `packaging/live/image-check.sh` (`make live-dist-check`) démarre l'image en BIOS dans QEMU sur une clé virtuelle de 16 Go et vérifie ces deux actions depuis le journal et la partition de données.

**Deux cadences (réalisé 2026-11-09).** Le système de la clé (squashfs, noyau, GRUB, schéma de l'image) change rarement, le paquet à chaque version. `packaging/live/live-mode.sh` compare, entre le tag publié précédent et le tag à publier, les fichiers qui entrent dans ce système (`live-mode.sh --inputs`). Inchangés : `package.yml` saute le travail `live` et, dans `publish`, `packaging/live/refresh_live.sh` télécharge l'image et le livrable de la release précédente, les vérifie avec la clé de version, refait la seule partition de données de l'image (même schéma, sans clé de signature) avec un magasin `app/<version>/` contenant le nouveau `.deb` Ubuntu 24.04 signé, recompresse et refait le livrable (`dist/live/app/<version>/` dedans, copié par `secmet-make-key.sh`) ; la clé déploie la version au démarrage (§N.6), exactement comme après « Installer la mise à jour ». La release passe ainsi d'un quart d'heure à quelques minutes, sans conteneur privilégié ni cache à entretenir. Un fichier du système modifié, ou `full_live` à la main (pour rafraîchir la base Ubuntu), déclenche la construction complète. La comparaison de tags remplace tout registre : une release a toujours été construite ou reprise depuis ces fichiers dans l'état de son tag. `packaging/release.sh` suit la même règle (`--full-live` pour forcer) et, avec KVM, `image-check.sh` vérifie en plus que la version du magasin a été déployée.

**SecMet Clé USB (réalisé 2026-11-10 ; Windows validé le 2026-11-11 en machine virtuelle Windows 11 avec une clé réelle).** balenaEtcher échouant sur les images `.img.xz` (problème connu, non corrigé) et les autres outils n'offrant aucune vérification de signature, chaque release publie aussi `secmet-key-writer_<version>_x86_64.AppImage` (`crates/secmet-key-writer`, Rust, interface egui, un seul binaire sans bibliothèque embarquée ; `make key-writer-appimage`, travail `key-writer` de `package.yml`, listé dans `SHA256SUMS`). L'outil lit `latest.json` (signé) pour connaître la version courante, télécharge l'image ou prend un fichier local (l'image de la version courante déjà dans le dossier des téléchargements est prise d'office au lancement, sans clic ; après un téléchargement, vérification puis choix de la clé s'enchaînent sans clic non plus), vérifie `SHA256SUMS.sig` avec la clé de version intégrée puis l'empreinte de l'image, ne propose que les disques amovibles de 12 Go ou plus sans partition système, puis relance le même exécutable en `--worker` par `pkexec` : démontage, ouverture exclusive du disque (`O_EXCL`), décompression xz en Rust pur (blocs décodés en parallèle), écriture, `fsync`, vidage du cache des blocs (`BLKFLSBUF`), relecture complète et comparaison des empreintes, relecture de la table de partitions. L'image publiée doit se nommer `secmet-key_<version>_x86_64.img.xz` (la version en est tirée pour retrouver `SHA256SUMS`), sauf si `SHA256SUMS` et `SHA256SUMS.sig` sont à côté du fichier. `secmet-key-writer --verify <image>` fait la même vérification en ligne de commande. Windows : même exécutable (`secmet-key-writer_<version>_x86_64.exe`, travail `key-writer-windows` de `package.yml` sur un runner Windows, ou `make key-writer-exe` en compilation croisée MinGW, sur la machine ou dans le conteneur Ubuntu 24.04 de l'AppImage), sans console ni bibliothèque à côté (TLS Schannel). Rendu par wgpu sur Direct3D 12 (dorsal `dx12` seul) et non OpenGL : le pilote d'affichage de base de Windows (machines virtuelles, bureau à distance, postes sans pilote graphique) ne fournit qu'OpenGL 1.1, où egui refuse de démarrer, alors que Direct3D 12 a toujours le repli logiciel WARP. Les coches et croix de l'interface sont ✔ et ✖ (U+2714, U+2716), seuls glyphes de ce type dans les polices embarquées d'egui. L'interface se relance en administrateur au lancement (`ShellExecuteEx` avec le verbe `runas`, invite UAC, comme Rufus) et le processus `--worker` en hérite ; disques par `IOCTL_STORAGE_QUERY_PROPERTY` et `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` sur `\\.\PhysicalDriveN` (amovible = bus USB, SD, MMC ou média amovible, pas le bit hotplug que portent les ports SATA internes ; volumes et lettres par `FindFirstVolume` et `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` ; le lecteur `%SystemDrive%` marque le disque système) ; avant l'écriture chaque volume de la clé est verrouillé (`FSCTL_LOCK_VOLUME`, dix essais) et démonté (`FSCTL_DISMOUNT_VOLUME`), poignées gardées ouvertes jusqu'à la fin pour que Windows ne remonte rien ; écriture et relecture sans cache (`FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`, lectures de taille alignée), puis `IOCTL_DISK_UPDATE_PROPERTIES`. **GPT complétée pendant l'écriture (`gpt.rs`, Linux comme Windows) :** l'image fait 11 Gio, sa GPT de secours est donc au bout de l'image et non de la clé ; Linux s'en contente, mais Windows, dès qu'il ré-analyse le disque (fermeture d'une poignée d'écriture, relâchement des verrous, `IOCTL_DISK_UPDATE_PROPERTIES`), « répare » la table à sa façon — en-tête primaire réécrit, entrées déplacées, et la signature `55 AA` du MBR protecteur écrasée par `EFI ` (constaté sur Windows 11 25H2, reproduit en PowerShell : le secteur 0 est intact tant que la GPT n'est pas ré-analysée) : la clé devient illisible pour Linux et non démarrable. Le worker écrit donc une table complète d'emblée : dès le premier bloc décodé, il écrit les entrées et l'en-tête de secours au bout du disque (avant le premier bloc, pour qu'aucune ré-analyse ne trouve jamais une table incomplète), puis le bloc 0 avec l'en-tête primaire corrigé (`AlternateLBA`, `LastUsableLBA`, CRC) et l'entrée protectrice étendue au disque ; la relecture compare ces secteurs à part et l'empreinte affichée reste celle de l'image. Avec une GPT complète Windows ne touche à rien (vérifié en PowerShell puis avec l'outil). `secmet-first-boot` relocalise de toute façon la GPT de secours (`sfdisk --relocate gpt-bak-std`) avant d'agrandir l'export. SmartScreen avertit (éditeur inconnu, pas de certificat de signature de code) : *Informations complémentaires* → *Exécuter quand même*. Le glisser-déposer d'un fichier vers une fenêtre élevée est bloqué par Windows (UIPI) : passer par *Choisir le fichier*. En ligne de commande (`--list-disks`, `--verify`), PowerShell et cmd rendent la main avant la sortie d'un exécutable sans console : `.\secmet-key-writer.exe --list-disks | Out-Host` pour l'attendre.

### Intégration continue

- Les workflows GitHub Actions : `ci.yml` (tests à chaque push sur `main` et à chaque pull request vers `main`), `fuzz.yml`, `package.yml` (construction de toutes les releases, signature avec le secret `SECMET_RELEASE_KEY` et publication sur tag, §N.4). Ils ne sont pas indispensables pour publier : `make release` fait de même en local, à l'écart de l'étape 5 près.
- Aucun abonnement payant n'est prévu : le dépôt du code est public, les Actions sont gratuites sur les machines standard de GitHub.
- Un runner auto-hébergé ou une forge locale (Forgejo, Gitea) restent possibles, mais ajoutent un service à maintenir : non retenus en v1.

## N.8 Confidentialité

La vérification ne transmet que l'adresse IP de l'atelier et la version utilisée (requête HTTPS vers l'hébergement des releases). Aucune donnée de disque, de certificat ou d'opérateur n'est envoyée.

## N.9 Recette

Scénarios repris et complétés dans l'[Annexe C](Annexe_C_plan_de_recette.md), §C.4 (R-41 à R-46), qui fait référence :

| Scénario | Résultat attendu |
|----------|------------------|
| Poste hors ligne | SecMet démarre normalement ; « dernière vérification » inchangée ; aucun message d'erreur |
| Nouvelle version publiée | Point jaune sur Réglages ; version, date et notes affichées ; le lien ouvre la page de la version |
| `latest.json` altéré ou signé par une autre clé | Ignoré : aucune annonce, « Vérification impossible » dans les Réglages |
| Case « Vérifier automatiquement » décochée | Aucune requête réseau ; « Vérifier maintenant » reste possible |
| Poste `.deb` ou `.rpm`, **Installer la mise à jour** | Téléchargement, vérification de la signature et de l'empreinte, mot de passe administrateur, installation ; **Relancer SecMet** démarre la nouvelle version |
| Opération en cours (effacement, analyse, TestDisk, liste des disques, PDF, clôture, déverrouillage, dé-frozen) | Bouton **Installer la mise à jour** absent, raison affichée ; il réapparaît à la fin de l'opération sans rouvrir les Réglages |
| Lot ouvert (poste en mode lot ou clé live) | Bouton absent, « Lot L-… ouvert : clôturez-le… » et bouton **Clôturer le lot…** ; après la clôture, le bouton d'installation réapparaît |
| AppImage sur un poste qui a apt ou dnf | Pas de bouton **Installer la mise à jour** ; note « téléchargez la nouvelle AppImage et remplacez le fichier » et lien de téléchargement |
| Poste Fedora 44, release `fc42` + `fc44` | Le paquet `fc44` est téléchargé et installé |
| Clé live, mise à jour installée | Paquet rangé dans `app/<version>/`, redéployé au démarrage suivant |
| Clé live, nouvelle version défaillante | Retour automatique à la version précédente au démarrage suivant |
| Clé live, version rangée dans `app/`, lot ouvert sous l'ancienne, redémarrage | Version reportée (« version … reportée : lot L-… ouvert sous secmet … » au journal), le lot garde sa version ; après la clôture du lot, déployée au démarrage suivant |

---

## Documents connexes

| Document | Objet |
|----------|-------|
| [Annexe M — Clé live et lots persistants](Annexe_M_cle_live_lots.md) | Structure de la clé, lots |
| [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md) | Installation et mise à jour manuelles actuelles |
| [README](../../README.md) | Commandes Make et intégration continue |
