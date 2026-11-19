# Annexe K — Présentation du produit

> Documentation SecMet — v0.4.0

## Pourquoi existe-t-elle ?

Quand on récupère, revend ou recycle des ordinateurs et des disques durs, les données qu’ils contiennent ne disparaissent pas en les effaçant « à la main ». Des photos, des documents, des mots de passe peuvent rester accessibles.

**SecMet** existe pour répondre à ce besoin simple : **effacer définitivement les données des disques**, de façon fiable, et **garder une trace** de ce qui a été fait.

C’est pensé pour un usage en **atelier** : on branche un ou plusieurs disques sur un **banc local** (sans serveur ni cloud), ou l’on démarre l’ordinateur à traiter sur une **clé USB SecMet** ; on efface depuis une **interface graphique**, on suit l’**état** de chaque disque à l’écran, on obtient un **certificat signé**. Pas besoin de connaître les commandes ou les détails techniques, l’application s’en charge.

## En une phrase

SecMet est un outil d’atelier qui permet de **supprimer en toute sécurité le contenu des disques durs et des SSD**, puis de **générer un certificat signé** qui atteste l’opération.

## À qui s’adresse-t-elle ?

À toute personne qui traite des disques dans un contexte professionnel ou associatif :

- reconditionnement d’ordinateurs ;
- recyclage de matériel ;
- élimination sécurisée des supports contenant des données sensibles ;
- besoin de **justifier**, certificat à l’appui, qu’un disque a bien été effacé.

Aucune connaissance technique n’est requise pour l’utiliser au quotidien.

## Ce qu’elle fait concrètement

1. **Affiche** les disques branchés sur la machine (type, taille, modèle) et leur **état** (disponibilité, santé, progression).
2. **Permet de choisir** ceux que l’on souhaite effacer.
3. **Lance l’effacement** de manière adaptée au type de disque (disque mécanique ou SSD), avec des méthodes alignées sur NIST SP 800-88 (commande d’effacement du disque lui-même quand il la permet, sinon écriture sur tout le disque), puis **relit** le disque pour contrôler le résultat.
4. **Montre l’avancement** pendant l’opération.
5. **Indique le sort de chaque disque** : prêt au réemploi, ou à remettre à une filière de destruction physique quand l’effacement n’est pas possible ou pas suffisant vu son état.
6. **Produit un certificat PDF** (attestation d’effacement) avec le nom de l’opérateur et les disques traités, utile pour l’archivage ou la traçabilité : une page par disque, ou un certificat groupé pour tout un lot. L’application et le certificat existent en français, en anglais et en allemand.

## La clé USB SecMet et les lots

Les disques des portables ne sont pas toujours faciles à démonter. La **clé USB SecMet** permet de les effacer sur place : on démarre le portable sur la clé, SecMet s’ouvre, on efface le disque interne, on éteint, on passe au portable suivant avec la même clé. La clé démarre la plupart des PC, y compris avec le démarrage sécurisé (Secure Boot) actif et sur les anciens PC.

- Les portables traités à la suite forment un **lot**. Le lot est conservé sur la clé : il survit aux redémarrages, et une coupure de courant ne le perd pas.
- À la fin, l’opérateur **clôture** le lot : SecMet produit un **certificat groupé** (nombre de machines, numéro de série de chacune) et un **manifeste signé** qui scelle le lot, avec, s’il y a lieu, la liste des disques à détruire.
- Les documents sont copiés sur une partie de la clé **lisible sous Windows et macOS**, pour être remis au client ou archivés.
- Chaque clé a **sa propre clé de signature**, dont l’identifiant figure sur chaque certificat.
- Les lots existent aussi sur un banc installé, pour regrouper les disques traités au fil des jours.

## Fabriquer sa clé, y compris depuis Windows

Chaque version de SecMet publie l’image de la clé et un petit outil, **SecMet Clé USB**, pour Linux et pour Windows. L’outil télécharge l’image, **vérifie sa signature**, ne propose que les clés USB (jamais les disques internes), écrit la clé puis la relit entièrement. Sous Windows, il tient en un seul fichier à lancer, sans installation. Voir [Fabriquer sa clé](../Fabriquer_sa_cle.md).

## Les mises à jour

SecMet signale à l’opérateur qu’une nouvelle version est disponible, sans jamais l’imposer ni interrompre un effacement ; sans réseau, il fonctionne normalement. Sur un banc installé par paquet, la mise à jour s’installe depuis les Réglages ; sur la clé, elle est conservée et reprise à chaque démarrage. Chaque version est **signée** : SecMet refuse une mise à jour ou une image de clé qui aurait été modifiée.

## Ce qu’il faut retenir

- L’effacement est **définitif** : les données ne pourront pas être récupérées.
- Le disque sur lequel tourne l’ordinateur, et la clé USB SecMet elle-même, sont **protégés** pour éviter une erreur grave.
- L’objectif n’est pas de « formater » un disque pour le réutiliser vite fait, mais de **détruire les données** de façon sérieuse, avec un **certificat** à la clé.
- Le certificat est **signé** : toute modification se voit, et chacun peut le vérifier hors ligne avec un outil public et gratuit. Il atteste qui a effacé quel disque, comment et avec quel résultat ; ce n’est pas une certification délivrée par un organisme.
- Le banc **dédié** reste la méthode de référence pour traiter des disques en volume. La **clé USB SecMet**, ou un démarrage réseau (**PXE**) sur le poste à traiter, permettent d’effacer un disque interne **sans le démonter**, plus vite qu’en ouvrant un portable pour le brancher au banc. Voir [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md).

## Documents connexes

| Document | Objet |
|----------|-------|
| [Annexe G — Guide utilisateur](Annexe_G_guide_utilisateur.md) | Utilisation au quotidien |
| [Annexe M — Clé live et lots persistants](Annexe_M_cle_live_lots.md) | Clé USB SecMet et lots |
| [Annexe N — Mises à jour et publication](Annexe_N_mises_a_jour.md) | Versions signées et mises à jour |
| [Fabriquer sa clé](../Fabriquer_sa_cle.md) | Écrire une clé USB SecMet |
