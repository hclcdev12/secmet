# Annexe C — Plan de recette (critères d'acceptation)

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

Le produit est considéré **conforme au cahier des charges** lorsque les scénarios suivants sont validés par le client (ou son représentant) sur un banc de test représentatif.

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
| R-13 | SSD SATA en état frozen : utilisation du bouton Dé-frozen | Disque débloqué après mise en veille, effacement firmware possible |
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

## C.3 Non-conformités

Tout écart sur un scénario **indispensable** (R-01 à R-12, R-17) constitue une non-conformité bloquante à la livraison. Les scénarios souhaitables (R-13 à R-16, R-18 à R-23) peuvent faire l'objet d'une livraison avec réserve documentée.
