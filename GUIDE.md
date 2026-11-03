# SecMet — Mode d’emploi (rapide)

> **Attention** : l’effacement est définitif. Vérifiez toujours les disques cochés avant de lancer.

## Effacer un disque

1. Branchez le(s) disque(s).
2. Si besoin, cliquez **Rafraîchir**.
3. Cochez le(s) disque(s) à effacer (ou **Tout sélectionner**).
4. Cliquez **Lancer la suppression sécurisée**.
5. Confirmez. Attendez la fin (badge **TERMINÉ** ou **ÉCHEC**).

Pour arrêter un effacement en cours (disque dur) : bouton **⏹** sur la ligne du disque.

## Disque « frozen » (gelé)

Si un SSD SATA est gelé et ne peut pas être effacé :

1. Cliquez **Dé-frozen**.
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

## Bouton « i » (couleur)

| Couleur | Sens |
|---------|------|
| Vert | Disque OK |
| Orange | À surveiller |
| Rouge | Problème SMART |
| Gris | Info indisponible |

Cliquez sur **i** pour voir le détail SMART.
