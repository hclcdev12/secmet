# Fabriquer sa clé USB SecMet

Chaque version publiée de SecMet contient une **image de clé USB prête à écrire** : `secmet-key_<version>_x86_64.img.xz`, sur la page [Releases](https://github.com/hclcdev12/secmet/releases). Il suffit de l'écrire sur une clé avec un outil gratuit ; aucun outil SecMet, aucune ligne de commande, pas besoin de Linux. La clé démarre ensuite n'importe quel PC ou portable (UEFI, Secure Boot actif ou non, et anciens BIOS) sur SecMet, avec ses lots, ses certificats et sa clé de signature conservés sur la clé ([Annexe M](Annexes/Annexe_M_cle_live_lots.md)).

## Il vous faut

- Une clé USB de **16 Go ou plus** (12 Go au minimum). USB 3 conseillé : l'écriture et les effacements sont plus rapides. **Tout ce qu'elle contient sera effacé.**
- Le fichier `secmet-key_<version>_x86_64.img.xz` de la release (environ 850 Mo).
- Un outil d'écriture d'images :
  - **Raspberry Pi Imager** (Windows, macOS, Linux) : <https://www.raspberrypi.com/software/> ;
  - ou **Rufus** (Windows) : <https://rufus.ie> ;
  - ou `dd` sous Linux.

  balenaEtcher n'est pas conseillé : ses versions récentes échouent sur les images `.img.xz` (« Something went wrong. If it is a compressed image… », problème connu et non corrigé chez Etcher, constaté avec l'image SecMet 0.3.12). Si vous n'avez que lui, décompressez d'abord l'image (7-Zip sous Windows, `xz -dk` sous Linux ou macOS : 11 Go sur le disque) et donnez-lui le `.img`.

## Vérifier le téléchargement (facultatif, recommandé)

Le fichier `SHA256SUMS` de la release, signé par la clé de version de SecMet (`SHA256SUMS.sig`), porte l'empreinte de l'image. Sous Linux, avec le `secmet-release` d'un paquet SecMet installé ou du livrable `secmet-live_<version>_x86_64.tar` :

```bash
secmet-release verify <dossier des fichiers téléchargés> --trust <clé publique de version> --file secmet-key_<version>_x86_64.img.xz
```

Sous Windows, `certutil -hashfile secmet-key_<version>_x86_64.img.xz SHA256` affiche l'empreinte à comparer à la ligne correspondante de `SHA256SUMS`.

## Écrire l'image

**Raspberry Pi Imager** : *Choisir le modèle* → n'importe lequel (sans effet sur une image personnalisée) ; *Choisir l'OS* → tout en bas, *Utiliser une image personnalisée* → le fichier `.img.xz` tel quel (pas besoin de le décompresser) ; *Choisir le stockage* → la clé USB ; *Suivant*, puis répondre **Non** à la personnalisation de l'OS si elle est proposée. Imager écrit puis relit la clé pour vérifier. Compter 11 Go écrits : 3 à 5 minutes sur une clé USB 3, 15 à 20 minutes sur une clé USB 2.

**Rufus** : choisir la clé dans *Périphérique*, *SÉLECTION* → le fichier `.img.xz`, laisser les autres réglages, *DÉMARRER*. Si Rufus propose un choix entre « image ISO » et « image DD », prendre **DD**.

**Linux** (remplacer `/dev/sdX` par la clé, vérifiée avec `lsblk` ; une erreur de lettre détruit un autre disque) :

```bash
xz -dc secmet-key_<version>_x86_64.img.xz | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync
```

**Sous Windows, après l'écriture** : Windows affiche une ou plusieurs fois « Vous devez formater le disque du lecteur … avant de pouvoir l'utiliser ». Répondre **Annuler** à chaque fois. C'est la partition de données Linux de la clé, que Windows ne sait pas lire ; la formater détruirait la clé. Il est normal que l'explorateur ne montre que deux lecteurs (`SECMET-SYS` et `SECMET-EXP`).

Retirer la clé proprement (éjection) avant de la débrancher.

## Premier démarrage

Brancher la clé sur le PC à traiter, l'allumer et choisir la clé dans le menu de démarrage (touche F12, F9, Échap ou F2 selon la marque, à l'allumage). SecMet s'ouvre directement sur le bureau, sans mot de passe ([Annexe G §9](Annexes/Annexe_G_guide_utilisateur.md)). Ce premier démarrage fait deux choses de plus, une seule fois, en quelques secondes :

1. **La partition d'export `SECMET-EXP` est agrandie** jusqu'au bout de la clé (l'image n'en contient qu'1 Go). C'est elle qui reçoit les livrables des lots clôturés, lisibles sous Windows.
2. **La clé de signature des certificats est créée** sur la clé. Chaque clé USB a la sienne, propre et unique : l'image publiée n'en contient aucune. Son identifiant (`key_id`, 16 caractères) figure sur chaque certificat PDF produit (« Clé de signature … identifiant … ») et dans le journal `logs/system/` de la partition de données ; sa clé publique est dans `config/attestation-key.pub`. C'est cet identifiant que le client enregistre comme clé de confiance pour vérifier les certificats ([Annexe M §M.6](Annexes/Annexe_M_cle_live_lots.md)).

Les démarrages suivants sont ordinaires : la clé garde ses lots, ses certificats et sa clé de signature d'un poste à l'autre.

## Mettre à jour

- **SecMet** : depuis les Réglages de la clé, bouton de mise à jour (téléchargement, vérification de la signature, installation ; la version est conservée sur la clé et réinstallée à chaque démarrage). Rien à réécrire.
- **Le système de la clé** (noyau, bureau) : réécrire l'image d'une version plus récente **efface tout**, lots et clé de signature compris. Avant : clôturer et copier les lots sur la partition d'export (Réglages), les mettre à l'abri, et, pour garder la même identité de signature, sauvegarder `config/attestation-key.hex` et `config/attestation-key.pub` de la partition de données (lisible sous Linux) ; les remettre au même endroit sur la nouvelle clé avant son premier démarrage. Sous Linux, le livrable `secmet-live_<version>_x86_64.tar` permet à la place de ne réécrire que la partition système, données conservées : `sudo secmet-live-<version>/packaging/live/secmet-make-key.sh /dev/sdX --system-only` ([packaging/live/README.md](../packaging/live/README.md)).

## En cas de problème

| Symptôme | Cause probable | Que faire |
|----------|----------------|-----------|
| Le PC ignore la clé | Démarrage USB désactivé, BIOS protégé | Activer le démarrage USB ; sinon, démonter le disque et le traiter au banc ([Annexe M §M.9](Annexes/Annexe_M_cle_live_lots.md)) |
| « Secure Boot violation » | Clé mal écrite (fichier tronqué) | Vérifier l'empreinte, réécrire ; la chaîne de démarrage est signée par Canonical et passe le Secure Boot d'origine |
| Écran noir après le logo pendant plus d'une minute | Carte graphique récalcitrante | Redémarrer, maintenir Échap au démarrage, choisir « mode sans échec graphique » |
| Windows propose de formater | Partition de données Linux | **Annuler** |
| La partition d'export reste à 1 Go | Elle n'était pas vide au premier démarrage, ou la clé fait moins de 12 Go | Vider `SECMET-EXP` et redémarrer la clé ; le journal `logs/system/` dit pourquoi (`secmet-first-boot`) |
