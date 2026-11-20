# Fabriquer sa clé USB SecMet

Chaque version publiée de SecMet contient une **image de clé USB prête à écrire** : `secmet-key_<version>_x86_64.img.xz`, sur la page [Releases](https://github.com/hclcdev12/secmet/releases). Il suffit de l'écrire sur une clé avec **SecMet Clé USB**, le petit outil publié avec chaque version, ou avec un outil d'écriture d'images gratuit ; aucune ligne de commande, pas besoin de Linux. La clé démarre ensuite n'importe quel PC ou portable (UEFI, Secure Boot actif ou non, et anciens BIOS) sur SecMet, avec ses lots, ses certificats et sa clé de signature conservés sur la clé ([Annexe M](Annexes/Annexe_M_cle_live_lots.md)).

Le système de la clé, **SecMet Live**, est basé sur Debian 13 (trixie) depuis la version 0.5.0. *Debian est une marque déposée de Software in the Public Interest, Inc. ; SecMet n'est ni affilié au projet Debian, ni approuvé par lui.*

## Il vous faut

- Une clé USB de **16 Go ou plus** (12 Go au minimum). USB 3 conseillé : l'écriture et les effacements sont plus rapides. **Tout ce qu'elle contient sera effacé.**
- **SecMet Clé USB** (sur la page Releases : `secmet-key-writer_<version>_x86_64.exe` pour Windows, `secmet-key-writer_<version>_x86_64.AppImage` pour Linux), qui télécharge l'image, vérifie sa signature, l'écrit et la relit. Sinon, le fichier `secmet-key_<version>_x86_64.img.xz` de la release (environ 850 Mo) et un outil d'écriture d'images :
  - **Raspberry Pi Imager** (Windows, macOS, Linux) : <https://www.raspberrypi.com/software/> ;
  - ou **Rufus** (Windows) : <https://rufus.ie> ;
  - ou `dd` sous Linux.

  balenaEtcher n'est pas conseillé : ses versions récentes échouent sur les images `.img.xz` (« Something went wrong. If it is a compressed image… », problème connu et non corrigé chez Etcher). Si vous n'avez que lui, décompressez d'abord l'image (7-Zip sous Windows, `xz -dk` sous Linux ou macOS : 11 Go sur le disque) et donnez-lui le `.img`.

## Vérifier le téléchargement (facultatif, recommandé)

Le fichier `SHA256SUMS` de la release, signé par la clé de version de SecMet (`SHA256SUMS.sig`), porte l'empreinte de l'image. Sous Linux, avec le `secmet-release` d'un paquet SecMet installé ou du livrable `secmet-live_<version>_x86_64.tar` :

```bash
secmet-release verify <dossier des fichiers téléchargés> --trust 7854d9fe5a5e2be6 --file secmet-key_<version>_x86_64.img.xz
```

`7854d9fe5a5e2be6` est l'identifiant (`key_id`) de la clé de version de SecMet ; sa clé publique Ed25519 complète est `bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5` ([README](../README.md)). SecMet Clé USB fait cette vérification tout seul.

Sous Windows, `certutil -hashfile secmet-key_<version>_x86_64.img.xz SHA256` affiche l'empreinte à comparer à la ligne correspondante de `SHA256SUMS`.

## Écrire l'image

**SecMet Clé USB** : sous Windows, lancer le `.exe` (SmartScreen affiche « Windows a protégé votre ordinateur » parce que l'outil n'est pas signé par un éditeur enregistré : *Informations complémentaires* → *Exécuter quand même* ; puis accepter l'invite d'administrateur, nécessaire pour écrire sur une clé) ; sous Linux, rendre l'AppImage exécutable (clic droit → Propriétés → « Autoriser l'exécution », ou `chmod +x`) et la lancer. Puis suivre les écrans :

1. **Image de la clé SecMet** : *Télécharger dans …* (le dossier des téléchargements), ou *Choisir le fichier…* pour une image déjà téléchargée. Une image de la version courante déjà présente dans le dossier des téléchargements est prise d'office (*Utiliser …*, *Télécharger à nouveau* pour la remplacer).
2. **Vérification de l'image** : la signature de la release et l'empreinte de l'image sont vérifiées avec la clé de version de SecMet ; rien ne s'écrit si elles ne passent pas. Après un téléchargement, la vérification puis le choix de la clé s'enchaînent sans clic ; *Autre image* ramène au premier écran.
3. **Choisir la clé USB** : seuls les disques amovibles de 12 Go ou plus y figurent, jamais les disques internes ; *Continuer*.
4. **Confirmer l'effacement** : cocher « J'ai vérifié qu'il s'agit de la bonne clé » puis *Effacer et écrire la clé*. Sous Linux, le système demande votre mot de passe pour autoriser l'écriture.
5. **Écriture de la clé** : l'outil écrit l'image en complétant au passage la table de partitions au bout de la clé (dès le premier bloc, sans quoi Windows la « réparerait » et rendrait la clé inutilisable), puis relit toute la clé et compare. Ne pas débrancher la clé.
6. **Clé prête** (avec une notification, ou un son sous Windows) : *Écrire une autre clé* ou fermer. En cas d'échec, l'écran **Clé non écrite** donne le message et le journal, avec *Réessayer sur la même clé*, *Choisir une autre clé* ou *Quitter*.

L'outil peut aussi être ouvert directement sur un fichier `.img.xz` (glisser le fichier sur l'icône, ou en argument ; sous Windows, le glisser-déposer vers la fenêtre déjà ouverte est bloqué par le système : passer par *Choisir le fichier…*). En ligne de commande : `--version`, `--list-disks` (disques proposés) et `--verify <image>` (vérification seule, sans écrire). Sous Windows, si l'outil signale un volume « utilisé par un autre programme », fermer les fenêtres de l'Explorateur ouvertes sur la clé et réessayer.

**Raspberry Pi Imager** : *Choisir le modèle* → n'importe lequel (sans effet sur une image personnalisée) ; *Choisir l'OS* → tout en bas, *Utiliser une image personnalisée* → le fichier `.img.xz` tel quel (pas besoin de le décompresser) ; *Choisir le stockage* → la clé USB ; *Suivant*, puis répondre **Non** à la personnalisation de l'OS si elle est proposée. Imager écrit puis relit la clé pour vérifier. Compter 11 Go écrits puis relus, quel que soit l'outil : de 5 à 25 minutes selon la clé (13 et 23 minutes mesurées sur deux clés USB 3 de 16 Go courantes), davantage sur une clé USB 2.

**Rufus** : choisir la clé dans *Périphérique*, *SÉLECTION* → le fichier `.img.xz`, laisser les autres réglages, *DÉMARRER*. Si Rufus propose un choix entre « image ISO » et « image DD », prendre **DD**.

**Linux** (remplacer `/dev/sdX` par la clé, vérifiée avec `lsblk` ; une erreur de lettre détruit un autre disque) :

```bash
xz -dc secmet-key_<version>_x86_64.img.xz | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync
```

**Sous Windows, après l'écriture** : depuis la 0.5.0, l'explorateur ne montre qu'un lecteur, `SECMET-EXP` (les livrables). Le système (`SECMET-SYS`) et les données (`SECMET-DATA`) sont cachés à Windows par des attributs de la table de partitions. Si Windows affiche malgré tout « Vous devez formater le disque du lecteur … avant de pouvoir l'utiliser » (image plus ancienne écrite avec Rufus, Imager ou `dd`), répondre **Annuler** à chaque fois : c'est la partition de données Linux de la clé, que Windows ne sait pas lire ; la formater détruirait la clé. Ne rien modifier ni ajouter sur `SECMET-SYS` s'il apparaît : c'est le système de la clé.

Retirer la clé proprement (éjection) avant de la débrancher.

## Premier démarrage

Brancher la clé sur le PC à traiter, l'allumer et choisir la clé dans le menu de démarrage (touche F12, F9, Échap ou F2 selon la marque, à l'allumage). Le bureau s'ouvre sans écran de connexion et SecMet se lance aussitôt ([Annexe G §9](Annexes/Annexe_G_guide_utilisateur.md)).

**Mot de passe.** Au lancement de SecMet, une invite d'administration demande le mot de passe du compte `secmet` : il protège les opérations sur les disques. Sur la clé écrite depuis l'image publiée, ce mot de passe est **`secmet`**. Il est le même sur toutes les clés publiées et figure dans cette documentation publique : il n'empêche donc pas un tiers qui a la clé en main de lancer un effacement. Le changer dans la session (`passwd`) ne dure que jusqu'à l'extinction, le système de la clé repartant de l'image à chaque démarrage. Pour un mot de passe propre à l'atelier, fabriquer la clé depuis le dépôt avec `SECMET_OPERATOR_PASSWORD` ([packaging/live/README.md](../packaging/live/README.md)) ; le choix du mot de passe au premier démarrage est à l'étude. Montages, réseau et extinction ne demandent rien.

Ce premier démarrage fait deux choses de plus, une seule fois, en quelques secondes :

1. **La partition d'export `SECMET-EXP` est agrandie** jusqu'au bout de la clé (l'image n'en contient qu'1 Go). C'est elle qui reçoit les livrables des lots clôturés, lisibles sous Windows.
2. **La clé de signature des certificats est créée** sur la clé. Chaque clé USB a la sienne, propre et unique : l'image publiée n'en contient aucune. Son identifiant (`key_id`, 16 caractères) figure sur chaque certificat PDF produit (« Clé de signature … identifiant … ») et dans le journal `logs/system/` de la partition de données ; sa clé publique est dans `config/attestation-key.pub`. C'est cet identifiant que le client enregistre comme clé de confiance pour vérifier les certificats ([Annexe M §M.6](Annexes/Annexe_M_cle_live_lots.md)).

Les démarrages suivants sont ordinaires : la clé garde ses lots, ses certificats et sa clé de signature d'un poste à l'autre.

**À chaque démarrage, la clé contrôle son propre système** (depuis la 0.5.0), en quelques secondes, en tâche de fond : les fichiers de démarrage et le système doivent être ceux de l'image publiée, dont les empreintes sont signées par la clé de version. S'ils ont été modifiés (par exemple par un programme sous Windows) ou se sont corrompus, un bandeau rouge l'indique dans SecMet et chaque certificat porte une **réserve** ; l'effacement reste possible. Réécrire alors la clé avec une image vérifiée. Une clé écrite avec une image d'avant la 0.5.0 n'a pas ces empreintes : ses certificats portent la réserve « intégrité … non vérifiée ». Ce contrôle détecte la corruption et la modification maladroite, pas un attaquant qui modifierait aussi le contrôle lui-même.

## Mettre à jour

- **SecMet** : quand une version plus récente est publiée, un point jaune apparaît sur l'icône des Réglages ; bouton **Installer la mise à jour** (téléchargement, vérification de la signature, installation après le mot de passe du compte `secmet`, puis **Relancer SecMet**). La version est conservée sur la clé et réinstallée à chaque démarrage. Rien à réécrire. Le bouton n'est pas proposé pendant une opération ni tant qu'un lot est ouvert : la raison s'affiche, avec un bouton **Clôturer le lot…** ; une fois le lot clôturé, la mise à jour s'installe. Un lot n'est ainsi traité que par une seule version.
- **Le système de la clé** (noyau, bureau) : réécrire l'image d'une version plus récente **efface tout**, lots et clé de signature compris. Avant : clôturer et copier les lots sur la partition d'export (Réglages), les mettre à l'abri, et, pour garder la même identité de signature, sauvegarder `config/attestation-key.hex` et `config/attestation-key.pub` de la partition de données (lisible sous Linux) ; les remettre au même endroit sur la nouvelle clé avant son premier démarrage. Sous Linux, le livrable `secmet-live_<version>_x86_64.tar` permet à la place de ne réécrire que la partition système, données conservées : `sudo secmet-live-<version>/packaging/live/secmet-make-key.sh /dev/sdX --system-only` ([packaging/live/README.md](../packaging/live/README.md)).

## Menu de démarrage

Le menu de la clé est caché : maintenir **Échap** au démarrage pour l'afficher. Il propose, en plus de l'entrée normale :

- **copie en mémoire** : le système est copié en mémoire au démarrage (25 à 40 s de plus, 4 Go de RAM) ; la clé peut ensuite être retirée ;
- **mode sans échec graphique** : pour une carte graphique récalcitrante ;
- **console de dépannage** : sans bureau, pour un technicien.

## En cas de problème

| Symptôme | Cause probable | Que faire |
|----------|----------------|-----------|
| Le PC ignore la clé | Démarrage USB désactivé, BIOS protégé | Activer le démarrage USB ; sinon, démonter le disque et le traiter au banc ([Annexe M §M.9](Annexes/Annexe_M_cle_live_lots.md)) |
| « Secure Boot violation » | Clé mal écrite (fichier tronqué) | Vérifier l'empreinte, réécrire ; la chaîne de démarrage (celle de Debian 13) est signée par Microsoft avec les autorités de 2011 et de 2023 : elle passe le Secure Boot d'origine des anciens PC comme des nouveaux |
| « Secure Boot violation » sur un PC « Secured-core » récent | Le PC refuse tout chargeur qui n'est pas de Microsoft | Dans le BIOS, activer l'option « Autoriser l'autorité de certification tierce Microsoft UEFI » (« Allow Microsoft 3rd party UEFI CA ») |
| Écran noir après le logo pendant plus d'une minute | Carte graphique récalcitrante | Redémarrer, maintenir Échap au démarrage, choisir « mode sans échec graphique » |
| Windows propose de formater | Partition de données Linux (image d'avant la 0.5.0) | **Annuler** |
| Bandeau rouge « Le système de cette clé USB ne correspond pas à l'image publiée » | Fichier de démarrage modifié ou corrompu sur `SECMET-SYS` (le détail est dans `logs/system/image-check.json`) | Clôturer le lot, réécrire la clé avec une image vérifiée (SecMet Clé USB) ; les certificats déjà produits portent la réserve |
| La partition d'export reste à 1 Go | Elle n'était pas vide au premier démarrage, ou la clé n'a presque pas de place au-delà de l'image (moins de 64 Mio) | Vider `SECMET-EXP` et redémarrer la clé ; le journal `logs/system/` dit pourquoi (`secmet-first-boot`) |
