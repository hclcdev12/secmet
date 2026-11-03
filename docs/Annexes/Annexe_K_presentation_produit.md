# Annexe K — Présentation du produit

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

## Pourquoi existe-t-elle ?

Quand on récupère, revend ou recycle des ordinateurs et des disques durs, les données qu’ils contiennent ne disparaissent pas en les effaçant « à la main ». Des photos, des documents, des mots de passe peuvent rester accessibles.

**SecMet** existe pour répondre à ce besoin simple : **effacer définitivement les données des disques**, de façon fiable, et **garder une trace** de ce qui a été fait.

C’est pensé pour un usage en **atelier** : on branche un ou plusieurs disques sur un **banc local** (sans serveur ni cloud), on les efface depuis une **interface graphique**, on suit leur **état** à l’écran, on obtient un **document de preuve**. Pas besoin de connaître les commandes ou les détails techniques, l’application s’en charge.

## En une phrase

SecMet est un outil d’atelier qui permet de **supprimer en toute sécurité le contenu des disques durs et des SSD**, puis de **générer un certificat** attestant que l’opération a bien été réalisée.

## À qui s’adresse-t-elle ?

À toute personne qui traite des disques dans un contexte professionnel ou associatif :

- reconditionnement d’ordinateurs ;
- recyclage de matériel ;
- élimination sécurisée des supports contenant des données sensibles ;
- besoin de **prouver** qu’un disque a bien été nettoyé.

Aucune connaissance technique n’est requise pour l’utiliser au quotidien.

## Ce qu’elle fait concrètement

1. **Affiche** les disques branchés sur la machine (type, taille, modèle) et leur **état** (disponibilité, santé, progression).
2. **Permet de choisir** ceux que l’on souhaite effacer.
3. **Lance l’effacement** de manière adaptée au type de disque (disque mécanique ou SSD).
4. **Montre l’avancement** pendant l’opération.
5. **Produit un certificat PDF** (attestation d’effacement) avec le nom de l’opérateur et les disques traités, utile pour l’archivage ou la traçabilité. L’application et le certificat existent en français, en anglais et en allemand.

## Ce qu’il faut retenir

- L’effacement est **définitif** : les données ne pourront pas être récupérées.
- Le disque sur lequel tourne l’ordinateur est **protégé** pour éviter une erreur grave.
- L’objectif n’est pas de « formater » un disque pour le réutiliser vite fait, mais de **détruire les données** de façon sérieuse, avec une **preuve** à la clé.
- Le banc **dédié** reste la méthode de référence pour traiter des disques en volume. Une **clé live USB** ou un boot **PXE** sur le poste à traiter permettent souvent d’effacer un SSD interne **sans le démonter** — plus rapide qu’ouvrir un laptop pour le brancher au banc. Voir [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md).

---

