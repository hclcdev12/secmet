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
   - le bouton **Installer la mise à jour** fonctionne comme sur un poste (installation pour la session, en mémoire) et, sur la clé, range en plus le paquet vérifié avec `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` dans `SECMET-DATA/app/<version>/` ; seules les deux versions les plus récentes sont gardées ;
   - au démarrage, avant la session (`secmet-app-update.service`, après le montage de la partition, avant LightDM), un script parcourt `app/` de la version la plus récente à la plus ancienne : si elle est plus récente que celle de l'image, `secmet-release verify --file` contrôle la signature de `SHA256SUMS` avec la clé de version (codée dans le script, la même que dans l'application) et l'empreinte du paquet, puis `apt-get install` l'installe dans le système en mémoire ; signature invalide ou installation échouée : version suivante, et à défaut celle de l'image ; chaque issue est tracée dans le journal (`secmet-app-update`) ;
   - `secmet-release` est livré dans les paquets `.deb` et `.rpm` à cet effet.
2. **Mises à jour du système** (noyau, bibliothèques, correctifs de sécurité) — en refaisant la clé :
   - pas de mise à jour du système live lui-même ;
   - l'image est reconstruite environ tous les 6 mois ou lors d'une faille importante ;
   - `secmet-make-key` réécrit la partition système seulement ; lot en cours, clé de signature et livrables sont conservés.

## N.7 Chaîne de publication : `make release`

La publication ne dépend pas de GitHub Actions : elle tourne sur le poste du responsable des versions, dans des conteneurs (podman ou docker). Elle reproduit le workflow `package.yml`.

Script : `packaging/release.sh` (`make release`, `make publish`). Il refuse un arbre de travail modifié ou un `HEAD` sans le tag `vX.Y.Z` (options `--allow-dirty`, `--allow-untagged` pour les essais).

1. Contrôles : `cargo fmt --check`, `clippy -D warnings`, tests du workspace, `cargo audit`, shellcheck.
2. Construction en conteneurs : `.deb` Ubuntu 24.04 et 22.04, `.rpm` Fedora 42, AppImage (Ubuntu 24.04).
3. Installation et contrôle de chaque paquet dans son conteneur (binaires présents, helper refusant d'être lancé hors protocole, TestDisk embarqué, policy PolicyKit) ; contrôle de la glibc requise par l'AppImage (≤ 2.39).
4. `secmet-release sign` : `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` signés avec la clé de version, puis `secmet-release verify` avec la clé publique.
5. `--publish` seulement : `gh release create vX.Y.Z` sur le dépôt avec les 7 fichiers, après confirmation explicite (`oui`). Une release déjà existante est refusée.

Sortie : `dist/release/<version>/`, avec les journaux de construction sous `logs/`. Le tag (`git tag -a vX.Y.Z`) et le push du code restent des gestes explicites du responsable des versions.

Outil `secmet-release` (crate `secmet-attest`, non livré dans les paquets) :

| Commande | Rôle |
|----------|------|
| `keygen <clé.hex>` | Crée la clé de version (fichier `0600`, clé publique à côté), affiche `public_key` et `key_id` ; refuse d'écraser |
| `sign <dossier> --key … --version … --date … --sums-url …` | Écrit `SHA256SUMS`, `SHA256SUMS.sig`, `latest.json` |
| `verify <dossier> [--trust …]` | Vérifie les deux signatures, l'empreinte de chaque fichier et la cohérence de la version ; code 0 seulement avec une clé de confiance |

Prérequis GitHub : une release exige un tag, donc le tag `vX.Y.Z` doit exister sur le dépôt ; `package.yml` (GitHub Actions, gratuit sur un dépôt public) construit, signe et publie à chaque push de tag, `make release` restant la voie locale.

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
