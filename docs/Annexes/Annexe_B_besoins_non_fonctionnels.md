# Annexe B — Besoins non fonctionnels

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

| ID | Catégorie | Exigence | Priorité |
|----|-----------|----------|----------|
| BNF-01 | Sécurité | Seul un utilisateur autorisé (administrateur du poste) peut lancer les opérations sur disques ; authentification requise au démarrage de l'application | Indispensable |
| BNF-02 | Sécurité | Les opérations sensibles ne doivent pas être réalisables sans authentification PolicyKit (`pkexec`) ; `pkexec` doit être installé avec le bit setuid — l'application **refuse de démarrer** sinon | Indispensable |
| BNF-03 | Fiabilité | L'application ne doit pas bloquer indéfiniment si un disque ne répond pas | Indispensable |
| BNF-04 | Traçabilité | Chaque certificat doit permettre d'identifier de façon unique la session et le disque concerné (ou le lot en mode groupé) | Indispensable |
| BNF-05 | Utilisabilité | Interface en français (langue native), anglais et allemand, compréhensible par un opérateur non informaticien | Indispensable |
| BNF-06 | Compatibilité | Fonctionnement sur poste Linux Debian/Ubuntu récent, interface graphique GTK | Indispensable |
| BNF-07 | Déploiement | Le paquet doit pouvoir être installé sans compilation sur le poste cible | Indispensable |
| BNF-08 | Autonomie | Les outils nécessaires à l'effacement SSD doivent être disponibles même si non installés séparément sur le système | Souhaitable |
| BNF-09 | Performance | L'application doit rester utilisable pendant les effacements en parallèle | Indispensable |
| BNF-10 | Maintenabilité | Code source documenté, tests automatisés sur les composants critiques | Souhaitable |
| BNF-11 | Autonomie | L'application doit fonctionner sur le banc sans serveur central ni service cloud requis | Indispensable |
