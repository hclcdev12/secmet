# Annexe G — Guide utilisateur

> Documentation SecMet — v0.4.0

Ce guide est pensé pour un **atelier** : branchement de disques, effacement, vérification et archivage d'un certificat. SecMet fonctionne **localement sur le banc** : aucun serveur central ni service cloud n'est requis pour traiter les disques ni générer le certificat.

## 1) Une seule action, une méthode décidée par disque

Une seule action : **Lancer la suppression sécurisée**. Pour chaque disque coché, le moteur établit un **plan** à partir de ce que le disque déclare (IDENTIFY DEVICE, Identify Controller, journal sanitize, état ATA Security) et des protections du système :

- **Purge firmware d'abord** : NVMe Sanitize ou Format NVM, ATA SANITIZE, SECURITY ERASE UNIT (HDD et SSD SATA).
- **Replis explicites** si une méthode est écartée (disque gelé, mot de passe tiers, pont USB) : le dialogue de confirmation affiche la méthode retenue, le nombre de replis et, le cas échéant, la dégradation (par exemple « Purge → Clear »).
- **Écriture hôte** en dernier recours : passes pseudo-aléatoires puis zéros. Pour un HDD sans commande firmware, le dialogue propose **1 passe + zéros** (défaut, NIST Clear) ou 3 passes (choix mémorisé).
- **Disques USB** : aucune commande firmware n'est envoyée à travers un pont USB ; écriture hôte, catégorie Clear.

Le certificat indique la **méthode réellement obtenue** et sa catégorie **NIST SP 800-88** (Purge, Clear, ou non classé).

## 2) Prérequis et démarrage

- Linux (Debian/Ubuntu/Mint ou Fedora) et droits administrateur : un mot de passe est demandé **au lancement**. Ou bien une **clé USB SecMet** démarrée sur la machine à traiter : rien à installer (voir §9).
- Disques **internes** (SATA, NVMe) ou **USB**.

### Premier lancement et droits administrateur

Au démarrage, SecMet lance son helper privilégié via PolicyKit (`pkexec`) : la fenêtre s'ouvre immédiatement avec l'indication « Authentification administrateur en cours… » et reste réactive pendant l'invite. Le mot de passe vaut pour la session (`auth_admin_keep`) ; s'il expire, il peut être redemandé au clic sur une action.

**Prérequis PolicyKit** : `pkexec` doit être installé avec le bit setuid et la policy `com.secmet.pkexec.helper` en place. Sinon l'application refuse de démarrer (`make fix-pkexec` puis `make verify-polkit` en environnement de développement, ou réinstallation du paquet sur le banc).

**AppImage** : au premier lancement, un dialogue « Composant système requis » propose d'installer le helper, la policy PolicyKit et TestDisk ; un mot de passe, une seule fois par machine. Voir [Annexe H](Annexe_H_deploiement_banc.md).

### Migration depuis shred-graphique

Si vous aviez l'ancien paquet `shred-graphique`, au **premier lancement** de SecMet, les journaux sous `~/.shred_graphique_logs/` sont déplacés vers `~/.secmet_logs/` (fusion et sauvegarde en `~/.shred_graphique_logs.bak` si les deux dossiers existaient).

## 3) La fenêtre principale

| Zone | Contenu |
|------|---------|
| Barre de titre | « SecMet – Effacement sécurisé », icône **Réglages** (engrenage) |
| Bandeau d'avertissement | Rappel permanent de l'irréversibilité |
| Avis sur l'hôte | Visible seulement si la racine du système n'a pas pu être rattachée à un disque local (média live, ZFS) : le disque système n'est alors protégé que par le verrou exclusif du noyau, vérifiez la sélection |
| Ligne de sélection | **Tout sélectionner**, total de la capacité cochée |
| Cartes disques | Case à cocher, nom, bouton **i** (identité, SMART, capacités), badge d'état (INACTIF, ANALYSE…, EN ATTENTE, EN COURS, ARRÊT…, ARRÊTÉ, TERMINÉ, ÉCHEC, À DÉTRUIRE, INDISPONIBLE, LOCKED), sous-titre (type, capacité, modèle, série, FROZEN, LOCKED, motif d'indisponibilité), barre de progression, bouton Stop, bouton **Marquer à détruire** (disque verrouillé ou en lecture seule) |
| Bandeau de lot | En mode lot : lot en cours (identifiant, opérateur, nombre de machines ou de disques) et bouton **Clôturer** |
| Barre d'actions | **Rafraîchir**, **TestDisk**, **Générer le PDF**, **Relancer les échoués**, **Déverrouiller**, **Dé-frozen**, **Lancer la suppression sécurisée** |

**ANALYSE…** : identification du disque en cours ; **EN ATTENTE** : disque retenu pour l'effacement, en attente de son tour ; **ARRÊT…** : arrêt demandé, en cours ; **LOCKED** (libellé non traduit) : disque verrouillé par un mot de passe ATA.

Les cartes se remplissent en deux temps : inventaire immédiat (sysfs), puis identification firmware disque par disque (modèle, série, SMART, capacités, verdict du moteur).

## 4) Procédure d'effacement

1. Brancher les disques (interne ou USB). Cliquer **Rafraîchir** si un disque vient d'être branché.
2. Cocher un ou plusieurs disques. Un disque monté, membre d'un volume, support du système ou dont l'inspection a échoué reste INDISPONIBLE avec son motif.
3. Si un disque SATA est **FROZEN** : bouton **Dé-frozen** (voir §6). Si un disque est verrouillé par un mot de passe ATA (**LOCKED**) : bouton **Déverrouiller** (voir §7).
4. Cliquer **Lancer la suppression sécurisée** : le dialogue de confirmation récapitule, par disque, la méthode, la catégorie NIST et les replis ; pour un HDD en écriture hôte, choisir le nombre de passes. Confirmer.
5. Suivre la progression : étape du plan, phase (canaris, firmware, relecture), pourcentage, débit et ETA pour l'écriture hôte, pourcentage du disque pour un sanitize, temps restant estimé pour un SECURITY ERASE (durée annoncée par le disque).
6. **Stop** interrompt une écriture hôte entre deux blocs ou un sanitize entre deux sondages. Pendant un Format NVM ou un SECURITY ERASE UNIT, le bouton est grisé : la commande ne peut pas être interrompue (débrancher le disque la fait échouer et laisse un mot de passe que SecMet lèvera au scan suivant).
7. À la fin de chaque disque : **Vérification : OK** (blocs relus, anomalies, signatures résiduelles) ou KO avec le bouton **Détails vérification**. Le moteur conclut sur le **sort du support** : un succès peut porter « destruction physique recommandée » (état du disque), un échec est soit **ÉCHEC** (à relancer, avec la consigne : liaison, firmware, blocage) soit **À DÉTRUIRE** (secteurs illisibles, écritures non persistantes, firmware sans effet, disque déjà condamné par le SMART). **Relancer les échoués** relance uniquement les disques en ÉCHEC, jamais ceux à détruire.
8. Un disque **verrouillé** par un mot de passe ATA inconnu (badge LOCKED) ou **en lecture seule** n'est jamais forcé et ne peut pas être coché. Si le mot de passe du propriétaire est connu, **Déverrouiller** le lève et l'effacement devient possible. Sinon, le bouton **Marquer à détruire** de sa ligne le sort du circuit de réemploi sans rien lancer : le disque est enregistré dans la session et le lot avec le sort « destruction physique requise », le certificat est produit comme pour un échec, et le badge passe à **À DÉTRUIRE**.

## 5) Certificat et archivage

### Journaux automatiques

| Fichier | Contenu |
|---------|---------|
| `~/.secmet_logs/sessions/<SEC-…>/journal.log` | Synthèse de session : horodatage, méthode, verdict par disque |
| `~/.secmet_logs/sessions/<SEC-…>/<disque>.log` | Rapport détaillé des étapes (issue, critère d'échec, validation, seed) et journal TestDisk |
| `~/.secmet_logs/session_report_<date>.json` | Rapport structuré de session (base du certificat) |

En mode lot, les journaux et certificats d'un lot vont dans `lots/<LOT-ID>/` (puis `archive/` à la clôture) ; sur la clé USB SecMet, tout est sur la partition `SECMET-DATA` (§9).

Les journaux et certificats sont écrits en lecture seule pour les autres utilisateurs (0700/0600). Les journaux restent en **français** quelle que soit la langue de l'interface ; les certificats sont rédigés dans la langue de l'interface (voir ci-dessous).

### Générer le certificat PDF

1. Terminer **tous** les effacements de la session (succès ou échec ; un arrêt manuel compte comme **Échec**).
2. Cliquer **Générer le PDF**, saisir le **nom de l'opérateur** (pré-rempli s'il est enregistré dans les Réglages, §8), choisir le format (**une page par disque** ou **certificat groupé**), cocher les disques, valider.
3. Les fichiers sont créés sous `~/.secmet_logs/certificates/` : `<SEC-…>.pdf` et une **attestation JSON signée** par disque (`<SEC-…>-<disque>.json`), ou `…-lot.pdf` / `…-lot.json` pour le format groupé.
4. **Ouvrir le PDF** lance le lecteur par défaut ; **Ouvrir le dossier** le gestionnaire de fichiers.

**Langue du certificat** : le PDF est rédigé dans la langue de l'interface au moment de la génération (français, anglais ou allemand) : libellés, dates, méthode, réserves et statut de vérification. Les textes libres de l'attestation JSON signée (libellé de la méthode, réserves, statut de vérification, mentions « non disponible » et « inconnu ») sont écrits dans la même langue ; les éléments vérifiables (code de méthode, catégorie NIST, empreinte des preuves, seed, signature) ne dépendent pas de la langue. Pour obtenir un certificat dans une autre langue, changer la langue dans les Réglages avant de cliquer **Générer le PDF**.

### Contenu d'une page

Statut de l'opération, date, machine et équipement du banc (fabricant, modèle, numéro de série lus dans le DMI), médium (type, modèle, numéro de série, firmware, capacité, nœud), détails de l'opération (méthode, catégorie NIST, résultat, politique, début, fin, durée), validation par relecture (preuve, blocs relus et anomalies, signatures résiduelles, empreinte des preuves, seed, vérification indépendante), **réserves** (pont USB, dégradation, numéro de série absent, espaces de noms NVMe non couverts, hôte non identifié), identifiant de clé, clé publique, signature Ed25519 et **QR code** contenant l'attestation complète.

Exemples : [certificat d'un disque](../exemple_pdf.pdf) · [certificat de lot](../exemple_lot.pdf).

### Vérifier un certificat (hors ligne)

```bash
secmet-verify SEC-20261009-18PH-sdc.json
secmet-verify --qr "SM1:…"                  # texte lu dans le QR
secmet-verify --trust fc5b6a969d0b1077 …    # clé ou key_id de confiance
```

`secmet-verify` est installé par les paquets `.deb` et `.rpm`. Avec l'AppImage : `./SecMet_<version>_x86_64.AppImage verify SEC-….json` (mêmes options). Sur un poste de vérification sans SecMet, prendre le binaire autonome de la release : `chmod +x secmet-verify_<version>_x86_64`, puis `./secmet-verify_<version>_x86_64 SEC-….json`.

Le programme contrôle la signature et n'accepte que les **clés de confiance** : `--trust`, variable `SECMET_TRUSTED_KEYS`, `/etc/secmet/trusted-keys`, sur la clé USB `SECMET_DATA_DIR/config/attestation-key.pub` et `SECMET_DATA_DIR/config/trusted-keys`, puis `~/.config/secmet/attestation-key.pub` (clé du poste) et `~/.config/secmet/trusted-keys`. Une signature valide avec une clé inconnue est signalée « clé non vérifiée » et le programme rend le code 1 (`--allow-untrusted` pour simplement afficher). Distribuez la clé publique du banc (`~/.config/secmet/attestation-key.pub`, affichée aussi dans les Réglages, section **Clé de signature des certificats**, §8) aux vérificateurs ; pour une clé USB, elle accompagne les livrables de chaque lot sur `SECMET-EXP` (`cle-publique-secmet-<key_id>.pub`). **Inscrire une clé** : ajouter sa ligne à `/etc/secmet/trusted-keys` (une clé publique par ligne, `#` pour un commentaire), par exemple `cat cle-publique-secmet-<key_id>.pub | sudo tee -a /etc/secmet/trusted-keys`. **La retirer** (support perdu, volé ou réécrit avec l'image publiée) : supprimer la ligne ; les certificats qu'elle a signés ne sont plus acceptés, sauf avec `--trust`.

Pour un **lot** clôturé (§8, §9), `secmet-verify --manifest L-…-manifest.json --trust <clé>` vérifie le manifeste signé et contrôle que toutes les attestations listées sont présentes et intactes dans le dossier, et qu'aucune n'a été ajoutée.

## 6) Dé-frozen SATA

Sur certains PC, le BIOS/UEFI « gèle » les commandes ATA Security au démarrage (état **FROZEN**) : le disque refuse SANITIZE et SECURITY ERASE.

1. Cliquer **Dé-frozen** (barre du bas) et vérifier la liste des disques gelés.
2. Cliquer **Mettre en veille** : la machine entre en veille ; la réveiller (bouton d'alimentation). Le helper ré-identifie le disque à la reprise.
3. Cliquer **Rafraîchir** pour confirmer l'absence de FROZEN.

Le dé-frozen est refusé tant qu'un effacement est en cours. Alternatives manuelles : hot-plug SATA, mode AHCI plutôt que RAID/RST.

## 7) Déverrouiller un disque ATA

Un disque qui porte un mot de passe ATA Security (verrouillé ou non) n'est pas effaçable tant que le mot de passe est actif. Si le mot de passe est **connu** (propriétaire, prestataire) :

1. Cliquer **Déverrouiller**, choisir le disque, saisir le mot de passe (utilisateur, ou maître si la case est cochée).
2. Le helper déverrouille puis lève le mot de passe ; la liste se rafraîchit.

Sans le bon mot de passe, le disque refuse et rien n'est modifié ; après cinq refus, le disque exige un cycle d'alimentation. SecMet ne contourne jamais un verrou ATA : sans mot de passe, la destruction physique reste la seule option.

Un mot de passe posé par SecMet lui-même (SECURITY ERASE interrompu) est levé automatiquement au scan suivant.

## 8) Réglages

Icône **engrenage** dans la barre de titre :

- **Langue de l'interface** : français (natif), anglais, allemand ; appliquée immédiatement (fenêtre reconstruite), verrouillée pendant une opération. Les certificats PDF générés ensuite sont rédigés dans la même langue ; les journaux restent en français.
- **Logo des certificats PDF** : image PNG, JPEG, GIF, BMP, WebP ou TIFF, convertie en JPEG sur fond blanc (`~/.config/secmet/pdf-logo.jpg`) et placée en haut à gauche de chaque page à la place du logo SecMet ; **Logo SecMet** rétablit le défaut.
- **Opérateur** : nom pré-rempli dans le dialogue **Générer le PDF** (modifiable à chaque génération) ; vide, il est saisi à chaque fois.
- **Lots** (poste installé) : **Mode lot** regroupe les effacements en lots, comme sur la clé USB : au lancement, SecMet propose d'ouvrir ou de continuer un lot ; les disques effacés au fil des sessions (brancher, effacer, débrancher, recommencer) y sont enregistrés avec leur certificat ; **Clôturer** produit le certificat groupé et le manifeste signé dans l'archive du lot (`~/.secmet_logs/archive`) ; sur un poste installé, rien n'est copié sur une clé SecMet branchée. Un lot sans aucun disque effacé ne se clôture pas : **Abandonner le lot** l'archive tel quel, sans certificat groupé ni manifeste. Un disque qui repasse dans le même lot est signalé (Remplacer / Ajouter quand même). Cocher la case ouvre le dialogue de lot immédiatement.
- **Clé de signature des certificats** : identifiant (`key_id`, imprimé sur chaque certificat) et clé publique du poste, ou de la clé USB, en lecture seule, avec **Copier**. C'est la ligne que le client inscrit dans sa liste de confiance (`trusted-keys`, §6) ; il la retire si le support est perdu. Tant qu'aucun certificat n'a été signé, la section l'indique (rien n'est créé pour l'afficher). La même clé est jointe aux livrables de chaque lot clôturé, dans le fichier `cle-publique-secmet-<key_id>.pub` (sur `SECMET-EXP` depuis la clé, dans l'archive du lot sur un poste).
- **Dossier des certificats** : bouton **Ouvrir le dossier des certificats**, qui ouvre dans le gestionnaire de fichiers le lot en cours (clé USB), sinon les lots clôturés (`archive/`) ou les certificats du poste. Le dialogue **Lot clôturé** propose de même **Ouvrir le dossier** vers la copie remise au client. Les fichiers restent modifiables par cet accès ; toute suppression ou altération d'une attestation se détecte au manifeste signé.
- **Mises à jour** : version installée, date de la dernière vérification et, si le canal de publication annonce une version plus récente, son numéro, sa date et ses notes, avec un bouton **Ouvrir la page de téléchargement**. **Installer la mise à jour** apparaît en plus quand le poste a le gestionnaire de paquets attendu (`apt` pour le `.deb`, `dnf` pour le `.rpm` de la Fedora du poste, y compris sur la clé USB), qu'**aucune opération n'est en cours** et qu'**aucun lot n'est ouvert** : SecMet télécharge le paquet, vérifie la signature de la release et l'empreinte du paquet (rien n'est installé sans empreinte vérifiée), puis l'installe après le mot de passe administrateur ; **Relancer SecMet** démarre ensuite la nouvelle version. Sinon la raison s'affiche à la place du bouton (« effacement en cours », « analyse des disques en cours », « TestDisk en cours », « lot L-… ouvert : clôturez-le… ») et change d'elle-même, sans rouvrir les Réglages ; avec un lot ouvert, **Clôturer le lot…** ouvre le dialogue de clôture, puis la mise à jour s'installe : un lot n'est ainsi traité que par une seule version. Pendant l'installation, les Réglages et la fenêtre principale ne se ferment pas. Lancé depuis une AppImage, SecMet n'affiche pas ce bouton : télécharger la nouvelle AppImage et remplacer le fichier (le composant système est mis à jour au lancement suivant). Sans gestionnaire de paquets reconnu, seule la page de téléchargement est proposée ([Annexe N](Annexe_N_mises_a_jour.md)). **Vérifier maintenant** interroge le canal immédiatement. La case **Vérifier automatiquement les mises à jour** (cochée par défaut) autorise une requête HTTPS par jour, sans donnée personnelle ; décochée, SecMet ne contacte jamais le réseau. Sans réseau, rien ne change et aucune erreur ne bloque. Un **point jaune** sur l'engrenage signale qu'une version plus récente est connue. Seules les annonces signées par la clé de version SecMet sont prises en compte.
- **Sur la clé USB SecMet** seulement : **Recopier les lots clôturés sur la partition d'export** (rattrapage d'une copie manquée) et **Nettoyer la clé…** (voir §9).

Les réglages (langue, logo, opérateur, passes HDD, mode lot, mises à jour) sont conservés dans `~/.config/secmet/settings.json` (sur la clé : `config/settings.json` de la partition `SECMET-DATA`).

## 9) Clé USB SecMet et lots

### Démarrer sur la clé

La clé USB SecMet s'écrit à partir de l'image publiée avec chaque version, avec l'outil **SecMet Clé USB** (Windows ou Linux) : voir [Fabriquer sa clé](../Fabriquer_sa_cle.md). Ensuite :

1. Brancher la clé sur la machine à traiter, démarrer dessus (menu de démarrage du BIOS/UEFI). La session s'ouvre seule et SecMet se lance.
2. **Mot de passe** : l'invite d'administration demande le mot de passe du compte `secmet`. Sur une clé écrite depuis l'image publiée, c'est **`secmet`**, le même sur toutes les clés : il n'empêche pas un tiers qui a la clé en main de lancer un effacement. Le changer dans la session (`passwd`) ne dure que jusqu'à l'extinction ; pour un mot de passe propre à l'atelier, faire fabriquer la clé avec un mot de passe choisi ([Fabriquer sa clé](../Fabriquer_sa_cle.md)). Gardez la clé en lieu sûr.
3. La clé a deux partitions de travail : `SECMET-DATA` (lots, certificats, clé de signature propre à cette clé, réglages) et `SECMET-EXP` (livrables remis au client, lisible sur tout PC). Si Windows propose de **formater** une partition de la clé, **refuser** toujours.
4. Au premier démarrage, la clé crée sa clé de signature : sa clé publique est à remettre une fois aux vérificateurs (§5).

Sur la clé, les machines traitées successivement sont regroupées en **lots** :

1. **Au démarrage**, SecMet propose d'**ouvrir un lot** (opérateur pré-rempli, référence client facultative) ou de continuer **sans lot**. Si un lot est déjà ouvert sur la clé, il propose **Continuer**, **Clôturer** ou (après clôture) **Nouveau lot**, et signale les machines **interrompues** par une coupure.
2. Un **bandeau** rappelle le lot en cours (identifiant, opérateur, nombre de machines) avec un bouton **Clôturer** ; il remplace l'avis « disque système non identifié », attendu sur une clé live.
3. **Au lancement d'un effacement**, si la machine figure déjà dans le lot (même numéro de série, ou mêmes disques), SecMet demande **Remplacer**, **Ajouter quand même** ou **Annuler**. L'entrée est écrite sur la clé avant le premier bloc effacé.
4. **En fin d'effacement**, les certificats de la machine (une page par disque, au nom de l'opérateur du lot) sont **générés automatiquement** dans le lot ; le message « Machine enregistrée au lot … » le confirme. Le bouton **Générer le PDF** n'a alors plus d'objet : il rappelle que les certificats sont déjà au lot et propose **Ouvrir le dossier** (les régénérer écraserait les fichiers enregistrés, et la clôture le refuserait). On peut alors éteindre la machine et passer à la suivante.
5. **Horloge** : si la date du poste est antérieure à une référence connue (pile du BIOS épuisée), SecMet le signale au démarrage ; **Garder l'horloge du poste** laisse la date telle quelle, **Utiliser cette date** applique la date saisie à SecMet seulement. Dans les deux cas, et dès que l'horloge n'est pas synchronisée par le réseau, le certificat porte une réserve sur la date.
6. **Clôturer** produit le **certificat groupé** (avec « Machines traitées : N » et la série de chaque machine sous son disque), le **manifeste signé** (liste des certificats et empreintes, vérifiable), copie le tout sur la partition d'export `SECMET-EXP` si elle est présente, puis archive le lot. Plus aucune machine ne peut y être ajoutée. Si la copie échoue (partition absente ou pleine), la clôture aboutit quand même et le compte rendu propose **Réessayer la copie sur la partition d'export** ; plus tard, les Réglages proposent **Recopier les lots clôturés sur la partition d'export**.
7. **Abandonner le lot** : tant qu'aucun disque n'a été effacé dans le lot, le bouton **Clôturer** propose d'abandonner le lot ; il est archivé avec son journal, sans certificat groupé ni manifeste, et rien n'est copié sur l'export. Dès qu'un certificat est enregistré, seule la clôture reste possible.

**Sans lot** sur la clé, le certificat produit par **Générer le PDF** est copié aussitôt sur `SECMET-EXP`, dans un dossier au nom de la session.

Les lots vivent sous `lots/` puis `archive/` sur la clé ; le journal `journal.ndjson` de chaque lot est chaîné : une ligne supprimée ou modifiée se détecte.

### Nettoyer la clé

Réglages > **Nettoyer la clé…** libère de la place : trois groupes à cocher (lots archivés, livrables de la partition d'export, certificats hors lot avec leurs journaux), avec le nombre de fichiers et la taille, puis confirmation. Le nettoyage est refusé tant qu'un lot est ouvert, et le groupe « lots archivés » tant qu'un lot clôturé n'a pas sa copie sur l'export (l'archive serait alors la seule copie signée). Les réglages, la clé de signature et les mises à jour téléchargées ne sont jamais touchés.

### Mises à jour sur la clé

**Installer la mise à jour** (Réglages, §8) fonctionne aussi sur la clé : la version installée est conservée sur `SECMET-DATA` et réinstallée à chaque démarrage, rien à réécrire. Le système de la clé lui-même (noyau, bureau) se met à jour en réécrivant la clé : voir [Fabriquer sa clé](../Fabriquer_sa_cle.md) pour garder lots et clé de signature.

## 10) Validation TestDisk (optionnelle)

Après une session avec au moins un succès, **TestDisk** analyse en lecture seule les disques cochés et signale d'éventuelles structures récupérables ; le résultat reste dans le dialogue et le journal, sans modifier le certificat. Voir [Annexe L](Annexe_L_validation_testdisk.md).

## 11) Badges et bouton **i**

- **i vert** : SMART bon ; **w orange** : à surveiller ; **! rouge** : SMART dégradé (secteurs réalloués, panne prédite) ; la confirmation l'annonce et le certificat porte alors « destruction physique recommandée » après l'effacement. **? gris** : inspection firmware impossible (le disque reste effaçable par écriture hôte si rien d'autre ne l'interdit).
- Le bouton **i** ouvre l'identité, les capacités (sanitize, Security, Format NVM), l'état ATA Security et la table SMART (attributs ATA avec seuils, page Health NVMe).

## Accessibilité visuelle (atelier)

Styles centralisés dans [`resources/secmet.css`](../../resources/secmet.css) (tailles explicites, contrastes renforcés). Checklist après modification de l'interface :

- [ ] Sous-titres disques lisibles à ~60 cm (type, modèle, motif INDISPONIBLE)
- [ ] Badges SMART et statut identifiables sans zoom
- [ ] Bandeau **ATTENTION** et avis sur l'hôte lisibles en un coup d'œil
- [ ] Cases à cocher faciles à viser à la souris
- [ ] Thème sombre : pas de texte « fantôme » sur les cartes
- [ ] Résolution 1366×768 : pas de troncature critique des libellés, dans les trois langues

## Problèmes courants

- **Refus « pkexec sans bit setuid » au démarrage** : pas de repli sudo ; `make fix-pkexec` ou contacter l'administrateur.
- **Disque INDISPONIBLE** : lire le motif dans le sous-titre (monté, swap, membre d'un volume, disque système, lecture seule). Démonter ou détacher, puis **Rafraîchir**.
- **Disque INDISPONIBLE pour blocage** (motif dans le sous-titre) : verrouillé, badge LOCKED (Déverrouiller), compteur de tentatives expiré (cycle d'alimentation), sanitize en cours ou en échec (attendre ou CLEAR via le helper au prochain scan).
- **FROZEN** : Dé-frozen puis Rafraîchir.
- **Stop grisé** : commande firmware bloquante en cours (Format NVM, SECURITY ERASE UNIT) ; attendre la fin estimée.
- **Vérification KO** : le firmware a acquitté sans effacer ou des signatures subsistent ; le plan passe au repli suivant automatiquement ; sinon **Relancer les échoués**.
- **Badge À DÉTRUIRE** : le moteur a conclu qu'aucune voie logicielle ne reste (le dialogue d'échec donne la raison), ou l'opérateur a utilisé **Marquer à détruire** sur un disque verrouillé ou en lecture seule. Ne pas relancer : remettre le disque à la filière de destruction physique ; à la clôture du lot, le PDF contient un **bordereau de destruction** avec les numéros de série concernés.
- **PDF s'ouvre dans un éditeur** : installer un lecteur PDF (`evince`, `okular`, `atril`) et utiliser **Ouvrir le PDF** ; les `.json` sont les attestations signées, pas le certificat visuel.
- **`secmet-verify` répond « clé non vérifiée »** : ajouter la clé publique du banc avec `--trust` ou dans `/etc/secmet/trusted-keys`.
- **Pas de bouton « Installer la mise à jour »** : la raison s'affiche à sa place quand une opération est en cours (attendre la fin : le bouton réapparaît seul) ou qu'un lot est ouvert (le clôturer, bouton **Clôturer le lot…**) ; sinon aucune version plus récente n'est annoncée, SecMet tourne depuis une AppImage (télécharger la nouvelle AppImage et remplacer le fichier) ou le poste n'a ni `apt` ni `dnf` (utiliser la page de téléchargement).
- **Diagnostic sur la clé** : le journal système de chaque démarrage est recopié dans `logs/system/` de la partition `SECMET-DATA` ; le joindre à une demande d'assistance.
