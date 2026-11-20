# Annexe C — Plan de recette (critères d'acceptation)

> Documentation SecMet — v0.4.0

Le produit est considéré **conforme aux besoins exprimés** ([Annexe A](Annexe_A_besoins_fonctionnels.md), [Annexe B](Annexe_B_besoins_non_fonctionnels.md)) lorsque les scénarios suivants sont validés par le client (ou son représentant) sur un banc de test représentatif.

## C.1 Scénarios indispensables

| N° | Scénario | Résultat attendu |
|----|----------|------------------|
| R-01 | Installation du `.deb` sur un poste Debian/Ubuntu vierge | Application accessible depuis le menu, lancement sans erreur (`pkexec` setuid et policy en place) |
| R-02 | Branchement d'un HDD USB, rafraîchissement, sélection, effacement | Disque effacé par écriture hôte (catégorie Clear), statut « TERMINÉ », progression (pourcentage, débit, ETA) visible, vérification par relecture « OK » affichée |
| R-03 | Branchement d'un SSD SATA interne, effacement | Méthode firmware retenue par le moteur (sanitize, Security Erase), catégorie NIST indiquée, rapport disponible |
| R-04 | Tentative d'effacement du disque système | Opération refusée (carte INDISPONIBLE, helper refuse `ExecutePlan`) |
| R-05 | Effacement de deux disques en parallèle | Les deux opérations aboutissent ou signalent un échec explicite |
| R-06 | Arrêt d'un effacement par écriture hôte en cours | Opération interrompue entre deux blocs, statut « ARRÊTÉ » ; certificat en **Échec** pour ce disque ; vérification non effectuée |
| R-07a | Génération du certificat PDF **individuel** après session complète | PDF multi-pages créé (une page par disque coché), nom opérateur présent, attestation JSON signée par disque, QR et signature visibles |
| R-07b | Génération du certificat PDF **groupé (lot)** | PDF unique avec tableau de lot, JSON signé unique `…-lot.json` |
| R-08 | Migration depuis un dossier `~/.shred_graphique_logs/` existant | Données accessibles sous `~/.secmet_logs/` ; ancien dossier sauvegardé en `~/.shred_graphique_logs.bak` si fusion |
| R-09 | Affichage du logo SecMet dans l'application et sur le PDF | Logo visible et lisible |
| R-10 | Génération PDF avec sous-ensemble de disques cochés | PDF et JSON ne contiennent que les disques sélectionnés |
| R-11 | Vérification post-effacement par relecture sur un effacement réussi | « Vérification : OK — N blocs relus, 0 anomalie » affiché ; en cas de KO, bouton « Détails vérification » accessible |
| R-12 | Tentative de lancement sans bit setuid sur `pkexec` | Application refuse de démarrer, message d'erreur explicite |
| R-17 | Vérification d'un certificat hors ligne avec `secmet-verify` | Clé du banc inconnue → `clé non vérifiée`, code 1 ; avec `--trust <clé ou key_id>` → `VALIDE`, code 0 ; JSON altéré → `INVALIDE` |

## C.2 Scénarios souhaitables

| N° | Scénario | Résultat attendu |
|----|----------|------------------|
| R-13 | SSD SATA en état frozen (badge GELÉ) : utilisation du bouton Dégeler | Disque débloqué après mise en veille, effacement firmware possible |
| R-04b | `ExecutePlan` envoyé directement au helper pour le disque système (labo sécu) | Helper refuse (`Refused`, motif `HostsSystem`) hors parcours UI |
| R-14 | SSD USB : effacement tenté | Aucune commande firmware envoyée à travers le pont ; écriture hôte, catégorie Clear, réserve « pont USB » sur le certificat |
| R-15 | Poste sans `hdparm`, `nvme-cli` ni `smartmontools` | Inventaire, SMART et effacements firmware fonctionnent (accès natifs par le helper) |
| R-16 | Validation TestDisk optionnelle après effacement réussi | Bouton **TestDisk** actif ; dialogue de sélection ; analyses séquentielles ; verdict dans le dialogue ; journal archivé dans la session ; voir [Annexe L](Annexe_L_validation_testdisk.md) |
| R-18 | Premier lancement de l'AppImage sur un poste sans composant | Dialogue « Composant système requis », un mot de passe, helper + policy + TestDisk installés, inventaire affiché ; second lancement sans dialogue |
| R-19 | Réglages : changement de langue, de logo et du nom de l'opérateur | Fenêtre reconstruite dans la langue choisie ; certificats générés ensuite avec le logo choisi ; nom de l'opérateur pré-rempli dans le dialogue **Générer le PDF** ; réglages persistants |
| R-20 | Disque ATA verrouillé par un mot de passe connu : bouton Déverrouiller | Avec le bon mot de passe, disque déverrouillé et mot de passe levé ; avec un mauvais mot de passe, refus du disque, aucune modification |
| R-21 | Disque dont le SECURITY ERASE a été interrompu (mot de passe SecMet encore posé) | Au scan suivant, le mot de passe est levé automatiquement et le coffre nettoyé ; en cas de refus, l'entrée est mise de côté sans consommer d'autres tentatives |
| R-23 | Mises à jour : poste hors ligne, puis canal annonçant une version plus récente, puis `latest.json` altéré ou signé par une autre clé | Hors ligne : démarrage normal, « Vérification impossible » dans les Réglages, aucun blocage ; version plus récente : point jaune et section Mises à jour renseignée ; fichier altéré ou clé inconnue : ignoré, aucune annonce |
| R-22 | Interface en anglais puis en allemand : génération d'un certificat individuel et d'un certificat de lot | PDF entièrement rédigé dans la langue choisie (libellés, dates, méthode, réserves, résultats du lot), sans texte tronqué ; journaux en français ; `secmet-verify` valide les attestations |

## C.3 Clé USB SecMet et lots

Scénarios issus de l'[Annexe M](Annexe_M_cle_live_lots.md) (§M.11), joués sur une clé écrite depuis l'image publiée et sur des portables représentatifs.

| N° | Scénario | Résultat attendu |
|----|----------|------------------|
| R-24 | Fabrication d'une clé sous Linux avec SecMet Clé USB (AppImage), image téléchargée puis image déjà présente dans le dossier des téléchargements | Signature de la release puis empreinte de l'image vérifiées avant toute écriture ; image de la version courante prise d'office au lancement ; seuls les disques amovibles de 12 Go ou plus sans partition système proposés ; mot de passe administrateur demandé ; écriture, relecture complète, écran **Clé prête** |
| R-25 | Fabrication d'une clé sous Windows avec `secmet-key-writer_<version>_x86_64.exe` | Au premier lancement, avertissement SmartScreen (*Informations complémentaires* → *Exécuter quand même*), puis invite UAC ; même parcours qu'en R-24 ; après l'écriture, Windows peut proposer de formater la partition de données : **Annuler**, la clé reste intacte et démarre |
| R-26 | SecMet Clé USB sur une image altérée (un octet modifié) ou accompagnée d'un `SHA256SUMS.sig` signé par une autre clé | Vérification en échec, aucune écriture proposée ; même verdict avec `secmet-key-writer --verify <image>` |
| R-27 | Premier démarrage d'une clé écrite depuis `secmet-key_<version>_x86_64.img.xz` | Clé de signature créée sur la partition de données (`key_id` au journal), partition d'export agrandie jusqu'au bout de la clé ; au lancement de SecMet, l'invite d'administration demande le mot de passe du compte `secmet` (`secmet` par défaut dans l'image publiée) ; deux clés écrites depuis la même image ont des `key_id` différents |
| R-28 | Démarrage de la clé sur 4 à 5 modèles de portables, en UEFI avec Secure Boot actif et en BIOS | SecMet s'ouvre maximisé, disque interne listé, clé USB INDISPONIBLE |
| R-29 | Lot de 3 portables, extinction entre chaque | Au démarrage, proposition de continuer le lot ; certificats générés automatiquement en fin d'effacement ; 3 entrées terminées |
| R-30 | Coupure d'alimentation pendant un effacement | Entrée signalée **interrompue** au démarrage suivant ; lot intact |
| R-31 | Même portable traité deux fois | Proposition de remplacer l'entrée ; ancienne entrée « remplacée » |
| R-32 | Clôture du lot | PDF groupé avec « Machines traitées : 3 » et séries par ligne ; manifeste valide avec `secmet-verify --manifest` ; livrables copiés sur `SECMET-EXP` |
| R-33 | Attestation retirée ou modifiée dans le dossier du lot après clôture | Désaccord signalé par `secmet-verify --manifest` (attestation manquante, modifiée ou en trop) |
| R-34 | Clé branchée sur un PC Windows | Partition d'export `SECMET-EXP` lisible ; si Windows propose de formater une partition qu'il ne lit pas, **Annuler** ne modifie rien |
| R-35 | Horloge du BIOS en 2000 | Date signalée au démarrage ; **Garder l'horloge du poste** ou **Utiliser cette date** ; réserve sur la date du certificat |
| R-36 | Lot ouvert sans aucun disque effacé, puis **Clôturer** | **Abandonner le lot** proposé ; lot archivé avec son journal, sans certificat groupé ni manifeste ; rien copié sur l'export |
| R-37 | Clôture avec la partition d'export absente ou pleine | Clôture aboutie, message explicite ; **Réessayer la copie** ou **Recopier les lots clôturés sur la partition d'export** copie les livrables manquants |
| R-38 | **Nettoyage de la clé** : avec un lot ouvert, puis avec un lot clôturé non recopié sur l'export, puis après recopie | Refus tant qu'un lot est ouvert ; refus du groupe « lots archivés » tant qu'une copie manque ; sinon inventaire et confirmation, seuls les groupes cochés sont supprimés, réglages, clé de signature et mises à jour conservés, nettoyage noté dans `logs/cleanup.log` |
| R-39 | Poste installé, réglage **Mode lot** activé : deux sessions de disques, puis clôture, avec une clé SecMet branchée | Certificat groupé (le banc comme machine unique, une ligne par disque) et manifeste signé dans `~/.secmet_logs/archive` ; rien n'est copié sur la clé branchée |
| R-40 | Disque verrouillé par un mot de passe ATA inconnu : **Marquer à détruire**, puis **Relancer les échoués** | Badge **À DÉTRUIRE**, sort « destruction physique requise » au certificat ; disque non relancé ; à la clôture du lot, bordereau de destruction avec son numéro de série |

## C.4 Mises à jour

Scénarios issus de l'[Annexe N](Annexe_N_mises_a_jour.md) (§N.9), complétant R-23. Un canal servi sur la boucle locale (`SECMET_UPDATE_URL=http://127.0.0.1:<port>/latest.json`) permet de les jouer sans publier.

| N° | Scénario | Résultat attendu |
|----|----------|------------------|
| R-41 | Case « Vérifier automatiquement les mises à jour » décochée | Aucune requête réseau ; **Vérifier maintenant** reste possible |
| R-42 | Poste installé par `.deb` ou `.rpm`, version plus récente annoncée : **Installer la mise à jour** | Téléchargement, signature de `SHA256SUMS` puis empreinte du paquet vérifiées, mot de passe administrateur demandé, installation, **Relancer SecMet** démarre la nouvelle version ; paquet altéré : refus, rien installé ; bouton absent, raison affichée, pendant chaque opération (effacement, analyse des disques, TestDisk, chargement de la liste, PDF, clôture, déverrouillage, dé-frozen), et réapparu à sa fin sans rouvrir les Réglages ; Réglages et fenêtre principale non fermables pendant l'installation ; AppImage : pas de bouton, note « remplacez le fichier » ; Fedora 44 avec une release `fc42` + `fc44` : paquet `fc44` installé |
| R-43 | Clé USB SecMet : **Installer la mise à jour**, puis redémarrage | Paquet rangé dans `app/<version>` sur la partition de données ; au démarrage suivant, version vérifiée et déployée (trace `secmet-app-update` dans le journal) ; paquet du magasin altéré ou déploiement échoué : version précédente, à défaut celle de l'image ; lot ouvert sous l'ancienne version au redémarrage : version reportée (journal), le lot la garde, déployée au démarrage qui suit sa clôture |
| R-44 | Vérification d'une release téléchargée : `secmet-release verify <dossier> --trust <clé publique de version>` | Code 0 avec la clé de version ; fichier modifié, signature d'une autre clé ou version incohérente : refus |
| R-45 | **Lot ouvert** (clé USB, ou poste en mode lot), version plus récente annoncée : Réglages | Pas de bouton **Installer la mise à jour** ; « Lot L-… ouvert : clôturez-le avant d'installer » et bouton **Clôturer le lot…** ; après la clôture, le bouton apparaît ; une fois la version installée, aucun lot ne s'ouvre avant **Relancer SecMet** |
| R-46 | Clôture d'un lot, puis Réglages, section **Clé de signature des certificats** | `cle-publique-secmet-<key_id>.pub` avec les livrables (sur `SECMET-EXP` depuis la clé, dans l'archive sur un poste), identique à `attestation-key.pub` ; Réglages : même `key_id` et même clé, **Copier** la place dans le presse-papiers ; poste sans clé : « Aucune clé de signature pour l'instant », aucun fichier créé |

Depuis la 0.4.1, un lot ouvert bloque l'installation dans l'application (R-45) : un lot n'est traité que par une seule version. Sur la clé, le déploiement au démarrage (`secmet-app-update`) applique la même garde depuis la 0.5.0 : une version rangée dans `app/` n'est pas déployée tant qu'un lot ouvert l'a été sous une autre (R-43).

## C.5 Contrôles automatisés et essais réels

Une partie des scénarios est rejouée à chaque version, sans matériel :

| Contrôle | Où il tourne | Ce qu'il vérifie | Scénarios |
|----------|--------------|------------------|-----------|
| Tests du workspace (`cargo test --workspace`, `make test`) | Intégration continue à chaque push et pull request, chaîne de publication | Lots (cycle de vie, journal chaîné et altération, doublons, entrées interrompues, abandon, recopie des lots archivés), manifeste signé et confrontation aux attestations, horloge, nettoyage de la clé, annonces de version signées et clé inconnue, choix et installation du paquet sous `pkexec`, filtre des disques et complétion de la GPT de SecMet Clé USB | R-17, R-23, R-26, R-29 à R-33, R-35 à R-38, R-42, R-44 (logique) |
| `make test-live-scripts` | Intégration continue, `make test` | Script de démarrage de la clé avec doublures : version la plus récente du magasin d'abord, repli sur signature invalide ou déploiement échoué, magasin absent, garde « lot ouvert » (même version, ancienne, inconnue, clôturé, plusieurs lots), toutes les clés de version passées | R-43 |
| `make live-check` | Chaîne de publication locale (KVM requis, 5 min) | Clé virtuelle démarrée dans QEMU (UEFI) avec un magasin de deux versions : la plus récente vérifiée puis déployée ; non signée par la clé de version, refusée | R-43 |
| `make live-dist-check` | Chaîne de publication locale (KVM requis, 4 min) | Premier démarrage de l'image publiable en BIOS sur une clé virtuelle de 16 Go : clé de signature créée, export agrandi, version du magasin déployée | R-27 |
| `make release` | Poste du responsable des versions | Contrôles ci-dessus, installation de chaque paquet dans son conteneur, signature puis vérification de `SHA256SUMS` | R-01, R-44 |

Essais réels menés avant publication : écriture de clés réelles avec SecMet Clé USB sous Linux et sous Windows 11 (machine virtuelle, clé USB réelle, écritures complètes puis démarrage de la clé écrite), démarrage de la clé et dé-frozen par mise en veille sur portable, lots sur banc installé et sur clé. Les scénarios matériels (R-25, R-28, R-29, R-30, R-34) restent à rejouer chez le client sur ses propres modèles.

## C.6 Non-conformités

Tout écart sur un scénario **indispensable** (R-01 à R-12, R-17, R-24 à R-34) constitue une non-conformité bloquante à la livraison. Les scénarios souhaitables (R-13 à R-16, R-18 à R-23, R-35 à R-44) peuvent faire l'objet d'une livraison avec réserve documentée.
