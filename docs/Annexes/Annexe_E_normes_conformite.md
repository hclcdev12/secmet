# Annexe E — Normes et conformité

> Annexe du Cahier des charges SecMet (document interne) — v0.3.1

## 1. Informations générales

| Élément | Description |
|---------|-------------|
| **Produit** | SecMet — Application d'effacement sécurisé de disques en atelier |
| **Version du produit** | 0.3.1 |
| **Version du présent document** | 2.0 |
| **Date** | 9 octobre 2026 |
| **Document source** | `normes.txt` (synthèse interne, non publiée) — synthèse des normes et pratiques d'effacement sécurisé |
| **Documents associés** | Cahier des charges (document interne, non publié), [Annexe F — Spécification technique](Annexe_F_specification_technique.md) |

### Objet du document

Ce document précise **quelles exigences et pratiques** issues du cadre normatif décrit dans `normes.txt` sont **prises en charge**, **partiellement couvertes** ou **hors périmètre** par SecMet v0.3.1.

Il ne constitue **pas** une attestation de certification officielle (NIST, ANSSI, ISO, etc.). Il sert de référence contractuelle et technique pour situer le produit dans l'écosystème normatif de la sanitization des supports.

---

## 2. Synthèse du cadre normatif (référence `normes.txt`)

| Niveau | Objectif typique | Exemples de méthodes citées |
|--------|------------------|----------------------------|
| **Clear** | Réutilisation interne, assainissement logique | Réécriture (*overwrite*) du support |
| **Purge** | Effacement irréversible avant réemploi ou rebus | Crypto-erase, block erase, commandes constructeur ATA/SCSI/NVMe |
| **Destruction** | Fin de vie sans réemploi | Destruction physique du support |

Références normatives récurrentes : **NIST SP 800-88 Rev. 1** (déc. 2014, guide de référence ; la lecture Rev. 2 / IEEE 2883 est plus conservatrice sur certaines méthodes), **IEEE 2883-2022**, commandes **ATA / SCSI / NVMe** natives, **ISO/IEC 21964** (destruction), **RGPD**, **FIPS 199 / NIST SP 800-53** (organisationnel). Le consensus écarte les schémas à passes multiples (DoD 5220.22-M) au profit de méthodes **adaptées au média**, **vérifiables** et **traçables**.

---

## 3. Positionnement global de SecMet

SecMet est un outil d'**atelier** pour un poste Linux dédié. Il vise à :

1. **sanitizer** des HDD et des SSD (SATA, NVMe) connectés localement, par la meilleure méthode que le disque supporte ;
2. **vérifier** chaque étape par relecture (canaris, motifs attendus, signatures résiduelles) ;
3. **documenter** chaque opération (journaux, attestation signée, certificat PDF) en indiquant la **catégorie NIST réellement obtenue**.

| Dimension normative | Couverture SecMet |
|-------------------|-------------------|
| Sanitization locale HDD / SSD | **Oui** (plan par disque, Purge d'abord, Clear en repli) |
| Commandes constructeur ATA/NVMe | **Oui** (SANITIZE, SECURITY ERASE, Sanitize NVMe, Format NVM, natives via SG_IO et ioctl) |
| Vérification post-effacement | **Oui, par échantillonnage** (1 000 blocs relus, canaris, signatures) ; pas de lecture forensique exhaustive |
| Traçabilité et preuve documentaire | **Oui** (journaux + attestation JSON signée Ed25519 + certificat PDF + QR vérifiable hors ligne) |
| Destruction physique | **Non** (recommandée sur le certificat quand le SMART le justifie) |
| Cloud, fog, stockage distant | **Non** |
| Contrôles organisationnels | **Hors périmètre logiciel** (responsabilité du client) |
| Certification ou labellisation officielle | **Non** |

---

## 4. Correspondance normes ↔ fonctionnalités SecMet

### 4.1 NIST SP 800-88

| Exigence / pratique | Prise en charge | Détail |
|--------------------------------|------------------------|--------|
| Distinction Clear / Purge / Destruction | **Oui** (Clear et Purge) | Chaque méthode porte son niveau ; le certificat indique Purge, Clear ou « non classé » (échec, absence de relecture) ; Destruction recommandée, non exécutée |
| Méthode adaptée au type de support | **Oui** | Moteur de décision sur les capacités déclarées (IDENTIFY, Identify Controller, journal sanitize) et le transport |
| Privilégier les commandes internes au périphérique | **Oui** | Purge firmware d'abord ; écriture hôte seulement en dernier repli, et jamais classée Purge |
| Overwrite pour HDD | **Oui** | SANITIZE OVERWRITE ou SECURITY ERASE (Purge) si supportés ; sinon écriture hôte 1 passe + zéros (Clear), option 3 passes |
| Documentation de l'opération | **Oui** | Attestation signée, certificat, journaux par disque et par session |
| Vérification de l'effacement | **Oui (échantillonnée)** | Canaris posés avant commande firmware, relecture de 1 000 blocs, motifs attendus après écriture hôte, signatures résiduelles en tête et queue |
| Choix du niveau selon la classification des données | **Non automatisé** | Le moteur vise toujours le meilleur niveau disponible ; la dégradation Purge → Clear est annoncée, le client décide de la suite |

**Alignement par type de média (politique Rev. 1 par défaut) :**

| Média | Méthodes dans l'ordre du plan | Niveau |
|-------|-------------------------------|--------|
| SSD NVMe | Sanitize block erase / crypto erase, Format NVM crypto / user data, Sanitize overwrite | **Purge** |
| SSD SATA | SANITIZE crypto scramble / block erase, SECURITY ERASE enhanced | **Purge** ; SECURITY ERASE normal : **Clear** |
| HDD SATA | SANITIZE overwrite, SECURITY ERASE (enhanced ou normal) | **Purge** |
| Tout média, repli | Écriture hôte (motif pseudo-aléatoire recomputable par LBA puis zéros) | **Clear** |
| Disque derrière un pont USB | Écriture hôte uniquement (aucune commande firmware à travers le pont) | **Clear** |

> La politique `nist-800-88-rev2` (disponible dans le moteur) classe Format NVM user-data et SECURITY ERASE sur SSD en Clear, lecture plus conservatrice alignée sur IEEE 2883.

---

### 4.2 IEEE 2883-2022

| Aspect | Prise en charge | Commentaire |
|------------------|------------------------|-------------|
| Sanitization vérifiable | **Oui (échantillonnée)** | Preuve des canaris (empreinte SHA-256 et seed sur le certificat) permettant de recomputer tout motif |
| Méthodes natives du périphérique | **Oui** | Cœur de la stratégie, avec état sanitize sondé jusqu'à achèvement |
| Traçabilité | **Oui** | Horodatage, méthode, critères d'échec, identité disque et banc, signature |
| Couverture universelle des contextes de stockage | **Non** | Poste atelier local uniquement |

---

### 4.3 Interfaces ATA / SCSI / NVMe

| Commande / protocole | Support | Implémentation (native, sans outil externe) |
|----------------------|--------|----------------|
| IDENTIFY DEVICE, SMART READ DATA / THRESHOLDS / RETURN STATUS | **Oui** | ATA PASS-THROUGH (16), repli (12) derrière un SATL de pont USB ou HBA SAS |
| ATA SANITIZE crypto scramble / block erase / overwrite | **Oui** | SANITIZE DEVICE, sondage SANITIZE STATUS EXT toutes les 2 s, motif d'abandon décodé (gelé, non supporté, échec) |
| ATA SECURITY ERASE UNIT (normal / enhanced) | **Oui** | SET PASSWORD (mot de passe aléatoire conservé dans un coffre root), ERASE PREPARE, ERASE UNIT bloquant avec progression sur la durée annoncée, IDENTIFY de contrôle, DISABLE PASSWORD ; levée automatique si interrompu |
| SECURITY UNLOCK / DISABLE PASSWORD avec mot de passe connu | **Oui** | Bouton Déverrouiller ; jamais de contournement |
| Déblocage « frozen » | **Oui** | Veille système puis ré-identification |
| NVMe Identify Controller / Namespace, Get Log Page (SMART, Sanitize) | **Oui** | `NVME_IOCTL_ADMIN_CMD` |
| NVMe Sanitize (block / crypto / overwrite) | **Oui** | Sondage du journal Sanitize Status |
| NVMe Format NVM (SES = 2 crypto, SES = 1 user data) | **Oui** | Bloquant, progression temporelle ; couverture multi-espaces de noms signalée |
| TRIM / blkdiscard | **Non** | Jamais retenu comme méthode (non vérifiable) |
| SCSI SANITIZE générique (SAS natif) | **Non** | Hors périmètre |

Les commandes sont émises par `secmet-helper` sur un nœud ouvert en exclusif, ré-identifié avant écriture ; aucune commande firmware n'est envoyée sur un disque tenu par un job.

---

### 4.4 ISO/IEC 21964 (destruction des supports)

| Exigence | Prise en charge |
|----------|------------------------|
| Destruction physique | **Non** |
| Réemploi du support après sanitization | **Oui** — objectif principal |
| Recommandation de destruction | **Oui** — le plan recommande la destruction après effacement quand le SMART prédit une panne, signale des secteurs réalloués au-delà du seuil, des secteurs en attente ou une fiabilité NVMe dégradée |
| Sort du support (verdict final) | **Oui** — à l'issue de chaque opération, le moteur (`secmet-core::disposition`) conclut : **réemploi possible**, **destruction recommandée** (effacé mais SMART dégradé, firmware acquittant sans effet contourné par écriture hôte, secteurs réalloués hors de portée d'un *Clear*), **destruction requise** (non effacé sans voie logicielle restante : secteurs illisibles à la relecture, écritures non persistantes, firmware sans effet et sans repli abouti, média en lecture seule, échec sur un disque déjà condamné par le SMART) ou **à relancer** (arrêt, erreur de liaison, firmware refusant, blocage levable). Le sort et ses raisons sont signés dans l'attestation (`erasure.disposition`, `erasure.disposition_reasons`), imprimés sur le certificat, et le PDF de lot ajoute un **bordereau de destruction physique** listant les supports à sortir du circuit |
| Disque non effaçable (verrou ATA sans mot de passe) | **Oui** — l'opérateur peut le **marquer à détruire** : certificat et lot portent « destruction physique requise » avec la raison, sans qu'aucun contournement ne soit tenté |

---

### 4.5 RGPD

| Obligation / droit | Contribution de SecMet | Limite |
|--------------------|------------------------|--------|
| Droit à l'effacement (art. 17) | **Oui** — effacement irréversible, catégorie NIST documentée | Supports traités en atelier uniquement |
| Preuve de l'effacement | **Oui** — attestation signée Ed25519, vérifiable hors ligne, certificat nominatif | Valeur probante liée à l'archivage et à la distribution de la clé publique du banc |
| Accountability (art. 5.2) | **Partielle** — traces techniques, opérateur, banc | Gouvernance globale organisationnelle |
| Autres supports (cloud, sauvegardes) | **Non** | Hors périmètre |

---

### 4.6 FIPS 199 et NIST SP 800-53

| Exigence | Prise en charge |
|----------|------------------------|
| Niveau de sanitization selon la criticité des données | **Non** — meilleur niveau disponible, dégradation annoncée |
| Politique documentée, tests périodiques, formation | **Hors périmètre** (documentation fournie) |

---

### 4.7 Références historiques (DoD 5220.22-M)

| Pratique | Statut dans `normes.txt` | Statut dans SecMet |
|----------|--------------------------|-------------------|
| DoD 5220.22-M (3 ou 7 passes) | Dépréciée | **Non visée** |
| Overwrite multi-passes HDD | Plus jugée nécessaire | **Défaut 1 passe + zéros** ; 3 passes proposées par continuité, sans revendication DoD ; aucune passe multiple n'élève le niveau au-delà de Clear |

---

## 5. Exigences pratiques transverses

| Exigence pratique | Couverture | Référence produit |
|-------------------|-------------------|-------------------|
| La suppression logique ne suffit pas | **Oui** | Commandes firmware ou écriture physique complète |
| Méthode adaptée au support | **Oui** | Plan par disque |
| Effacement vérifiable | **Oui (échantillonné)** | Canaris, relecture, signatures ; preuve recomputable (seed) |
| Documentation et traçabilité | **Oui** | `~/.secmet_logs/`, attestation signée, certificat |
| Testabilité | **Partielle** | Journaux détaillés ; `secmet-verify` ; pas de mode « test sans destruction » |
| Cloud / stockage externalisé | **Non** | Non applicable |

---

## 6. Matrice de couverture par méthode normalisée

| Méthode | Niveau | SecMet | Commentaire |
|------------------------|--------|--------|-------------|
| Clear / overwrite | Clear | **Oui** | Écriture hôte vérifiée (1 passe + zéros par défaut) |
| Purge par crypto-erase | Purge | **Oui** | SANITIZE crypto scramble, NVMe Sanitize crypto, Format NVM SES=2 ; l'acquittement est pris en défaut par les canaris si le firmware n'efface pas |
| Purge par block erase | Purge | **Oui** | SANITIZE block erase, NVMe Sanitize block erase, Format NVM SES=1 |
| Secure erase interne (ATA) | Purge / Clear | **Oui** | SECURITY ERASE : Purge sur HDD et en enhanced sur SSD, Clear en normal sur SSD |
| Destruction physique | Destruction | **Non** | Recommandée ou requise sur le certificat (sort du support) ; bordereau de destruction dans le PDF de lot |
| Overwrite sur SSD | — | **Repli seulement** | Clear, jamais Purge |

---

## 7. Traçabilité et vérification

### 7.1 Documentation

- **Attestation JSON signée** par disque : identité, méthode, niveau, politique, horodatages, durée, preuve (seed, blocs relus, anomalies, empreinte), réserves, banc, signature Ed25519 et clé publique.
- **Certificat PDF** : une page par disque (ou lot) reproduisant l'attestation et son QR code.
- **Journaux** : session, détail par disque (étapes, critères d'échec, validation), TestDisk.
- **Version logicielle** sur le certificat ; aucun outil externe n'intervient dans l'effacement.

### 7.2 Vérification

| Mécanisme | Ce qui est vérifié | Ce qui n'est pas vérifié |
|-----------|-------------------|--------------------------|
| Canaris avant commande firmware | Qu'un échantillon de blocs écrit avant l'effacement a bien disparu | Les blocs hors échantillon ; les zones non adressables (over-provisioning) sont couvertes par la méthode Purge elle-même, pas par la relecture |
| Relecture après écriture hôte | Motif attendu de la dernière passe, zéros, lisibilité | Idem |
| Signatures résiduelles | Table de partitions et systèmes de fichiers en tête et queue | Fragments de fichiers |
| Critères firmware | Statut de la commande, état sanitize achevé, mot de passe levé, acquittement attesté | Comportement propriétaire non documenté |
| TestDisk (optionnel) | Structures récupérables | Lecture forensique bit à bit |
| `secmet-verify` | Signature et confiance de la clé | Authenticité du banc au-delà de sa clé |

La vérification SecMet est un **contrôle d'atelier échantillonné et cryptographiquement tracé**, pas une attestation forensique exhaustive.

---

## 8. Limites connues et réserves

1. **USB** : aucune commande firmware ; écriture hôte, Clear.
2. **Disque système** : protégé ; sur média live ou ZFS la racine peut ne pas être rattachée à un disque, ce qui est signalé (bandeau, réserve).
3. **Annulation** : impossible pendant un Format NVM ou un SECURITY ERASE UNIT.
4. **Pas de destruction physique**.
5. **NVMe multi-espaces de noms** : seul l'espace ouvert est couvert par Format NVM sans FNA bit 1 ou par écriture hôte ; signalé.
6. **Pas de certification tierce** : le certificat atteste l'opération réalisée par SecMet.
7. **Échantillonnage** : la relecture couvre 1 000 blocs, pas le média entier.

---

## 9. Synthèse pour la recette et la conformité client

| Référence | Niveau de couverture | Formulation recommandée |
|-----------|---------------------|-------------------------|
| NIST SP 800-88 Rev. 1 | **Alignement** | Purge firmware quand le disque le supporte, Clear sinon, catégorie réellement obtenue sur le certificat |
| IEEE 2883-2022 | **Alignement partiel** | Méthodes natives, vérification échantillonnée, traçabilité signée |
| ATA / NVMe | **Couverture opérationnelle** | Implémentation native complète pour les cas d'usage atelier |
| ISO/IEC 21964 | **Non couvert** | Destruction physique hors périmètre, recommandée si nécessaire |
| RGPD | **Contribution** | Preuve d'effacement signée ; gouvernance à charge du client |
| DoD 5220.22-M | **Non visé** | Schéma historique non revendiqué |

### Formulation de périmètre (proposition)

> SecMet v0.3.1 est un outil d'atelier permettant la sanitization de disques HDD et SSD connectés localement à un banc Linux, par commandes firmware natives (NIST Purge) ou écriture hôte (NIST Clear), chaque étape étant vérifiée par relecture d'échantillons et documentée par une attestation signée vérifiable hors ligne. Il s'aligne sur NIST SP 800-88 Rev. 1 et sur l'esprit d'IEEE 2883-2022. Il ne remplace pas une procédure de destruction physique, ne couvre pas le stockage distant ou cloud, et ne constitue pas une certification officielle au sens des référentiels cités.

---

## 10. Documents de référence

| Document | Rôle |
|----------|------|
| `normes.txt` (synthèse interne, non publiée) | Synthèse du cadre normatif |
| Cahier des charges (document interne, non publié) | Exigences et critères de recette |
| [Annexe F — Spécification technique](Annexe_F_specification_technique.md) | Architecture et implémentation |
| [Annexe G — Guide utilisateur](Annexe_G_guide_utilisateur.md) | Guide opérateur |
| [Annexe H — Déploiement du banc](Annexe_H_deploiement_banc.md) | Déploiement |
| NIST SP 800-88 Rev. 1 | <https://doi.org/10.6028/nist.sp.800-88r1> |

---

*Document établi pour SecMet v0.3.1 — à valider par le maître d'ouvrage pour usage contractuel ou audit interne.*
