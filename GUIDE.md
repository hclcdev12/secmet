# SecMet — Mode d’emploi (rapide)

> **Attention** : l’effacement est définitif. Vérifiez toujours les disques cochés avant de lancer.

## Effacer un disque

1. Branchez le(s) disque(s).
2. Si besoin, cliquez **Rafraîchir**.
3. Cochez le(s) disque(s) à effacer (ou **Tout sélectionner**).
4. Cliquez **Lancer la suppression sécurisée**.
5. Confirmez. Attendez la fin (badge **TERMINÉ**, **ÉCHEC** ou **À DÉTRUIRE**).

Pour arrêter un effacement en cours : bouton **⏹** sur la ligne du disque (écriture ou sanitize ; un Format NVM ou un Security Erase déjà lancé ne s’interrompt pas).

- **ÉCHEC** : le disque peut être relancé ; **Relancer les échoués** relance uniquement les disques en ÉCHEC.
- **À DÉTRUIRE** : aucune voie logicielle ne reste. Ne relancez pas : remettez le disque à la destruction physique.
- Disque **verrouillé** par un mot de passe ATA (badge **VERROUILLÉ** ; LOCKED en anglais, GESPERRT en allemand) : si le mot de passe du propriétaire est connu, **Déverrouiller** le lève ; sinon, **Marquer à détruire** sur sa ligne l’enregistre comme « destruction physique requise » sans rien lancer.

## Disque gelé (« frozen »)

Si un SSD SATA est gelé (badge ou sous-titre **GELÉ** ; FROZEN en anglais, EINGEFROREN en allemand) et ne peut pas être effacé :

1. Cliquez **Dégeler** (Unfreeze, Auftauen).
2. Cliquez **Mettre en veille**, puis réveillez la machine.
3. Cliquez **Rafraîchir**, puis relancez l’effacement.

## Vérifier avec TestDisk (optionnel)

Pour contrôler qu’aucune partition n’est récupérable après un effacement réussi :

1. Cliquez **TestDisk**.
2. Cochez les disques à tester.
3. Validez et lisez le résultat dans la fenêtre (OK / partitions détectées).

## Certificat PDF

**Avant de débrancher** les disques :

1. Cliquez **Générer le PDF**.
2. Saisissez votre nom.
3. Choisissez le format (une page par disque, ou certificat groupé).
4. Cochez les disques à inclure → **Générer le PDF**.
5. Attendez : une boîte de dialogue s’ouvre pour ouvrir le PDF ou le dossier.

Le fichier se trouve dans `~/.secmet_logs/certificates/`.

## Sur la clé USB SecMet

> Le bureau s’ouvre sans connexion. Au démarrage de SecMet, le mot de passe demandé est celui du compte `secmet` (**`secmet`** par défaut sur l’image publiée).

1. Au démarrage, SecMet propose d’**ouvrir un lot** (ou de **continuer** le lot en cours, ou de travailler **sans lot**).
2. Effacez les disques de la machine comme ci-dessus. En fin d’effacement, les certificats sont **générés automatiquement** dans le lot (`lots/<lot>/certificates` sur la clé) ; **Générer le PDF** ne fait plus qu’ouvrir ce dossier.
3. Éteignez la machine et passez à la suivante.
4. En fin de série, **Clôturer** produit le certificat groupé et le manifeste signé, copiés sur la partition `SECMET-EXP` (lisible sous Windows). Un lot sans disque effacé peut être abandonné (**Abandonner le lot**).

Sur un poste installé, la case **Mode lot** des Réglages active le même regroupement en lots.

## Réglages et mises à jour

Icône engrenage : langue, logo des certificats, nom de l’opérateur, mode lot, mises à jour. Un **point jaune** sur l’engrenage signale une nouvelle version : **Installer la mise à jour** (poste avec `apt` ou `dnf`, hors AppImage, aucune opération en cours et aucun lot ouvert : sinon la raison s’affiche, avec **Clôturer le lot…**) puis **Relancer SecMet**. Section **Clé de signature des certificats** : identifiant (`key_id`) et clé publique du poste, à copier pour la liste de confiance du client.

## Bouton « i » (couleur)

| Couleur | Sens |
|---------|------|
| Vert | Disque OK |
| Orange | À surveiller |
| Rouge | Problème SMART |
| Gris | Info indisponible |

Cliquez sur **i** pour voir le détail SMART.

Guide complet : [Annexe G, guide utilisateur](docs/Annexes/Annexe_G_guide_utilisateur.md).
