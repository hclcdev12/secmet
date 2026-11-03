# Annexe D — Glossaire

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

| Terme | Définition |
|-------|------------|
| **Banc d'effacement** | Poste Linux dédié au traitement des disques ; peut être une machine fixe, un environnement live USB ou un poste démarré en PXE |
| **Certificat** | Attestation d'effacement au format PDF, preuve documentée de l'opération (qui, quoi, comment, résultat) ; ne constitue pas une certification normative officielle |
| **Certificat groupé (lot)** | Format de certificat PDF unique regroupant plusieurs disques dans un tableau de lot, avec un JSON miroir unique (`…-lot.json`) |
| **JSON miroir** | Fichier de métadonnées techniques écrit à côté du PDF (par disque ou par lot) ; sert de base au calcul de l'empreinte SHA-256 |
| **État du disque** | Situation d'un disque à un instant donné : disponibilité (`INACTIF` / `INDISPONIBLE`), santé SMART, blocage éventuel (`FROZEN`), progression (`EN COURS`) et résultat (`TERMINÉ`, `ARRÊTÉ`, `ÉCHEC`) |
| **Effacement sécurisé** | Destruction intentionnelle et irréversible des données selon une méthode adaptée au support |
| **Frozen** | État de sécurité ATA empêchant certaines commandes d'effacement sur SSD SATA |
| **Session** | Période d'utilisation comprise entre le lancement de l'application et la génération du certificat |
| **Best-effort** | Tentative d'effacement sans garantie totale (cas typique : SSD USB) |
| **Vérification par relecture** | Contrôle post-effacement exécuté par le helper : relecture d'un échantillon de blocs (canaris posés avant l'effacement, motif attendu de la passe) et recherche de signatures résiduelles (table de partitions, systèmes de fichiers) ; verdict affiché à l'écran (OK / KO / erreur) |
| **Canari** | Motif unique par bloc (ChaCha20, seed de session) écrit sur un échantillon avant une commande firmware ; aucun ne doit subsister après, sinon le firmware a acquitté sans effacer |
| **Attestation signée** | Données du certificat sous forme JSON canonique signée Ed25519 par la clé du banc ; vérifiable hors ligne (`secmet-verify`, QR code) |
| **Helper** | Daemon privilégié `secmet-helper` lancé par PolicyKit ; seul composant qui ouvre les disques et envoie les commandes firmware |
| **Validation TestDisk** | Contrôle complémentaire optionnel post-effacement via TestDisk (lecture seule) ; détecte d'éventuelles structures récupérables ; distinct de la vérification par relecture ; voir [Annexe L](Annexe_L_validation_testdisk.md) |
