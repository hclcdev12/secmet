# Annexe D — Glossaire

> Documentation SecMet — v0.4.0

| Terme | Définition |
|-------|------------|
| **Banc d'effacement** | Poste Linux dédié au traitement des disques ; peut être une machine fixe, un poste démarré sur la clé USB SecMet ou sur un autre environnement live, ou un poste démarré en PXE |
| **Certificat** | Attestation d'effacement au format PDF, preuve documentée de l'opération (qui, quoi, comment, résultat) ; ne constitue pas une certification normative officielle |
| **Certificat groupé (lot)** | Format de certificat PDF unique regroupant plusieurs disques dans un tableau de lot, avec un JSON miroir unique (`…-lot.json`) |
| **JSON miroir** | Fichier de métadonnées techniques écrit à côté du PDF (par disque ou par lot) ; sert de base au calcul de l'empreinte SHA-256 |
| **État du disque** | Situation d'un disque à un instant donné, affichée par le badge de sa carte : disponibilité (`INACTIF` / `INDISPONIBLE`), verrou ATA (`LOCKED`), attente et analyse (`EN ATTENTE`, `ANALYSE…`), progression (`EN COURS`, `ARRÊT…`) et résultat (`TERMINÉ`, `ARRÊTÉ`, `ÉCHEC`, `À DÉTRUIRE`) ; la santé SMART et le gel ATA (mention `FROZEN`) figurent sur la carte, hors badge |
| **Effacement sécurisé** | Destruction intentionnelle et irréversible des données selon une méthode adaptée au support |
| **Frozen** | État de sécurité ATA (« gelé ») empêchant certaines commandes d'effacement sur SSD SATA ; levé le plus souvent par une mise en veille (bouton **Dé-frozen**) |
| **Session** | Période d'utilisation comprise entre le lancement de l'application et la génération du certificat ; en mode lot, une session correspond à une entrée du lot (une machine sur la clé, un jeu de disques sur un poste) |
| **Best-effort** | Tentative d'effacement sans garantie totale (cas typique : SSD USB) |
| **Vérification par relecture** | Contrôle post-effacement exécuté par le helper : relecture d'un échantillon de blocs (canaris posés avant l'effacement, motif attendu de la passe) et recherche de signatures résiduelles (table de partitions, systèmes de fichiers) ; verdict affiché à l'écran (OK / KO / erreur) |
| **Canari** | Motif unique par bloc (ChaCha20, seed de session) écrit sur un échantillon avant une commande firmware ; aucun ne doit subsister après, sinon le firmware a acquitté sans effacer |
| **Attestation signée** | Données du certificat sous forme JSON canonique signée Ed25519 par la clé du banc ; vérifiable hors ligne (`secmet-verify`, QR code) |
| **Helper** | Daemon privilégié `secmet-helper` lancé par PolicyKit ; seul composant qui ouvre les disques et envoie les commandes firmware |
| **Validation TestDisk** | Contrôle complémentaire optionnel post-effacement via TestDisk (lecture seule) ; détecte d'éventuelles structures récupérables ; distinct de la vérification par relecture ; voir [Annexe L](Annexe_L_validation_testdisk.md) |
| **Sort du support** | Conclusion du moteur sur le devenir d'un disque après traitement : réemploi, destruction physique recommandée (effacement réussi sur un disque dégradé), à relancer (`ÉCHEC`) ou destruction physique requise (`À DÉTRUIRE`) ; il figure sur le certificat |
| **Marquer à détruire** | Action qui sort du circuit de réemploi un disque que SecMet ne peut pas effacer (verrouillé par un mot de passe ATA inconnu, en lecture seule) : il est consigné avec le sort « destruction physique requise » sans qu'aucune commande ne soit lancée |
| **Bordereau de destruction** | Partie du certificat groupé d'un lot clôturé qui liste, par numéro de série, les disques à remettre à une filière de destruction physique |
| **Clé USB SecMet** | Clé démarrable (Ubuntu 24.04, bureau simplifié, SecMet lancé au démarrage) qui sert de banc sur le poste à traiter ; quatre partitions : système (`SECMET-SYS`), données de travail (`SECMET-DATA`), export (`SECMET-EXP`) et une petite partition de démarrage BIOS ; voir [Annexe M](Annexe_M_cle_live_lots.md) |
| **`SECMET-DATA`** | Partition ext4 de la clé qui conserve d'un démarrage à l'autre les lots, journaux, certificats, réglages, la clé de signature de la clé USB et les mises à jour téléchargées (`app/`) |
| **`SECMET-EXP`** | Partition exFAT d'export de la clé (nom GPT `SECMET-EXPORT`), lisible sous Windows et macOS, où sont copiés les livrables des lots clôturés et les certificats produits sans lot |
| **Mode clé / mode poste** | Un seul logiciel, deux comportements : avec une partition `SECMET-DATA` (variable `SECMET_DATA_DIR` renseignée par la clé), les données vont sur la clé, les lots sont proposés à chaque démarrage et l'export est actif ; sur un poste installé, les données restent dans le dossier de l'utilisateur et les lots sont optionnels |
| **Lot** | Ensemble d'effacements regroupés sous un identifiant (`L-…`) et un opérateur : machines traitées successivement avec la même clé, ou disques traités au fil des sessions sur un poste ; états : ouvert, clôturé (archivé, lecture seule) ou abandonné (aucun disque effacé) |
| **Journal du lot** | Fichier `journal.ndjson` d'un lot, qui ne fait que croître : chaque ligne contient l'empreinte de la précédente, si bien qu'une ligne supprimée ou modifiée se détecte |
| **Entrée interrompue** | Entrée de lot restée « en cours » après une coupure, un plantage ou une clé arrachée ; signalée au démarrage suivant pour être refaite ou consignée en échec |
| **Manifeste de lot** | Fichier `<LOT-ID>-manifest.json` produit à la clôture et signé : identifiant du lot, opérateur, dates, liste des certificats avec leurs empreintes, empreinte finale du journal ; `secmet-verify --manifest` le confronte aux attestations du dossier |
| **Clé de signature (des certificats)** | Paire Ed25519 propre à un poste ou à une clé USB, qui signe les attestations ; sa clé publique s'enregistre chez le client pour la vérification ; elle n'est jamais incluse dans l'image publiée de la clé, mais créée au premier démarrage |
| **Clé de version** | Paire Ed25519 distincte des clés de signature des certificats, qui signe les releases (`SHA256SUMS`, `latest.json`) ; sa clé publique est intégrée à l'application, au système de la clé et à SecMet Clé USB |
| **Release** | Version publiée sur la page des releases du dépôt, sous un tag `vX.Y.Z` : paquets, AppImage, image de la clé, livrable de la clé, SecMet Clé USB pour Linux et Windows, `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` ; voir [Annexe N](Annexe_N_mises_a_jour.md) |
| **`SHA256SUMS` / `SHA256SUMS.sig`** | Liste des empreintes SHA-256 de tous les fichiers d'une release, et sa signature par la clé de version ; vérifiables avec `secmet-release verify` |
| **`latest.json`** | Annonce signée de la dernière version (numéro, date, notes, adresse de `SHA256SUMS`), consultée par SecMet pour signaler une mise à jour et par SecMet Clé USB pour connaître l'image courante |
| **Image de la clé** | Fichier `secmet-key_<version>_x86_64.img.xz` publié avec chaque release : la clé complète, à écrire avec SecMet Clé USB, Raspberry Pi Imager, Rufus ou `dd` ; le livrable `secmet-live_<version>_x86_64.tar` sert à fabriquer une clé depuis Linux avec le script fourni |
| **SecMet Clé USB** | Outil graphique (`secmet-key-writer`, Linux et Windows) qui télécharge ou reprend l'image de la clé, vérifie la signature de la release et l'empreinte de l'image, puis l'écrit sur une clé amovible et la relit entièrement ; il complète la table de partitions GPT au bout de la clé pendant l'écriture ; voir [Fabriquer sa clé](../Fabriquer_sa_cle.md) |
| **Magasin `app/`** | Dossier de `SECMET-DATA` où une clé range les mises à jour de SecMet installées depuis ses Réglages (les deux plus récentes) ; au démarrage, la plus récente dont la signature est valide est déployée, sinon la précédente, à défaut la version de l'image |
| **Réserve d'horloge** | Mention portée sur le certificat quand la date n'est pas garantie : horloge non synchronisée par le réseau, horloge douteuse conservée sur confirmation, ou date saisie par l'opérateur |
