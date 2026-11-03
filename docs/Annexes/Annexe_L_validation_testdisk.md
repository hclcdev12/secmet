# Annexe L — Validation post-effacement TestDisk

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

## L.1 Objet

La validation TestDisk est un **contrôle complémentaire optionnel** proposé après un effacement réussi. Elle vise à détecter d'éventuelles **structures récupérables** (table de partitions, systèmes de fichiers) encore présentes sur le support.

Elle est **distincte** de la vérification par relecture exécutée par le helper à la fin de chaque étape du plan (BF-28, indispensable) :

| Contrôle | Nature | Priorité | Impact certificat |
|----------|--------|----------|-------------------|
| Vérification par relecture | Relecture d'un échantillon de 1 000 blocs (canaris, motifs attendus), recherche de signatures en tête et en queue de disque | Indispensable | Oui (catégorie NIST, blocs relus, anomalies, signatures résiduelles) |
| TestDisk | Analyse en lecture seule de structures récupérables | Souhaitable (BF-30) | Non (journal technique uniquement) |

Ce contrôle **ne constitue pas** une attestation forensique ni une certification normative.

## L.2 Déclenchement

1. Après la fin d'une session d'effacement comportant au moins un disque en **Succès**, le bouton **TestDisk** de la barre basse devient actif.
2. L'opérateur ouvre le dialogue **« Validation TestDisk »**, coche les disques à analyser (pré-cochés par défaut) et clique sur **Valider**.
3. Les analyses sont demandées au helper (commande `VerifyTestdisk`, jeton du disque) et exécutées **séquentiellement**.
4. Le verdict et le bouton **« Afficher les détails »** s'affichent dans ce dialogue ; une réouverture conserve les derniers résultats de la session.

## L.3 Mode d'exécution

Le helper lance TestDisk en mode scripté, en lecture seule :

```text
testdisk /log /logname /run/secmet/testdisk-<id>/testdisk.log /cmd /dev/<disque> analyze,search
```

- **Binaire** : `/usr/lib/secmet/bin/testdisk` (embarqué dans le `.deb` et l'AppImage, avec ses bibliothèques privées sous `/usr/lib/secmet/lib`) s'il existe, sinon le `testdisk` du système.
- **Lecture seule** : aucune écriture sur le périphérique ; une analyse compte comme un job pour le helper (aucun effacement ni commande firmware concurrente sur ce disque, arrêt du daemon différé).
- **Répertoire de travail** : `/run/secmet/testdisk-<id>/` (créé par le helper, 0700 root) pour que TestDisk 7.1, qui ignore souvent `/logname`, écrive `./testdisk.log` au bon endroit.
- **Sortie** : journal, stdout et stderr renvoyés au client, bornés à 256 Kio chacun ; le dossier temporaire est supprimé. Une copie est archivée dans `~/.secmet_logs/sessions/<SEC-…>/`.

## L.4 Source d'analyse

| Priorité | Source | Usage |
|----------|--------|-------|
| 1 | Contenu de `testdisk.log` | **Parsing** des signaux et **affichage** des détails |
| 2 | stdout / stderr (repli) | Uniquement si le journal est absent ; avertissement dans les détails |

Le parsing repose sur des signaux textuels du journal (lignes de partition, mentions GPT/MBR, systèmes de fichiers, « No partition found », etc.). Les séquences ANSI et caractères de contrôle sont retirés avant affichage.

## L.5 Verdicts

| Verdict | Signification |
|---------|---------------|
| **Succès** | Aucune partition ni structure récupérable détectée |
| **Attention** | Une ou plusieurs structures récupérables ont été détectées |
| **Erreur** | Outil absent, refus d'autorisation, ou échec d'exécution |

Le résumé est affiché dans la langue de l'interface et consigné en français dans le journal de session ; il **n'altère pas** le certificat PDF.

## L.6 Dépendance

- `testdisk` est **recommandé** (`Recommends`) par le `.deb` et de toute façon **embarqué** sous `/usr/lib/secmet/bin/` (licence GPL-2.0+, conformité dans `bundled-tools-compliance.txt`). L'AppImage l'embarque de même et l'installe avec le composant système.
- L'usage de la validation reste **optionnel** pour l'opérateur.

## L.7 Limites

- Analyse basée sur le journal textuel TestDisk : pas une lecture forensique bit à bit.
- Un verdict « Attention » n'implique pas une récupération facile des données ; il signale la présence de structures détectables.
- Un verdict « Succès » ne remplace pas une analyse forensique tierce.
- Voir aussi [Annexe E — Normes et conformité](Annexe_E_normes_conformite.md).

## L.8 Correspondance CDC

| Élément | Référence |
|---------|-----------|
| Besoin | BF-30 (souhaitable) — [Annexe A](Annexe_A_besoins_fonctionnels.md) |
| Recette | R-16 (souhaitable) — [Annexe C](Annexe_C_plan_de_recette.md) |
| Glossaire | « Validation TestDisk » — [Annexe D](Annexe_D_glossaire.md) |
| Implémentation | `crates/secmet-helper/src/testdisk.rs` (exécution), `src/verification/testdisk.rs` (analyse), `src/ui/testdisk_ui.rs` (dialogue) |
