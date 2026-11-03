# Annexe A — Besoins fonctionnels

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

Les besoins ci-dessous expriment **ce que le client attend du produit**, indépendamment des choix techniques de réalisation.

## A.1 Gestion des disques

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-01 | L'application doit afficher la liste des disques physiques connectés au banc et leur état opérationnel (INACTIF, INDISPONIBLE, en cours, terminé, arrêté, échec, gelé) | Indispensable |
| BF-02 | L'application doit permettre de rafraîchir la liste après branchement ou débranchement | Indispensable |
| BF-03 | L'application doit indiquer le type de chaque disque (disque dur, SSD, USB, etc.) | Indispensable |
| BF-04 | L'application doit empêcher l'effacement du disque sur lequel fonctionne le banc | Indispensable |
| BF-05 | L'application doit refuser d'effacer un disque dont le type n'a pas pu être déterminé ; le type est établi par IDENTIFY DEVICE / Identify Controller via le helper, avec repli sur l'attribut `rotational` de sysfs ; sur disque **USB**, aucune commande firmware n'est envoyée et le type repose sur sysfs | Indispensable |
| BF-06 | L'application doit permettre de sélectionner plusieurs disques en une fois | Indispensable |
| BF-07 | L'application doit afficher un indicateur de santé du disque (SMART) ; lu nativement par le helper (SMART READ DATA, page Health NVMe) ; l'indicateur reste visible (y compris `?` si indisponible) | Souhaitable |

## A.2 Effacement

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-08 | L'application doit proposer une action unique d'effacement, adaptée automatiquement au type de disque | Indispensable |
| BF-09 | L'application doit permettre d'effacer plusieurs disques en parallèle | Indispensable |
| BF-10 | L'application doit afficher la progression de l'effacement | Indispensable |
| BF-11 | L'application doit permettre d'interrompre un effacement en cours sur disque dur ; l'arrêt manuel est consigné comme **échec** au certificat | Indispensable |
| BF-12 | L'application doit proposer une aide lorsque un SSD SATA est « gelé » et ne peut pas être effacé | Indispensable |
| BF-13 | L'application doit avertir l'utilisateur lorsque l'effacement d'un SSD USB est réalisé en mode « best-effort » | Indispensable |
| BF-14 | L'application doit demander une confirmation explicite avant tout effacement | Indispensable |
| BF-15 | L'application doit afficher un avertissement clair sur le caractère irréversible de l'opération | Indispensable |
| BF-28 | L'application doit exécuter une vérification post-effacement par relecture d'échantillons (canaris ChaCha20 posés avant l'effacement, motifs attendus, recherche de signatures résiduelles) et afficher le verdict (OK / KO / erreur) avec accès aux détails | Indispensable |
| BF-30 | L'application doit proposer une validation post-effacement **optionnelle** via TestDisk (lecture seule, mode scripté) : bouton global dans la barre basse, dialogue de sélection multi-disques (statut + détails isolés dans ce dialogue), exécution séquentielle ; distincte de la vérification par relecture (BF-28) ; voir [Annexe L](Annexe_L_validation_testdisk.md) | Souhaitable |

## A.3 Traçabilité et preuve

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-16 | L'application doit enregistrer automatiquement un journal de chaque session d'effacement | Indispensable |
| BF-17 | L'application doit permettre de générer un certificat PDF après la session | Indispensable |
| BF-18 | Le certificat doit comporter le nom de l'opérateur saisi au moment de la génération ; un nom par défaut peut être enregistré dans les réglages et pré-rempli | Indispensable |
| BF-19 | En mode **individuel**, le certificat doit comporter une page par disque sélectionné (identité, méthode, résultat, empreinte SHA-256) | Indispensable |
| BF-20 | Le certificat doit inclure les informations d'identification du banc d'effacement | Souhaitable |
| BF-21 | L'application doit permettre d'ouvrir le PDF et le dossier de certificats depuis l'interface après génération | Indispensable |
| BF-25 | L'application doit proposer le choix du format de certificat : individuel (une page par disque) ou groupé (lot avec tableau) | Indispensable |
| BF-26 | L'application doit permettre de sélectionner quels disques terminés inclure dans le certificat PDF | Indispensable |
| BF-27 | L'application doit produire des métadonnées JSON miroir à côté du PDF (un fichier par disque en mode individuel ; un fichier unique `…-lot.json` en mode groupé) | Indispensable |
| BF-29 | Le certificat doit inclure une empreinte SHA-256 d'intégrité documentaire (par page ou par lot selon le format) | Indispensable |
| BF-31 | Le certificat doit être rédigé dans la langue de l'interface au moment de la génération (français, anglais, allemand) ; les journaux restent en français | Souhaitable |

## A.4 Installation et continuité

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-22 | Le produit doit être installable via un paquet Debian standard | Indispensable |
| BF-23 | Les journaux de l'ancien outil `shred-graphique` doivent être migrés automatiquement vers `~/.secmet_logs/` (fusion si les deux dossiers existent ; ancien dossier renommé en `~/.shred_graphique_logs.bak`) | Indispensable |
| BF-24 | Le produit doit être identifiable visuellement (logo SecMet) dans l'interface et sur les documents produits | Indispensable |
