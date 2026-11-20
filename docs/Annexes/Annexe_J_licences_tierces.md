# Annexe J — Licences tierces

> Documentation SecMet — v0.4.0
>
> Document de conformité pour la distribution et la commercialisation de SecMet.  
> **Version produit** : 0.4.0  
> **Date** : 19 novembre 2026
>
> Ce document décrit l'état du dépôt et des livrables ; il n'est pas un avis juridique. Les points marqués « à valider » relèvent d'un juriste.

---

## 1. Logiciel SecMet

| Élément | Licence |
|---|---|
| Code source (`secmet`, `secmet-device`, `secmet-core`, `secmet-helper`, `secmet-attest`, `secmet-verify`, `secmet-release`, `secmet-key-writer` — SecMet Clé USB) | **GPL-3.0-or-later** — voir [LICENSE](../../LICENSE) |
| Scripts de construction (`packaging/`, `packaging/live/`, `Makefile`, workflows) | **GPL-3.0-or-later** (même projet) |
| Documentation du projet (hors tiers) | **GPL-3.0-or-later** (même projet) |
| Nom « SecMet » et logo (lion) | Nom et logo **réservés**, tous droits réservés (marque non déposée à ce jour) — voir [content/LICENSE](../../content/LICENSE) |

SecMet est un logiciel libre sous **GNU GPL version 3 ou ultérieure** (décision du 3 novembre 2026, avant la première publication des sources ; les versions antérieures, jamais distribuées en sources, étaient sous MIT). Conséquences :

- le code peut être utilisé, étudié, modifié et commercialisé par quiconque ;
- toute version **distribuée** (modifiée ou non, intégrée ou non) doit l'être sous GPL, **avec ses sources** et la notice de copyright ;
- le nom et le logo ne suivent pas la GPL : une version modifiée doit être renommée (voir §6) ;
- les auteurs restent libres de proposer d'autres conditions (licence commerciale, version privée) tant qu'ils détiennent l'intégralité du copyright ; toute contribution externe devra donc faire l'objet d'un accord de cession ou d'une double licence explicite.

### 1.1 Livrables couverts

Une release publie 14 fichiers. Ce document couvre les onze livrables logiciels :

| Livrable | Contenu | Sections |
|---|---|---|
| `secmet_<v>.ubuntu24.04_amd64.deb`, `secmet_<v>.ubuntu22.04_amd64.deb`, `secmet-<v>-1.fc44.x86_64.rpm`, `secmet-<v>-1.fc42.x86_64.rpm` (Fedora 42 : 0.4.1 seulement, transition) | `secmet` (GTK 4), `secmet-helper`, `secmet-verify`, `secmet-release`, TestDisk et ses bibliothèques privées | §2, §3.1, §4, §5 |
| `SecMet_<v>_x86_64.AppImage` | les mêmes binaires (dont `secmet-verify` et `secmet-release`), plus GTK 4 et sa chaîne copiés d'Ubuntu 24.04, plus TestDisk | §2, §3.2, §4, §5 |
| `secmet-verify_<v>_x86_64`, `secmet-release_<v>_x86_64` | outils en ligne de commande seuls (construits sous Ubuntu 22.04), crates Rust liées statiquement, aucune bibliothèque embarquée (glibc de l'hôte seulement) ; licence GPL-3.0 et avis des crates : ceux du dépôt (`LICENSE`, §2) | §2 |
| `secmet-key-writer_<v>_x86_64.AppImage` | SecMet Clé USB pour Linux (egui), aucune bibliothèque embarquée | §2, §3.3, §6.2 |
| `secmet-key-writer_<v>_x86_64.exe` | SecMet Clé USB pour Windows (egui, wgpu/Direct3D 12, Schannel) | §2, §3.4, §6.2 |
| `secmet-key_<v>_x86_64.img.xz`, `secmet-live_<v>_x86_64.tar` | image de la clé USB : système Ubuntu 24.04 complet, plus le `.deb` SecMet | §7 |

`SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` sont des données produites par SecMet (empreintes, signature, description de la version) : aucune licence tierce.

---

## 2. Crates Rust (liaison statique)

Les binaires SecMet sont liés **statiquement** aux crates Rust de leur graphe de dépendances. Comptes de la 0.4.0 (crates distinctes, nom et version, hors crates du projet) :

| Livrable / cible | Crates compilées dans le binaire |
|---|---|
| `secmet`, `secmet-helper`, `secmet-verify`, `secmet-release` (Linux) | 134 |
| SecMet Clé USB, Linux (`x86_64-unknown-linux-gnu`) | 168 |
| SecMet Clé USB, Windows (`x86_64-pc-windows-msvc`, cible du `.exe` publié) | 174 |
| Ensemble des livrables publiés | 268 |

Les crates de construction (scripts `build.rs`, proc-macros : `syn`, `quote`, `cc`, `system-deps`…) et de développement (`tempfile`…) ne sont pas distribuées.

### 2.1 Licences

| Licence (expression SPDX du `Cargo.toml`) | Crates (extraits) |
|---|---|
| MIT OR Apache-2.0 (y compris l'écriture `MIT/Apache-2.0`) | serde, serde_json, chrono, regex, sha2, chacha20, rand, image, ureq, url, zeroize, uuid, egui, eframe, epaint, egui-wgpu, wgpu, wgpu-core, wgpu-hal, naga, native-tls, windows, windows-sys, x11rb… |
| MIT | gtk4, gdk4, gsk4, glib, gio, cairo-rs, pango, gdk-pixbuf, graphene-rs (et leurs crates `-sys`), rfd, schannel, wayland-client/-backend/-protocols, smithay-client-toolkit, calloop, tiff, harfrust, tracing… |
| Unicode-3.0 | icu_* , litemap, tinystr, writeable, yoke, zerofrom, zerotrie, zerovec, potential_utf (via `url`/`idna`) |
| Apache-2.0 seule | winit, glutin et ses crates `*_sys`, gethostname, unicode-general-category, codespan-reporting (Windows) |
| MIT OR Apache-2.0 OR Zlib | glow, raw-window-handle, cursor-icon, xkeysym, zune-core, zune-jpeg, miniz_oxide, bytemuck |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | rustix, linux-raw-sys |
| Unlicense OR MIT | memchr, aho-corasick, byteorder-lite |
| BSD-3-Clause | ed25519-dalek, curve25519-dalek, subtle |
| ISC | rustls-webpki, untrusted, libloading |
| BSD-3-Clause OR Apache-2.0 | moxcms, pxfm |
| (MIT OR Apache-2.0) AND Unicode-3.0 | unicode-ident, unicode-properties |
| CDLA-Permissive-2.0 | webpki-roots 0.26 et 1.0 (certificats racines Mozilla embarqués) |
| Apache-2.0 AND ISC | ring 0.17 (code issu de BoringSSL sous Apache-2.0, code nouveau sous ISC ; Linux seulement) |
| Apache-2.0 OR ISC OR MIT | rustls |
| Apache-2.0 AND MIT | dpi (winit) |
| 0BSD ; 0BSD OR MIT OR Apache-2.0 | xz-native (décompression xz réécrite en Rust, sans code C) ; adler2 |
| BSD-2-Clause OR Apache-2.0 OR MIT | zerocopy |
| Zlib | foldhash |
| (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | epaint_default_fonts (polices d'egui, SecMet Clé USB seulement, voir §6.2) |
| **MPL-2.0** | **option-ext** 0.2 (via `dirs` → `dirs-sys`), dans tous les livrables |
| **Apache-2.0 OR GPL-2.0-only** | **self_cell** 1.3 (via `epaint`), SecMet Clé USB seulement : la branche Apache-2.0 est retenue |

Quand une crate offre un choix de licences, la branche MIT (ou Apache-2.0 pour `self_cell`) est retenue ; elle convient à la GPL-3.0 de SecMet.

Points particuliers :

- **Copyleft** : une seule crate, `option-ext`, est sous **MPL-2.0**, un copyleft limité au fichier. La MPL-2.0 est compatible avec la GPL-3.0 (§3.3) et n'impose rien au reste du programme ; la distribution du binaire oblige seulement à indiquer où obtenir la source de cette crate : https://crates.io/crates/option-ext (dépôt https://github.com/soc/option-ext). Aucune crate GPL, LGPL ou AGPL n'est compilée ; `self_cell` ne cite la GPL-2.0 qu'en alternative.
- **TLS** : `rustls` et `ring` sous Linux (aucun OpenSSL : ni `libssl` ni `libcrypto` dans les binaires) ; **Schannel** de Windows via `native-tls` et `schannel` pour le `.exe` (composant système de Windows, non redistribué).
- **Windows** : `windows-sys` et `windows` (bindings MIT OR Apache-2.0 ; les bibliothèques d'import `windows_x86_64_msvc` sont sous la même licence).
- **Apache-2.0** : aucune des crates distribuées ne publie de fichier `NOTICE` à reproduire.
- Les avis des crates BSD-3-Clause (dalek) interdisent d'utiliser le nom des auteurs pour promouvoir le produit.

Liste complète, par livrable et par cible (graphe réellement compilé) :

```bash
# Application et helper (Linux)
cargo tree --locked -e normal,no-proc-macro --prefix none --format '{p} {l}' \
  --workspace --exclude secmet-key-writer --target x86_64-unknown-linux-gnu \
  | sed 's/ (\*)$//' | grep -v "$PWD" | sort -u
# SecMet Clé USB : même commande avec -p secmet-key-writer, une fois avec
# --target x86_64-unknown-linux-gnu, une fois avec --target x86_64-pc-windows-msvc
```

`cargo license --all-features` ou `cargo metadata` sans filtre de cible ne conviennent pas : ils unifient les fonctionnalités et les cibles et font apparaître des crates qui ne sont pas compilées (`openssl`, `clipboard-win`). Pour produire un fichier d'avis complet (textes des licences), `cargo about generate` s'utilise de la même façon, par cible.

---

## 3. Bibliothèques système (liaison dynamique)

### 3.1 Paquets `.deb` et `.rpm`

`secmet` est lié dynamiquement à `libgtk-4`, `libgdk_pixbuf-2.0`, `libcairo`, `libgio-2.0`, `libgobject-2.0`, `libglib-2.0`, `libgcc_s`, `libm` et `libc` ; `secmet-helper` et `secmet-verify` ne lient que `libgcc_s` et `libc`. Ces bibliothèques sont fournies par la distribution de l'utilisateur (`Depends`) : SecMet ne les redistribue pas.

| Composant | Licence | Obligation distribution |
|---|---|---|
| **GTK 4** (`libgtk-4-1`) | **LGPL-2.1+** | Mentionner l'usage de GTK ; liaison dynamique, bibliothèque remplaçable |
| **GLib, GIO, GObject, Pango, GdkPixbuf, librsvg** | **LGPL-2.1+** (graphene : MIT) | Idem (chaîne GTK) |
| **Cairo** | **LGPL-2.1 OR MPL-1.1** | Idem |
| **PolicyKit** (`pkexec`, `polkitd`) | **LGPL-2.0+** | Programme système appelé en processus séparé, non redistribué |
| **glibc**, `libgcc_s` | LGPL-2.1+ ; GPL-3+ avec exception GCC Runtime Library | Jamais embarquées |

Texte LGPL-2.1 : https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html

### 3.2 AppImage SecMet

L'AppImage **embarque** sous `usr/lib` une soixantaine de bibliothèques copiées d'Ubuntu 24.04 par linuxdeploy et son greffon GTK. Elles sont liées dynamiquement (fichiers `.so` distincts, remplaçables après `--appimage-extract`) et non modifiées. Familles présentes :

| Famille | Bibliothèques | Licences |
|---|---|---|
| Chaîne GTK | gtk-4, glib/gio/gobject/gmodule, pango/pangocairo/pangoft2, cairo/cairo-gobject, gdk-pixbuf et ses chargeurs d'images, librsvg, graphene, epoxy, pixman, thai, datrie, graphite2, ffi, pcre2 | LGPL-2.1+ / LGPL-2+ ; Cairo LGPL-2.1 OR MPL-1.1 ; graphite2 LGPL-2.1+ OR GPL-2+ OR MPL-1.1 ; pixman, epoxy, graphene, ffi MIT ; pcre2 BSD-3-Clause ; librsvg intègre des crates Rust MIT/Apache-2.0/MPL-2.0 |
| Images et compression | png16, jpeg-turbo, tiff, webp, sharpyuv, Lerc, deflate, zstd, lzma, bz2, brotli | permissives (libpng, BSD, Apache-2.0, MIT, Zlib) |
| **GPL-2.0-or-later** | **libjbig** (requise par libtiff, chargée par le chargeur TIFF de gdk-pixbuf) ; **liblzo2** (requise par libcairo-script-interpreter) | **GPL-2+** |
| X11, Wayland, Vulkan | Xau, Xdmcp, Xext, Xfixes, Xi, Xinerama, Xrandr, Xrender, Xcursor, Xdamage, xcb-render/-shm, xkbcommon, wayland-egl, vulkan | MIT/X11 ; vulkan Apache-2.0 |
| Divers | icuuc/icudata, xml2, selinux, mount, blkid, bsd, md | ICU (Unicode) ; xml2 MIT ; selinux domaine public ; mount/blkid LGPL-2.1+ ; bsd/md BSD |
| TestDisk et ses bibliothèques privées | voir §5 | |

S'y ajoutent deux éléments redistribués dans chaque AppImage :

- le **runtime AppImage** (type2-runtime, en tête du fichier) : **MIT**, avec **libfuse (LGPL-2.1) lié statiquement**, squashfuse (BSD-2-Clause), zstd et zlib ; source : https://github.com/AppImage/type2-runtime ;
- le crochet `apprun-hooks/linuxdeploy-plugin-gtk.sh` du greffon GTK : **MIT**.

`linuxdeploy` lui-même sert à l'assemblage et n'est pas redistribué.

Les bibliothèques GPL-2+ `libjbig` et `liblzo2` sont compatibles avec la GPL-3.0 de SecMet. Elles ne servent pas à SecMet (le logo passe par la crate `image`) et seraient à exclure de l'assemblage si SecMet était un jour distribué sous une autre licence que la GPL.

La liste exacte dépend de la version d'Ubuntu au moment de la construction ; elle se lit dans l'AppImage livrée :

```bash
./SecMet_<v>_x86_64.AppImage --appimage-extract >/dev/null
ls squashfs-root/usr/lib squashfs-root/usr/lib/secmet/lib    # bibliothèques
ls squashfs-root/usr/share/doc                               # avis copyright des paquets Ubuntu
```

Chaque bibliothèque embarquée (`usr/lib/**/*.so*`, celles de TestDisk comprises) et `testdisk` sont accompagnés du fichier `usr/share/doc/<paquet>/copyright` de leur paquet Ubuntu d'origine : `packaging/bundle-tools.sh --copyrights` retrouve ce paquet (`dpkg -S`, avec et sans `/usr` à cause d'usrmerge) entre les deux passages de linuxdeploy, et la construction échoue s'il manque un paquet ou un copyright. La correspondance fichier → paquet est dans `usr/share/doc/secmet/bundled-libraries.txt`, et la fumée de `package.yml` et de `release.sh` (`packaging/appimage-smoke.sh`) la contrôle. Jusqu'à la 0.4.0, ces fichiers manquaient pour `liblzo2-2`, `libntfs-3g89t64`, `libuuid1`, `libcom-err2` et `testdisk`.

### 3.3 SecMet Clé USB, Linux (AppImage)

Le binaire ne lie que `libc`, `libm` et `libgcc_s`. X11, Wayland, EGL, OpenGL, xkbcommon et D-Bus sont chargés à l'exécution depuis le système hôte (`dlopen`) et **ne sont pas embarqués**. L'AppImage contient le binaire, l'icône, le fichier `LICENSE` de SecMet et le runtime AppImage décrit au §3.2.

### 3.4 SecMet Clé USB, Windows (`.exe`)

- L'exécutable publié est construit par la CI avec la chaîne **MSVC** (cible `x86_64-pc-windows-msvc`). Il dépend de `VCRUNTIME140.dll` et de l'UCRT, composants Microsoft présents sur Windows 10 et 11 et non embarqués.
- Il importe uniquement des DLL système de Windows (kernel32, user32, gdi32, advapi32, crypt32, secur32, ole32, shell32, setupapi, dxgi, d3d12…). Direct3D 12, WARP et Schannel sont des composants propriétaires de Windows, **non redistribués** : aucune contrainte de licence.
- Construction locale avec MinGW-w64 (`packaging/build_key_writer_exe.sh`) : les objets de démarrage MinGW (ZPL-2.1 et domaine public) et les parties statiques de libgcc (GPL-3+ **avec exception GCC Runtime Library 3.1**) sont liés au binaire ; l'exception autorise toute licence pour le programme. Le script vérifie qu'aucune DLL libgcc, libstdc++ ou libwinpthread n'est requise.
- La licence des outils Visual Studio Build Tools de l'image `windows-latest` de GitHub couvre ce projet libre ; pour une construction commerciale fermée, elle est **à valider**.

---

## 4. Programmes lancés en sous-processus

Depuis la v0.3.0, SecMet n'invoque **aucun outil externe** pour l'inventaire, le SMART, les commandes firmware ATA, NVMe et SCSI, l'écriture hôte, l'inventaire du banc (DMI via sysfs) ou le rendu PDF : `secmet-helper` passe par des ioctl natifs (hdparm, nvme-cli, smartmontools, sg3_utils et blkdiscard ne sont ni appelés ni embarqués). Les programmes suivants sont lancés comme processus séparés :

| Programme | Lancé par | Licence | Rôle |
|---|---|---|---|
| `testdisk` | `secmet-helper` | **GPL-2.0+** | Validation post-effacement optionnelle (lecture seule) ; embarqué, voir §5 |
| `pkexec` (polkit) | `secmet`, SecMet Clé USB (Linux) | LGPL-2.0+ | Élévation des droits (helper, installation d'une mise à jour, écriture de la clé) |
| `apt-get` / `dnf` (via `pkexec`) | `secmet` | GPL-2+ | Installation d'une mise à jour vérifiée |
| `systemctl` (systemd) | `secmet-device` | LGPL-2.1+ | Mise en veille pour dégeler un disque ATA |
| `which`, `timeout`, `sh` | `secmet` | GPL | Détection de programmes, délai, relance après mise à jour |
| `xdg-open`, gestionnaire de fichiers, lecteur PDF (Evince, Okular, Atril, Zathura…) | `secmet` | GPL-2.0+ pour la plupart | Ouverture des certificats et des dossiers |
| `lsblk` (util-linux), `notify-send` (libnotify) | SecMet Clé USB (Linux) | GPL-2+ ; LGPL-2.1+ | Liste des clés USB, notification de fin |

Ces programmes appartiennent au système de l'utilisateur et ne sont pas redistribués par SecMet, sauf TestDisk. L'appel en processus séparé, avec échange par la ligne de commande et les tubes, est couramment considéré comme un agrégat de programmes distincts (FAQ GNU, « mere aggregation ») : selon cette interprétation, il ne fait pas de SecMet une œuvre dérivée de ces outils.

---

## 5. Outils embarqués (`.deb`, `.rpm` et AppImage)

Lors de la construction, `packaging/bundle-tools.sh` copie `testdisk` et toutes les bibliothèques qu'il lie (sauf le cœur de la glibc) sous `/usr/lib/secmet/{bin,lib}/` (paquets `.deb` et `.rpm`) ou `usr/lib/secmet/` (AppImage, installés sur l'hôte au premier lancement). Les binaires sont copiés **sans modification**. La clé USB installe en plus le paquet Ubuntu `testdisk` (§7).

| Binaire embarqué | Licence | Origine |
|---|---|---|
| `testdisk` | **GPL-2.0+** (© Christophe Grenier, https://www.cgsecurity.org/) | Ubuntu 24.04 (`7.1-5+nmu1build2`) pour le `.deb` 24.04 et l'AppImage ; Ubuntu 22.04 pour le `.deb` 22.04 ; Fedora 44 et Fedora 42 pour les deux `.rpm` |
| `libncursesw`, `libtinfo` (ncurses) | **MIT/X11** | idem |
| `libuuid` (util-linux) | **BSD-3-Clause** | idem |
| `libcom_err` (e2fsprogs) | licence de type MIT (MIT SIPB) | idem |
| `libext2fs` (e2fsprogs) | **LGPL-2** | idem |
| `libntfs-3g` (ntfs-3g) | **GPL-2+** (certaines parties sous LGPL-2+ selon le copyright Debian ; GPL-2+ retenue pour l'ensemble) | idem |

Ce sont les six bibliothèques de la construction Ubuntu 24.04. La liste réelle de chaque paquet est celle que donne `ldd` sur la machine de construction ; elle se lit dans le paquet (`dpkg -c`, `rpm -qlp`).

Obligations GPL-2.0 pour TestDisk (et LGPL pour ses bibliothèques) :

- texte de la licence livré : fait (`/usr/share/doc/secmet/gpl-2.0.txt`) ;
- binaire non modifié : fait ;
- source correspondante : voir §8. Le renvoi actuel aux archives des distributions suffit pour une distribution non commerciale ; il ne suffit pas pour une distribution commerciale.

Détail installé avec le paquet : [packaging/bundled-tools-compliance.txt](../../packaging/bundled-tools-compliance.txt) (`/usr/share/doc/secmet/bundled-tools-compliance.txt`).

---

## 6. Assets graphiques et polices

### 6.1 Application, PDF, clé

| Fichier | Origine | Licence |
|---|---|---|
| `content/SecMet_icon.svg`, `content/SecMet_logo.svg`, `content/SecMet_logo_dark.svg`, `crates/secmet-attest/resources/secmet_logo.jpg` | Création SecMet | Nom et logo réservés (SecMet Contributors, tous droits réservés ; marque non déposée à ce jour) : reproductibles avec le logiciel non modifié ; une version modifiée doit être renommée et livrée sans eux |
| `packaging/live/overlay/usr/share/backgrounds/secmet.png`, `packaging/live/overlay/usr/share/plymouth/themes/secmet/watermark.png` | Création SecMet (fond d'écran et écran de démarrage de la clé) | Idem |
| Thème Plymouth « secmet » de la clé | images de la roue copiées du thème `spinner` de Plymouth par `build_rootfs.sh` | **GPL-2.0+** (Plymouth) ; source : paquet `plymouth` d'Ubuntu |
| `content/device-hdd.svg` | [Bootstrap Icons](https://icons.getbootstrap.com/) (`bi-device-hdd`) | **MIT** (© The Bootstrap Authors) |
| `resources/secmet.css` | SecMet | GPL-3.0-or-later |

Le logo personnalisé des certificats, s'il est configuré par l'exploitant, relève de ses propres droits.

Le PDF et l'application GTK n'embarquent aucune police. Le PDF utilise les polices standard PDF (Helvetica, Helvetica-Bold, Courier), fournies par le lecteur ; seules les largeurs de caractères de Helvetica (métriques AFM publiées par Adobe pour les 14 polices standard) sont reprises dans `secmet-attest::pdf` pour la mise en page.

Voir [content/LICENSE](../../content/LICENSE).

### 6.2 Polices embarquées dans SecMet Clé USB

SecMet Clé USB (AppImage et `.exe`) embarque les polices d'egui (crate `epaint_default_fonts`, avec la fonctionnalité `monochrome_emoji_fonts` pour les glyphes ✔ et ✖) :

| Police | Licence | Obligation |
|---|---|---|
| Ubuntu-Light | **Ubuntu Font Licence 1.0** | joindre le texte UFL ; la police reste sous UFL ; une police modifiée doit être renommée |
| Hack-Regular | **MIT** (Hack, © Source Foundry Authors) et **Bitstream Vera License** (glyphes Bitstream Vera, noms réservés « Bitstream » et « Vera ») ; part DejaVu dans le domaine public | joindre les deux avis |
| NotoEmoji-Regular | **SIL Open Font License 1.1** | joindre le texte OFL ; la police ne peut pas être vendue seule (la vente avec un logiciel est permise) |
| emoji-icon-font, egui-icons (sous-ensemble du précédent) | **MIT** (© John Slegers) | joindre l'avis |

Le champ SPDX de `epaint_default_fonts` (« (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 ») omet le MIT de Hack et d'emoji-icon-font ainsi que la licence Bitstream Vera : un générateur d'avis automatique les oublie. Les textes sont dans le dossier `fonts/` de la crate (`UFL.txt`, `OFL.txt`, `Hack-Regular.txt`, `emoji-icon-font-mit-license.txt`).

---

## 7. Image de la clé USB (`secmet-key_<v>_x86_64.img.xz`, `secmet-live_<v>_x86_64.tar`)

L'image de la clé et le livrable `.tar` (qui contient le noyau, l'initrd, le squashfs, les fichiers EFI et BIOS, le cas échéant le paquet SecMet plus récent que l'image et sa signature, `secmet-make-key.sh`, `grub.cfg` et le binaire `secmet-release`) redistribuent un **système Ubuntu 24.04 « noble » complet**, construit par `packaging/live/build_rootfs.sh` (debootstrap, composants `main` et `universe`), plus le `.deb` SecMet. Pour la licence, c'est un **agrégat** : chaque composant garde sa licence, et la licence de SecMet ne s'étend pas au système.

### 7.1 Composants notables

| Composant | Licence | Remarque |
|---|---|---|
| Noyau Linux (`linux-image-generic-hwe-24.04`) | **GPL-2.0-only** | module ZFS (CDDL) supprimé de l'image ; reste `v4l2loopback` (GPL-2) |
| GRUB (`grub-efi-amd64-signed`, `grub-pc-bin` : `grubx64.efi`, modules i386-pc, `core.img`) | **GPL-3.0+** | copiés sur la partition de démarrage, signés par Canonical (EFI) |
| shim (`shim-signed` : `shimx64.efi`, MokManager `mmx64.efi`) | licence de type BSD | signé par Microsoft ; MokManager permet à l'utilisateur d'enregistrer sa propre clé |
| live-boot | **GPL-3.0+** | **modifié** par `build_rootfs.sh` (`9990-toram-todisk.sh`, par `sed`) : le script public tient lieu de source de la modification |
| XFCE, LightDM, coreutils, bash, systemd, NetworkManager, udisks2, Zathura… | GPL-2+, GPL-3+, LGPL | paquets Ubuntu non modifiés |
| Ghostscript (`libgs10`), `jbig2dec`, `fonts-urw-base35` | **AGPL-3.0+** (polices URW : AGPL avec exception pour les polices) | présents en dépendance d'autres paquets, non modifiés, sans usage réseau |
| Microcodes `intel-microcode`, `amd64-microcode`, firmwares `linux-firmware` | **non libres**, redistribution autorisée en binaire non modifié | avis à reproduire ; pas d'usage promotionnel du nom des fabricants ; ingénierie inverse interdite. La clé n'est donc pas « 100 % libre » |
| Polices du système (`fonts-ubuntu`, `fonts-dejavu`, `fonts-noto-color-emoji`) | UFL-1.0, Bitstream Vera / domaine public, OFL-1.1 | |
| TestDisk (paquet Ubuntu) | GPL-2.0+ | |
| SecMet (`.deb`) et thème Plymouth « secmet » | voir §1 et §6.1 | |

Les avis de licence de chaque paquet sont conservés dans l'image (`/usr/share/doc/<paquet>/copyright`). Les fichiers de démarrage copiés sur la partition FAT (shim, GRUB) ne sont accompagnés d'aucun texte de licence sur cette partition.

### 7.2 Liste des paquets d'une version

La liste exacte des paquets (plus de 600) et de leurs versions change à chaque construction de l'image ; elle n'est pas reproduite ici. Elle se lit dans l'image publiée :

```bash
# sur une clé démarrée
dpkg-query -W -f '${Package} ${Version}\n'
# depuis le livrable .tar, sans démarrer
tar -xf secmet-live_<v>_x86_64.tar
unsquashfs -d rootfs secmet-live-<v>/dist/live/filesystem.squashfs var/lib/dpkg usr/share/doc
dpkg-query --admindir=rootfs/var/lib/dpkg -W -f '${Package} ${Version}\n'
```

Les licences de chaque paquet sont dans `rootfs/usr/share/doc/<paquet>/copyright`.

### 7.3 Marque Ubuntu

La politique de propriété intellectuelle de Canonical (https://canonical.com/legal/intellectual-property-policy, §3) demande qu'une redistribution **modifiée** d'Ubuntu soit approuvée par Canonical si elle est associée aux marques Ubuntu ; sinon, les marques doivent être retirées ou remplacées. La même politique précise qu'elle ne réduit pas les droits accordés par les licences libres des paquets. La clé SecMet est un système dérivé : elle ne doit **pas** être présentée ni vendue comme « Ubuntu ». Le fond d'écran et l'écran de démarrage sont déjà ceux de SecMet ; certains fichiers du système (`base-files`, `lsb-release`) affichent encore « Ubuntu ». La conduite à tenir (retrait des mentions restantes ou accord de Canonical) est **à valider**.

---

## 8. Sources correspondantes : état actuel et reste à faire

**Code SecMet.** Les sources de chaque version, scripts de construction compris (paquets, AppImage, image de clé), sont publiques sur https://github.com/hclcdev12/secmet, avec un tag par version (`vX.Y.Z`).

**Composants tiers, état actuel.** Pour TestDisk, ses bibliothèques, les bibliothèques embarquées dans l'AppImage et le système Ubuntu de la clé, SecMet **renvoie aux archives des distributions** (paquets source Ubuntu par `apt source`, Launchpad ; paquets source Fedora par `dnf download --source`, https://src.fedoraproject.org/) et au site amont de TestDisk. Pour les crates Rust, la source est sur https://crates.io (versions exactes dans `Cargo.lock`). Ce renvoi correspond à l'option GPL-2 §3(c), **réservée à la distribution non commerciale**, et les archives Ubuntu ne gardent pas indéfiniment les anciennes versions des paquets mis à jour.

**Reste à faire avant une distribution commerciale** (vente de clés, de bancs, de paquets) :

1. publier avec chaque release une **archive des sources correspondantes** des composants GPL, LGPL et AGPL redistribués (TestDisk et ses bibliothèques, bibliothèques de l'AppImage, paquets de l'image de clé, GRUB, shim, live-boot modifié), produite à partir de la liste exacte des paquets de la version ; **ou** joindre une **offre écrite, valable au moins 3 ans**, de fournir ces sources (GPL-2 §3(b) ; GPL-3 §6) ;
2. joindre aux livrables qui n'en ont pas un fichier d'avis tiers : le `.exe` et l'AppImage de SecMet Clé USB (aujourd'hui seulement `LICENSE` dans l'AppImage, rien à côté du `.exe`), le `.tar` et la partition de démarrage de la clé (textes GPL-3 de GRUB, avis shim) ;
3. publier avec chaque image de clé la liste de ses paquets (§7.2) ;
4. faire valider par un juriste le traitement de la marque Ubuntu (§7.3).

Aucun de ces points n'est réalisé en 0.4.0.

---

## 9. Inspirations et crédits (hors dépendances)

| Projet | Licence | Lien | Rôle pour SecMet |
|---|---|---|---|
| **QDiskInfo** | **GPL-3.0** | https://github.com/edisionnano/QDiskInfo | Inspiration UX de la présentation SMART ; module `disk_info` réécrit en Rust/GTK4 sur les données natives du helper. Aucun code QDiskInfo inclus. |

> Aucun code QDiskInfo n'est incorporé ; SecMet étant lui-même sous GPL-3.0-or-later, la question de l'œuvre dérivée ne se pose plus.

---

## 10. Commercialisation — synthèse

| Question | Réponse |
|---|---|
| SecMet peut-il être vendu ? | **Oui** (la GPL n'interdit pas la vente ; l'acheteur reçoit les sources et les mêmes droits) |
| Les sources sont-elles publiques ? | **Oui**, par choix (GPL-3.0-or-later, dépôt public) ; un concurrent peut les reprendre mais doit publier ses modifications sous GPL et renommer le produit |
| Les crates Rust bloquent-elles la vente ? | **Non** : licences permissives, plus `option-ext` en MPL-2.0 (copyleft par fichier, compatible GPL) ; avis à joindre |
| GTK / LGPL bloque-t-il la vente ? | **Non**, avec mentions LGPL et liaison dynamique (y compris dans l'AppImage) |
| TestDisk (GPL) embarqué bloque-t-il ? | **Non** (programme séparé, agrégat), à condition de livrer la licence (fait) et les sources correspondantes : l'offre actuelle ne suffit pas pour une vente (§8) |
| La clé USB (Ubuntu) peut-elle être vendue ? | **Oui**, comme agrégat, à condition de fournir les sources des paquets GPL/LGPL/AGPL (§8) et de respecter la politique de marque de Canonical (§7.3) |
| QDiskInfo (inspiration) oblige-t-il la GPL ? | Sans objet (réécriture indépendante, et SecMet est GPL) |
| Points d'attention principaux | **Offre de sources** (TestDisk, AppImage, clé) ; **avis tiers** absents à côté de SecMet Clé USB et sur la clé ; **marque Ubuntu** de la clé |

---

## 11. Fichiers de licence livrés

| Livrable | Chemin | Contenu |
|---|---|---|
| `.deb`, `.rpm` | `/usr/share/doc/secmet/LICENSE` | Licence GPL-3.0 SecMet (notice + texte intégral) |
| | `/usr/share/doc/secmet/content-LICENSE` | Nom et logo SecMet ; icône Bootstrap (MIT) |
| | `/usr/share/doc/secmet/THIRD_PARTY_LICENSES.md` | Ce document (copie de l'Annexe J) |
| | `/usr/share/doc/secmet/copyright` | Synthèse au format Debian (DEP-5), avec les textes des licences des crates et des bibliothèques de TestDisk |
| | `/usr/share/doc/secmet/bundled-tools-compliance.txt`, `gpl-2.0.txt` | Conformité de TestDisk embarqué |
| | `/usr/share/doc/secmet/USER_GUIDE.md`, `GUIDE_DEPLOIEMENT_BANC.md`, `Annexes/` | Annexes G, H et toutes les annexes |
| AppImage SecMet | `usr/share/doc/secmet/` | Les mêmes documents, sous les mêmes noms (dont `copyright`, `USER_GUIDE.md` et `GUIDE_DEPLOIEMENT_BANC.md`), plus `usr/share/doc/<paquet>/copyright` de chaque bibliothèque Ubuntu embarquée et de TestDisk (liste : `bundled-libraries.txt`) |
| `secmet-verify_<v>_x86_64`, `secmet-release_<v>_x86_64` | — | Aucun fichier (exécutable seul) ; licence et avis : `LICENSE` et cette annexe dans le dépôt |
| AppImage SecMet Clé USB | `usr/share/doc/secmet-key-writer/LICENSE` | Licence GPL-3.0 SecMet seulement |
| `.exe` SecMet Clé USB | — | Aucun fichier (exécutable seul) |
| Image de clé, `.tar` | `/usr/share/doc/*/copyright` dans le système ; `/usr/share/doc/secmet/` (paquet SecMet installé) | Avis des paquets Ubuntu ; aucun texte sur la partition de démarrage ni dans le `LISEZMOI.txt` du `.tar` |
