# Annexe B — Besoins non fonctionnels

> Documentation SecMet — v0.4.0

| ID | Catégorie | Exigence | Priorité |
|----|-----------|----------|----------|
| BNF-01 | Sécurité | Seul un utilisateur autorisé (administrateur du poste) peut lancer les opérations sur disques ; authentification requise au démarrage de l'application | Indispensable |
| BNF-02 | Sécurité | Les opérations sensibles ne doivent pas être réalisables sans authentification PolicyKit (`pkexec`) ; `pkexec` doit être installé avec le bit setuid — l'application **refuse de démarrer** sinon | Indispensable |
| BNF-03 | Fiabilité | L'application ne doit pas bloquer indéfiniment si un disque ne répond pas | Indispensable |
| BNF-04 | Traçabilité | Chaque certificat doit permettre d'identifier de façon unique la session et le disque concerné (ou le lot en mode groupé) | Indispensable |
| BNF-05 | Utilisabilité | Interface en français (langue native), anglais et allemand, compréhensible par un opérateur non informaticien | Indispensable |
| BNF-06 | Compatibilité | Fonctionnement sur poste Linux Debian/Ubuntu ou Fedora récent, interface graphique GTK 4 ; la clé USB SecMet démarre la plupart des PC x86-64 en UEFI (Secure Boot actif ou non) comme en BIOS ; SecMet Clé USB fonctionne sous Linux (AppImage) et sous Windows (`.exe` seul, validé sous Windows 11) | Indispensable |
| BNF-07 | Déploiement | Le paquet doit pouvoir être installé sans compilation sur le poste cible ; la clé USB doit pouvoir être fabriquée sans poste Linux (SecMet Clé USB sous Windows, ou image écrite telle quelle avec Raspberry Pi Imager, Rufus ou `dd`) | Indispensable |
| BNF-08 | Autonomie | Les outils nécessaires à l'effacement SSD doivent être disponibles même si non installés séparément sur le système | Souhaitable |
| BNF-09 | Performance | L'application doit rester utilisable pendant les effacements en parallèle | Indispensable |
| BNF-10 | Maintenabilité | Code source documenté, tests automatisés sur les composants critiques | Souhaitable |
| BNF-11 | Autonomie | L'application doit fonctionner sur le banc sans serveur central ni service cloud requis ; la vérification des mises à jour est facultative et son échec ne bloque rien | Indispensable |
| BNF-12 | Sécurité | Les mises à jour et l'image de la clé sont **signées** (Ed25519) par une clé de version distincte des clés de signature des certificats ; l'application, le démarrage de la clé et SecMet Clé USB refusent tout paquet ou toute image dont la signature de `SHA256SUMS` ou l'empreinte ne correspond pas. La signature est faite par la chaîne de publication, soit en intégration continue (GitHub Actions, clé de version fournie au travail de publication comme secret du dépôt), soit sur le poste du responsable des versions (`packaging/release.sh`) | Indispensable |
| BNF-13 | Confidentialité | La vérification des mises à jour ne transmet que l'adresse IP de l'atelier et la version utilisée ; aucune donnée de disque, de certificat ni d'opérateur n'est envoyée | Indispensable |
| BNF-14 | Fiabilité | Une coupure d'alimentation ou une clé arrachée ne doit ni corrompre ni perdre un lot : écritures forcées sur le support puis renommées (fichier absent ou complet), journal de lot chaîné (toute ligne supprimée ou modifiée se détecte), entrée en cours signalée **interrompue** au démarrage suivant ; un mot de passe ATA posé par SecMet sur la clé est conservé sur sa partition de données | Indispensable |
| BNF-15 | Sécurité | Sur la clé USB SecMet, les opérations sur disques restent soumises au mot de passe administrateur, comme sur un banc : compte `secmet`, dont le mot de passe est **`secmet` par défaut** dans l'image publiée. Ce mot de passe étant public, l'exploitant qui le souhaite fabrique ses clés depuis une image construite avec son propre mot de passe (`SECMET_OPERATOR_PASSWORD` à la construction) : un changement par `passwd` sur la clé ne survit pas au redémarrage, le système de la clé repartant de l'image | Indispensable |
| BNF-16 | Traçabilité | Chaque clé USB porte sa propre clé de signature ; son identifiant (`key_id`) figure sur chaque certificat, et la clé publique s'enregistre une fois chez le client pour la vérification hors ligne | Indispensable |
