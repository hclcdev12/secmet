# Clé USB SecMet — image et fabrication

Mise en œuvre de l'[Annexe M](../../docs/Annexes/Annexe_M_cle_live_lots.md) : une clé USB qui démarre un laptop sur un système Linux minimal, ouvre SecMet directement sur le bureau, et conserve lots, certificats et clé de signature sur sa propre partition.

## Ce que voit l'opérateur

1. Il branche la clé, démarre le laptop dessus (menu de démarrage UEFI ou BIOS).
2. Le bureau s'ouvre sans écran de connexion ; SecMet est déjà lancée, maximisée.
3. SecMet propose de continuer ou d'ouvrir un lot, puis affiche le disque interne.
4. Après l'effacement, le certificat est enregistré sur la clé ; l'opérateur éteint le laptop depuis le menu du bureau et passe au suivant.
5. À la clôture du lot, le certificat groupé et le manifeste signé sont copiés sur la partition d'export, lisible sous Windows.

Un seul mot de passe à connaître : celui des opérations sur disque (compte `secmet`, défini à la construction de l'image). Le reste (montage des partitions, réseau, extinction) ne demande rien.

## Construction de l'image

```bash
make live                      # → dist/live/{vmlinuz,initrd.img,filesystem.squashfs,efi/,bios/,VERSION}
SECMET_OPERATOR_PASSWORD='…' make live   # mot de passe opérateur (défaut : secmet)
```

`build_live.sh` construit l'image `secmet-live-builder` et lance `build_rootfs.sh` dans un conteneur **privilégié** (podman ou docker) : debootstrap Ubuntu 24.04 minimal, noyau HWE, `live-boot`, XFCE dépouillé (session, panneau, bureau, Thunar, xterm, Zathura, NetworkManager ; ni Atril ni xfce4-terminal, qui tiraient WebKit, Ghostscript, MathJax et Perl, soit 400 Mo installés), agent PolicyKit `policykit-1-gnome` (invite du mot de passe du helper), paquets de langue français (GTK, bureau), LightDM en connexion automatique, SecMet installé depuis le `.deb` construit à partir du dépôt courant, opérateur `secmet` (uid 1000, groupe `secmet`), locale et clavier français, fichiers de `overlay/`. Sortie : squashfs (xz), noyau, initrd, binaires UEFI signés par Canonical (shim, grub, MokManager : Secure Boot) et modules GRUB BIOS.

Durée : 15 à 25 minutes selon le réseau (paquets Ubuntu) ; le registre cargo est conservé dans un volume.

## Fabrication d'une clé

```bash
make live-key DEV=/dev/sdX     # root ; EFFACE la clé
sudo packaging/live/secmet-make-key.sh /dev/sdX [--data-size 4G]
sudo packaging/live/secmet-make-key.sh /dev/sdX --system-only   # mise à jour de l'image, données conservées
```

| Partition | Type GPT | Format | Contenu |
|-----------|----------|--------|---------|
| 1 `SECMET-SYS` (6 Gio) | EFI System | FAT32 | `live/` (noyau, initrd, squashfs), `EFI/BOOT` (shim → grub signé), `EFI/ubuntu/grub.cfg`, `boot/grub` (BIOS) |
| 2 `SECMET-DATA` (4 Gio par défaut) | Linux | ext4 | `config/` (réglages, **clé de signature**), `logs/`, `lots/`, `archive/` (opérateur) ; `vault/` (root, coffre ATA) |
| 3 `SECMET-EXPORT` | Microsoft basic data | exFAT `SECMET-EXP` | Livrables des lots clôturés |
| 4 `BIOS boot` (1 Mio, placée à 1 Mio, avant la partition 1) | BIOS boot | — | Code de GRUB i386-pc pour les postes sans UEFI |

Sur une machine qui n'a pas le dépôt (banc, atelier), il suffit de copier `dist/live/`, `packaging/live/secmet-make-key.sh`, `packaging/live/grub.cfg` et un `secmet-release` compilé pour Ubuntu 24.04 (`target-appimage/release/secmet-release`) dans une arborescence `<dossier>/{dist/live,packaging/live,target/release}` et de lancer `sudo <dossier>/packaging/live/secmet-make-key.sh /dev/sdX`. La partition d'export d'une clé existante peut servir de dossier de transfert : en `--system-only`, seule la partition système est démontée.

Le script crée la clé de signature des certificats (`config/attestation-key.hex`, même format que celle créée par l'application) et affiche son **`key_id`** : à enregistrer chez le client comme clé de confiance (`secmet-verify --trust`). `--system-only` réécrit la partition 1 seulement.

## Essai en machine virtuelle

```bash
make live-image                 # dist/live/secmet-key.img : clé virtuelle (conteneur privilégié)
make live-test                  # démarre la clé dans QEMU (UEFI OVMF), fenêtre GTK
packaging/live/qemu-test.sh --headless     # sans fenêtre : VNC :9 et moniteur dist/live/qemu/mon.sock
packaging/live/qemu-ctl.py screendump /tmp/ecran.png   # capture ; sendkey, type, click aussi
SECMET_QEMU_SECBOOT=1 make live-test       # firmware OVMF Secure Boot
SECMET_QEMU_MACHINE=pc make live-test      # type de machine i440fx (défaut q35)
make test-live-scripts                      # script de mise à jour au démarrage, avec des doublures (dans make test et en CI)
make live-check                             # contrôle QEMU sans écran : le magasin app/ est déployé dans l'ordre (5 min)
SECMET_KERNEL_EXTRA="console=ttyS0,115200n8 console=tty0" make live-image   # clé virtuelle avec console série
packaging/live/qemu-ctl.py serial "" secmet secmet "journalctl -b | tail"    # shell série (connexion secmet / mot de passe)
```

La console série (`SECMET_KERNEL_EXTRA`) ne concerne que la clé virtuelle : `secmet-make-key.sh` copie `grub.cfg` tel quel.

**Mise à jour au démarrage, deux niveaux de test.** `packaging/live/tests/app-update.sh` exécute le script `secmet-app-update` de l'overlay sur un magasin factice avec des doublures de `mountpoint`, `dpkg-query`, `dpkg`, `secmet-release`, `dpkg-deb` et `logger` : la version la plus récente est choisie (0.3.10 devant 0.3.9, régression du 2026-11-06 où un tri ascendant redéployait la plus ancienne), repli sur signature invalide ou installation échouée, rien à faire quand l'image est à jour. Il tourne dans `make test` et dans `ci.yml`. `packaging/live/qemu-check.sh` (`make live-check`) va jusqu'au bout : un magasin de deux versions factices (9.9.8 et 9.9.9, le même `.deb` de `dist/` renommé) signé avec la clé de version si elle est disponible, une clé virtuelle fabriquée avec ce magasin, un démarrage sans écran, puis le journal est lu depuis l'image (partition de données extraite, `e2fsck`, `debugfs`) et doit montrer 9.9.9 vérifiée et déployée sans que 9.9.8 soit examinée ; sans la clé de version, il vérifie que 9.9.9 puis 9.9.8 sont refusées et que rien n'est déployé. À lancer après `make live`, avant une release.

Limite : la **veille (S3) ne se teste pas dans cette VM** : après `systemctl suspend` puis `system_wakeup`, l'invité ne reprend pas (OVMF, q35 comme pc). Le cycle Dé-frozen (veille, réveil, écran rallumé) se valide sur un portable réel.

La VM a un disque SATA de 2 Gio à effacer (`QEMU LAPTOP SSD`, série `LAPTOPDISK001`) et une identité DMI de test (`Atelier Test / Laptop QEMU / QEMU-LAPTOP-001`). Les disques virtuels ne connaissent pas ATA Security : l'effacement se fait par écriture hôte (Clear).

## Fichiers de `overlay/`

| Fichier | Rôle |
|---------|------|
| `etc/fstab` | Montage par étiquette de `SECMET-DATA` et `SECMET-EXP`, sans bloquer le démarrage |
| `etc/lightdm/lightdm.conf.d/50-secmet.conf` | Connexion automatique de `secmet`, session XFCE |
| `etc/xdg/xfce4/panel/default.xml`, `etc/xdg/xfce4/panel/whiskermenu-1.rc` | Une seule barre en haut, sombre et translucide, masquée en permanence (elle sort quand la souris touche le bord haut) : menu Whisker (recherche, favoris SecMet, Thunar, xterm, Zathura), fenêtres, zone de notification, énergie, heure, extinction. Pas de dock en bas qui recouvrirait les boutons de SecMet |
| `etc/xdg/autostart/secmet.desktop` | SecMet au démarrage de la session |
| `usr/local/bin/secmet-live` | Lance `secmet` avec `SECMET_DATA_DIR=/media/secmet-data` ; avertit si la partition manque |
| (image) `policykit-1-gnome` | Agent PolicyKit de la session : sans lui, `pkexec` n'a personne à qui demander le mot de passe et SecMet refuse de démarrer |
| `etc/polkit-1/rules.d/49-secmet-live.rules` | Montages amovibles, réseau et extinction sans mot de passe pour l'opérateur ; les opérations sur disque gardent le leur |
| `etc/xdg/xfce4/xfconf/xfce-perchannel-xml/xsettings.xml`, `xfwm4.xml`, `xfce4-desktop.xml`, `etc/gtk-3.0/settings.ini`, `etc/gtk-4.0/settings.ini`, `usr/share/backgrounds/secmet.png` | Bureau sombre et plat : Arc-Dark (GTK 3 et décorations xfwm4, ombres), icônes elementary-xfce-dark, police Ubuntu, fond d'écran SecMet, pas d'icônes système sur le bureau. SecMet (GTK 4) garde son thème par défaut sombre (`GTK_THEME=Default:dark` dans `secmet-live`). Paquets ajoutés : arc-theme, elementary-xfce-icon-theme, fonts-ubuntu, xfce4-whiskermenu-plugin, plymouth-theme-spinner (≈ 30 Mo avant compression). Écran de démarrage : thème Plymouth `secmet` (`usr/share/plymouth/themes/secmet/watermark.png` : lion et mention SecMet au centre ; roue et réglages repris du spinner à la construction, fond sombre, thème sélectionné par `plymouthd.conf` et l'alternative `default.plymouth`) |
| `etc/xdg/xfce4/xfconf/xfce-perchannel-xml/xfce4-power-manager.xml`, `etc/systemd/logind.conf.d/50-secmet-live.conf` | Rien n'interrompt un effacement : pas d'extinction d'écran ni de veille d'inactivité, capot fermé ignoré, pas de verrouillage au réveil |
| `etc/zathurarc` | Lecteur PDF Zathura : page ajustée à la fenêtre, presse-papiers système, titre = nom du fichier |
| `etc/initramfs-tools/hooks/zz-secmet-trim` | Initrd sans le pilote nouveau ni le firmware NVIDIA GSP (103 Mo) : environ 85 Mo au lieu de 190, lus par GRUB depuis la clé avant tout affichage ; le système complet les a |
| `etc/systemd/system.conf.d/50-secmet-live.conf` | Extinction : un service qui ne s'arrête pas est forcé après 15 s (système en mémoire, rien à préserver), au lieu des 90 s par défaut |
| `etc/sysctl.d/50-secmet-live.conf` | Écritures vers la clé au fil de l'eau (`vm.dirty_*` bas) : veille et extinction sans longue synchronisation |
| `usr/local/bin/secmet-app-update`, `etc/systemd/system/secmet-app-update.service` | Au démarrage, installe la version de SecMet conservée dans `app/<version>/` de la partition de données par le bouton « Installer la mise à jour », après vérification de sa signature avec la clé de version (Annexe N §N.6) |
| `usr/lib/systemd/system-sleep/secmet-display` | Avant la veille, copie le journal sur la clé ; au réveil, rallume l'écran de la session (`xset dpms force on`, `xrandr --auto`) |
| `etc/systemd/journald.conf.d/50-secmet-live.conf`, `usr/local/bin/secmet-journal-save`, `etc/systemd/system/secmet-journal-{sync.service,sync.timer,final.service}` | Journal système en mémoire, recopié en texte dans `logs/system/<boot-id>.log` de la partition de données toutes les minutes, avant la veille et à l'extinction (un journal persistant écrit directement sur la clé bloquait journald 40 s à l'extinction) |

## Démarrage

`grub.cfg` cherche la partition `SECMET-SYS` et démarre `live-boot` (`boot=live live-media-path=/live`). Entrée par défaut : la racine reste sur la clé, le squashfs est monté en loop et lu à la demande (chaque bloc décompressé une fois, puis gardé dans le cache de pages), un overlay en RAM reçoit les écritures ; c'est le démarrage le plus court, et la clé doit rester branchée toute la session. Derrière l'écran de démarrage il reste environ 5 s muettes : la recherche et le montage de la clé par live-boot. La veille du dé-frozen fonctionne avec la racine sur la clé (vérifié le 2026-11-05 sur HP ProBook 455 G3) ; le gel au réveil du 2026-11-02 venait de `nomodeset`, pas de la clé. Deuxième entrée (Échap) « copie en mémoire » : `toram=filesystem.squashfs` copie le squashfs (≈ 725 Mo) en RAM avant de le monter, la clé peut ensuite être retirée ; compter 4 Go de RAM et 25 à 40 s de plus derrière l'écran de démarrage (message « Copie du système en mémoire » sous le logo), soit 894 Mo à 24 Mo/s mesurés le 2026-11-05 dans le journal entre la détection de la clé et le montage de `loop0`. `toram=filesystem.squashfs` plutôt que `toram` seul, qui copiait toute la partition, noyau et initrd compris. Ne jamais mettre `nomodeset` (ni `nomodeset=0` : le noyau ignore la valeur) sur les entrées principales : cela désactive tous les pilotes graphiques et l'écran reste noir après la veille du dé-frozen (vu sur HP ProBook 455 G3, 2026-11-02). Menu caché (2 s, Échap pour l'afficher) avec deux entrées de secours : `nomodeset` (carte graphique récalcitrante) et console de dépannage.

## Limites connues

- Postes sans démarrage USB, BIOS verrouillé, Intel RST « RAID On » : voir Annexe M §M.9.
- Le système lui-même (noyau, bureau) ne se met pas à jour sur la clé : on reconstruit l'image et on réécrit la partition système (`--system-only`). SecMet, elle, se met à jour par le bouton des Réglages : le paquet est conservé sur la partition de données et réinstallé à chaque démarrage (Annexe N §N.6).
- Le bouton « Éteindre ce PC » dans SecMet n'existe pas : l'extinction se fait par le menu du bureau.
