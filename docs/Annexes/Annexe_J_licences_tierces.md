# Annexe J — Licences tierces

> Annexe du Cahier des charges SecMet (document interne) — v0.3.5
>
> Document de conformité pour la distribution et la commercialisation de SecMet.  
> **Version produit** : 0.3.5  
> **Date** : 3 novembre 2026

---

## 1. Logiciel SecMet

| Élément | Licence |
|---|---|
| Code source (`secmet`, `secmet-device`, `secmet-core`, `secmet-helper`, `secmet-attest`, `secmet-verify`, `secmet-release`) | **GPL-3.0-or-later** — voir [LICENSE](../../LICENSE) |
| Documentation du projet (hors tiers) | **GPL-3.0-or-later** (même projet) |
| Nom « SecMet » et logo (lion) | **Marque**, tous droits réservés — voir [content/LICENSE](../../content/LICENSE) |

SecMet est un logiciel libre sous **GNU GPL version 3 ou ultérieure** (décision du 3 novembre 2026, avant la première publication des sources ; les versions antérieures, jamais distribuées en sources, étaient sous MIT). Conséquences :

- le code peut être utilisé, étudié, modifié et commercialisé par quiconque ;
- toute version **distribuée** (modifiée ou non, intégrée ou non) doit l'être sous GPL, **avec ses sources** et la notice de copyright ;
- le nom et le logo ne suivent pas la GPL : une version modifiée doit être renommée (voir §6) ;
- les auteurs restent libres de proposer d'autres conditions (licence commerciale, version privée) tant qu'ils détiennent l'intégralité du copyright ; toute contribution externe devra donc faire l'objet d'un accord de cession ou d'une double licence explicite.

---

## 2. Dépendances Rust directes

SecMet est lié **statiquement** aux crates suivantes. Licences permissives, usage commercial autorisé avec conservation des notices.

| Crate | Version | Licence | Usage |
|---|---|---|---|
| `gtk4`, `glib`, `cairo-rs` | 0.9 / 0.20 / 0.20 | MIT (bindings) | Interface graphique |
| `serde`, `serde_json` | 1.0 | MIT **ou** Apache-2.0 | Sérialisation (protocole helper, rapports, attestations) |
| `chrono` | 0.4 | MIT **ou** Apache-2.0 | Horodatage |
| `regex` | 1.x | MIT **ou** Apache-2.0 | Analyse de la sortie TestDisk |
| `dirs` | 5.0 | MIT **ou** Apache-2.0 | Chemins utilisateur |
| `rand` | 0.8 | MIT **ou** Apache-2.0 | Aléatoire (identifiants de session) |
| `image` | 0.25 | MIT **ou** Apache-2.0 | Conversion du logo personnalisé |
| `sha2` | 0.10 | MIT **ou** Apache-2.0 | Empreintes (rapports, preuves, clés) |
| `chacha20` | 0.10 | MIT **ou** Apache-2.0 | Canaris et motifs d'écriture hôte |
| `ed25519-dalek` | 2.x | **BSD-3-Clause** | Signature des attestations |
| `zeroize` | 1.x | Apache-2.0 **ou** MIT | Effacement mémoire des secrets (mots de passe ATA, graines) |
| `flate2` (backend Rust) | 1.x | MIT **ou** Apache-2.0 | Compression de l'attestation dans le QR |
| `qrcode` | 0.14 | MIT **ou** Apache-2.0 | Matrice QR |
| `thiserror` | 2.x | MIT **ou** Apache-2.0 | Types d'erreur |
| `libc` | 0.2 | MIT **ou** Apache-2.0 | ioctl `SG_IO`, NVMe, `fstat`, alignement |
| `ureq` | 2.x | MIT **ou** Apache-2.0 | Client HTTPS de la vérification des mises à jour |
| `rustls`, `rustls-webpki`, `ring` (via `ureq`) | 0.23 / 0.103 / 0.17 | Apache-2.0 ou ISC ou MIT ; ISC ; Apache-2.0 **et** ISC | TLS sans OpenSSL |
| `webpki-roots` (via `ureq`) | 1.x | **CDLA-Permissive-2.0** | Certificats racines Mozilla embarqués |

Les crates **transitives** (gdk4, pango, gio, curve25519-dalek, icu via `url`, etc.) héritent de licences permissives (MIT, Apache-2.0, BSD, ISC, Unicode-3.0, CDLA-Permissive-2.0). Aucune licence copyleft. Liste complète :

```bash
cargo install cargo-license
cargo license --all-features --avoid-build-deps --avoid-dev-deps
```

---

## 3. Bibliothèques système (liaison dynamique)

| Composant | Licence | Obligation distribution |
|---|---|---|
| **GTK 4** (`libgtk-4-1`) | **LGPL-2.1+** | Mentionner l'usage de GTK ; liaison dynamique, bibliothèque remplaçable |
| **GLib, GIO, Cairo, Pango, GdkPixbuf, librsvg** | **LGPL-2.1+** / MPL-2.0 (cairo) | Idem (chaîne GTK) |
| **PolicyKit** (`pkexec`, `polkitd`) | **LGPL-2.0+** | Dépendance système, non redistribuée |
| **glibc** | LGPL-2.1+ | Jamais embarquée |

Texte LGPL-2.1 : https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html

### 3.1 Cas de l'AppImage

L'AppImage **embarque** GTK 4 et sa chaîne (GLib, Pango, Cairo, GdkPixbuf, librsvg, chargeurs d'images, schémas) copiées depuis Ubuntu 24.04. Ces bibliothèques restent sous LGPL : la liaison est **dynamique** (fichiers `.so` distincts dans l'image, remplaçables après extraction `--appimage-extract`), les notices de licence Ubuntu accompagnent les paquets d'origine, et SecMet doit **indiquer leur présence et leur licence** dans sa documentation livrée (`THIRD_PARTY_LICENSES.md`, ce document). Aucune modification de ces bibliothèques n'est effectuée.

Outils d'assemblage (`linuxdeploy`, `linuxdeploy-plugin-gtk`, MIT) : utilisés à la construction, **non redistribués**.

---

## 4. Outils système invoqués

Depuis la v0.3.0, SecMet n'invoque **aucun outil externe** pour l'inventaire, le SMART, les commandes firmware, l'écriture hôte, l'inventaire du banc (DMI via sysfs) ou le rendu PDF. Le seul programme lancé en sous-processus est :

| Outil | Paquet Debian | Licence | Rôle |
|---|---|---|---|
| `testdisk` | testdisk | **GPL-2.0+** | Validation post-effacement optionnelle (lecture seule), lancée par `secmet-helper` |
| Evince / Okular / Atril | (lecteur PDF) | **GPL-2.0+** | Consultation des certificats, ouverts via le gestionnaire du bureau |

L'appel en sous-processus ne fait pas de SecMet une œuvre dérivée de ces outils GPL.

---

## 5. Outils embarqués (`.deb`, `.rpm` et AppImage)

Lors de la construction, `testdisk` et ses bibliothèques privées (libntfs-3g, libext2fs, libncursesw, libjpeg, libuuid…) sont copiés sous `/usr/lib/secmet/{bin,lib}/` (paquets `.deb` et `.rpm`) ou `usr/lib/secmet/` (AppImage, installés sur l'hôte au premier lancement).

| Binaire embarqué | Licence | Sources |
|---|---|---|
| `testdisk` (7.1, Ubuntu 24.04 ; version Fedora pour le `.rpm`) | **GPL-2.0+** | https://www.cgsecurity.org/ (sources Ubuntu : `apt source testdisk` ; Fedora : `dnf download --source testdisk`) |

Obligations GPL-2.0 : texte de la licence livré (`gpl-2.0.txt`), **offre de sources** documentée dans [packaging/bundled-tools-compliance.txt](../../packaging/bundled-tools-compliance.txt) (installé sous `/usr/share/doc/secmet/`), binaire non modifié.

---

## 6. Assets graphiques

| Fichier | Origine | Licence |
|---|---|---|
| `content/SecMet_icon.svg`, `content/SecMet_logo.svg`, `crates/secmet-attest/resources/secmet_logo.jpg` | Création SecMet | **Marque** (SecMet Contributors, tous droits réservés) : reproductibles avec le logiciel non modifié ; une version modifiée doit être renommée et livrée sans eux |
| `content/device-hdd.svg` | [Bootstrap Icons](https://icons.getbootstrap.com/) (`bi-device-hdd`) | **MIT** (© Bootstrap Team) |

Le logo personnalisé des certificats, s'il est configuré par l'exploitant, relève de ses propres droits.

Le PDF utilise les polices standard PDF (Helvetica, Helvetica-Bold, Courier), fournies par le lecteur : aucune police n'est embarquée. Seules les largeurs de caractères de Helvetica (métriques AFM publiées par Adobe pour les 14 polices standard) sont reprises dans `secmet-attest::pdf` pour la mise en page.

Voir [content/LICENSE](../../content/LICENSE).

---

## 7. Inspirations et crédits (hors dépendances)

| Projet | Licence | Lien | Rôle pour SecMet |
|---|---|---|---|
| **QDiskInfo** | **GPL-3.0** | https://github.com/edisionnano/QDiskInfo | Inspiration UX de la présentation SMART ; module `disk_info` réécrit en Rust/GTK4 sur les données natives du helper. Aucun code QDiskInfo inclus. |
| **Oblivio** | — | — | Référence d'audit (accès ioctl directs, ouverture exclusive, canaris, attestation signée) ayant guidé la refonte v0.3.0 ; aucun code inclus. |

> Aucun code QDiskInfo n'est incorporé ; SecMet étant lui-même sous GPL-3.0-or-later, la question de l'œuvre dérivée ne se pose plus.

---

## 8. Commercialisation — synthèse

| Question | Réponse |
|---|---|
| SecMet peut-il être vendu ? | **Oui** (la GPL n'interdit pas la vente ; l'acheteur reçoit les sources et les mêmes droits) |
| Les sources sont-elles publiques ? | **Oui**, par choix (GPL-3.0-or-later, dépôt public) ; un concurrent peut les reprendre mais doit publier ses modifications sous GPL et renommer le produit |
| GTK / LGPL bloque-t-il la vente ? | **Non**, avec mentions LGPL et liaison dynamique (y compris dans l'AppImage) |
| TestDisk (GPL) embarqué bloque-t-il ? | **Non**, à condition de livrer la licence et l'offre de sources (fait) |
| QDiskInfo (inspiration) oblige-t-il la GPL ? | Sans objet (réécriture indépendante, et SecMet est GPL) |
| Point d'attention principal | **Redistribution de TestDisk** (`.deb`, `.rpm` et AppImage) → conformité GPL-2.0 ; et **mention LGPL** pour la chaîne GTK embarquée dans l'AppImage |

---

## 9. Fichiers installés avec le paquet Debian

| Chemin | Contenu |
|---|---|
| `/usr/share/doc/secmet/LICENSE` | Licence GPL-3.0 SecMet (notice + texte intégral) |
| `/usr/share/doc/secmet/content-LICENSE` | Marque : nom et logo SecMet |
| `/usr/share/doc/secmet/THIRD_PARTY_LICENSES.md` | Ce document (copie de l'Annexe J) |
| `/usr/share/doc/secmet/USER_GUIDE.md`, `GUIDE_DEPLOIEMENT_BANC.md` | Annexes G et H |
| `/usr/share/doc/secmet/Annexes/` | Annexes (guide utilisateur, déploiement, clé live, mises à jour…) |
| `/usr/share/doc/secmet/copyright` | Synthèse Debian-style |
| `/usr/share/doc/secmet/bundled-tools-compliance.txt`, `gpl-2.0.txt` | Conformité TestDisk embarqué |
| `/usr/share/doc/secmet/content-LICENSE` | Licence des assets graphiques |

Le paquet `.rpm` installe les mêmes documents sous `/usr/share/doc/secmet/` ; l'AppImage les contient sous `usr/share/doc/secmet/`.
