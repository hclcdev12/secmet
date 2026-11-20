# Annexe A — Besoins fonctionnels

> Documentation SecMet — v0.4.0

Les besoins ci-dessous expriment **ce que le client attend du produit**, indépendamment des choix techniques de réalisation.

## A.1 Gestion des disques

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-01 | L'application doit afficher la liste des disques physiques connectés au banc et leur état opérationnel (badges INACTIF, INDISPONIBLE, EN ATTENTE, ANALYSE…, EN COURS, ARRÊT…, ARRÊTÉ, TERMINÉ, ÉCHEC, VERROUILLÉ, GELÉ, À DÉTRUIRE ; mention « gelé » sur la carte d'un SSD SATA gelé), traduits dans la langue de l'interface | Indispensable |
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
| BF-22 | Le produit doit être installable via un paquet Debian standard (Ubuntu 24.04 et 22.04) ; un paquet `.rpm` (Fedora) et une AppImage sont publiés avec chaque version | Indispensable |
| BF-23 | Les journaux de l'ancien outil `shred-graphique` doivent être migrés automatiquement vers `~/.secmet_logs/` (fusion si les deux dossiers existent ; ancien dossier renommé en `~/.shred_graphique_logs.bak`) | Indispensable |
| BF-24 | Le produit doit être identifiable visuellement (logo SecMet) dans l'interface et sur les documents produits | Indispensable |

## A.5 Sort du support

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-32 | L'application doit conclure, pour chaque disque traité, sur le **sort du support** : réemploi possible, destruction physique recommandée (effacement réussi sur un disque dégradé), à relancer (**ÉCHEC**) ou destruction physique requise (**À DÉTRUIRE** : secteurs illisibles, écritures non persistantes, firmware sans effet, disque condamné par le SMART) ; le sort figure sur le certificat | Indispensable |
| BF-33 | L'application doit permettre de relancer en une fois les disques en échec (**Relancer les échoués**), sans jamais relancer un disque à détruire | Souhaitable |
| BF-34 | L'application doit permettre de **marquer à détruire** un disque qu'elle ne peut pas effacer (verrouillé par un mot de passe ATA inconnu, en lecture seule) : le disque est enregistré dans la session et le lot avec le sort « destruction physique requise » et figure au certificat ; à la clôture d'un lot, le certificat groupé comporte un bordereau de destruction listant les numéros de série concernés | Indispensable |
| BF-35 | L'application doit permettre de déverrouiller un disque ATA dont le mot de passe est connu (**Déverrouiller**), sans jamais contourner un verrou ; un mot de passe posé par SecMet lors d'un SECURITY ERASE interrompu doit être levé automatiquement au scan suivant | Souhaitable |

## A.6 Clé USB SecMet et lots

Voir l'[Annexe M](Annexe_M_cle_live_lots.md) pour le détail.

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-36 | Le produit doit pouvoir démarrer le poste à traiter sur une **clé USB SecMet** (UEFI avec Secure Boot actif, ou BIOS) pour effacer son disque interne sans le démonter ; la clé elle-même n'est jamais proposée à l'effacement (INDISPONIBLE) | Indispensable |
| BF-37 | Les données de travail (lots, certificats, journaux, réglages, clé de signature) doivent persister sur la clé d'un démarrage à l'autre ; chaque clé USB a sa propre clé de signature des certificats, créée à la fabrication ou au premier démarrage de la clé, jamais fournie par l'image publiée | Indispensable |
| BF-38 | L'application doit regrouper les effacements successifs en **lots** : ouverture (opérateur, référence client facultative), reprise au démarrage suivant, signalement des entrées interrompues par une coupure, contrôle de doublon avant l'effacement (Remplacer / Ajouter quand même / Annuler), certificats générés automatiquement en fin d'effacement ; sur la clé, le dialogue de lot s'ouvre à chaque démarrage (**Sans lot** reste possible) ; sur un poste installé, le mode lot s'active dans les Réglages | Indispensable |
| BF-39 | La **clôture** d'un lot doit produire un certificat groupé (« Machines traitées : N » et numéro de série de chaque machine sur la clé) et un **manifeste signé** listant les certificats et leurs empreintes, vérifiable hors ligne (`secmet-verify --manifest`) ; le lot clôturé est archivé en lecture seule ; un lot sans aucun certificat peut être **abandonné** (journalisé, sans certificat groupé ni manifeste) | Indispensable |
| BF-40 | Sur la clé, les livrables d'un lot clôturé et les certificats produits sans lot doivent être copiés sur une partition d'export lisible sous Windows et macOS (`SECMET-EXP`) ; la copie ne bloque pas la clôture et peut être refaite (**Réessayer la copie**, **Recopier les lots clôturés sur la partition d'export**) ; sur un poste installé, rien n'est copié vers une clé branchée | Indispensable |
| BF-41 | Sur la clé, l'application doit détecter une horloge antérieure aux références connues (pile du BIOS épuisée), proposer **Garder l'horloge du poste** ou **Utiliser cette date** (appliquée à SecMet seulement), et porter une réserve sur la date du certificat dès que l'horloge n'est pas synchronisée par le réseau | Indispensable |
| BF-42 | L'application doit permettre de libérer la place sur la clé (**Nettoyage de la clé**, Réglages de la clé) par groupes à cocher, avec inventaire et confirmation ; le nettoyage est refusé tant qu'un lot est ouvert, et les lots archivés ne peuvent être supprimés tant qu'ils n'ont pas leur copie sur la partition d'export | Souhaitable |
| BF-43 | L'opérateur doit pouvoir fabriquer une clé depuis un poste **Linux ou Windows** avec l'outil **SecMet Clé USB** : téléchargement de l'image de la version courante (ou reprise de l'image déjà présente dans le dossier des téléchargements), vérification de la signature de la release puis de l'empreinte de l'image, choix limité aux disques amovibles de 12 Go ou plus sans partition système, écriture puis relecture complète ; voir [Fabriquer sa clé](../Fabriquer_sa_cle.md) | Indispensable |

## A.7 Mises à jour

Voir l'[Annexe N](Annexe_N_mises_a_jour.md) pour le détail.

| ID | Besoin | Priorité |
|----|--------|----------|
| BF-44 | L'application doit signaler qu'une version plus récente est publiée (vérification au démarrage puis toutes les 24 h, désactivable ; **Vérifier maintenant**) : point jaune sur l'icône Réglages, version, date et notes ; seules les annonces signées par la clé de version SecMet sont prises en compte ; sans réseau, rien n'est bloqué | Souhaitable |
| BF-45 | Pour les paquets `.deb` et `.rpm`, l'application doit proposer **Installer la mise à jour** : téléchargement, vérification de la signature de `SHA256SUMS` puis de l'empreinte du paquet, installation sous authentification administrateur, puis **Relancer SecMet** ; le bouton n'est pas proposé pendant un effacement en cours ; l'AppImage se remplace à la main | Souhaitable |
| BF-46 | Sur la clé, une mise à jour installée doit être conservée sur la partition de données et déployée à chaque démarrage, avec repli sur la version précédente (puis sur celle de l'image) si sa signature ou son déploiement échoue. Le lot ouvert n'est pas vérifié avant l'installation : un même lot peut donc contenir des certificats produits par deux versions, chaque attestation indiquant la version du logiciel qui l'a produite | Souhaitable |
