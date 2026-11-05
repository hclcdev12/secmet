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

`build_live.sh` construit l'image `secmet-live-builder` et lance `build_rootfs.sh` dans un conteneur **privilégié** (podman ou docker) : debootstrap Ubuntu 24.04 minimal, noyau HWE, `live-boot`, XFCE dépouillé (session, panneau, bureau, Thunar, terminal, Atril, NetworkManager), agent PolicyKit `policykit-1-gnome` (invite du mot de passe du helper), paquets de langue français (GTK, bureau), LightDM en connexion automatique, SecMet installé depuis le `.deb` construit à partir du dépôt courant, opérateur `secmet` (uid 1000, groupe `secmet`), locale et clavier français, fichiers de `overlay/`. Sortie : squashfs (xz), noyau, initrd, binaires UEFI signés par Canonical (shim, grub, MokManager : Secure Boot) et modules GRUB BIOS.

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
SECMET_KERNEL_EXTRA="console=ttyS0,115200n8 console=tty0" make live-image   # clé virtuelle avec console série
packaging/live/qemu-ctl.py serial "" secmet secmet "journalctl -b | tail"    # shell série (connexion secmet / mot de passe)
```

La console série (`SECMET_KERNEL_EXTRA`) ne concerne que la clé virtuelle : `secmet-make-key.sh` copie `grub.cfg` tel quel.

Limite : la **veille (S3) ne se teste pas dans cette VM** : après `systemctl suspend` puis `system_wakeup`, l'invité ne reprend pas (OVMF, q35 comme pc). Le cycle Dé-frozen (veille, réveil, écran rallumé) se valide sur un portable réel.

La VM a un disque SATA de 2 Gio à effacer (`QEMU LAPTOP SSD`, série `LAPTOPDISK001`) et une identité DMI de test (`Atelier Test / Laptop QEMU / QEMU-LAPTOP-001`). Les disques virtuels ne connaissent pas ATA Security : l'effacement se fait par écriture hôte (Clear).

## Fichiers de `overlay/`

| Fichier | Rôle |
|---------|------|
| `etc/fstab` | Montage par étiquette de `SECMET-DATA` et `SECMET-EXP`, sans bloquer le démarrage |
| `etc/lightdm/lightdm.conf.d/50-secmet.conf` | Connexion automatique de `secmet`, session XFCE |
| `etc/xdg/xfce4/panel/default.xml`, `etc/xdg/xfce4/panel/whiskermenu-1.rc` | Une seule barre en haut, sombre et translucide, masquée en permanence (elle sort quand la souris touche le bord haut) : menu Whisker (recherche, favoris SecMet, Thunar, terminal, Atril), fenêtres, zone de notification, énergie, heure, extinction. Pas de dock en bas qui recouvrirait les boutons de SecMet |
| `etc/xdg/autostart/secmet.desktop` | SecMet au démarrage de la session |
| `usr/local/bin/secmet-live` | Lance `secmet` avec `SECMET_DATA_DIR=/media/secmet-data` ; avertit si la partition manque |
| (image) `policykit-1-gnome` | Agent PolicyKit de la session : sans lui, `pkexec` n'a personne à qui demander le mot de passe et SecMet refuse de démarrer |
| `etc/polkit-1/rules.d/49-secmet-live.rules` | Montages amovibles, réseau et extinction sans mot de passe pour l'opérateur ; les opérations sur disque gardent le leur |
| `etc/xdg/xfce4/xfconf/xfce-perchannel-xml/xsettings.xml`, `xfwm4.xml`, `xfce4-desktop.xml`, `etc/gtk-3.0/settings.ini`, `etc/gtk-4.0/settings.ini`, `usr/share/backgrounds/secmet.png` | Bureau sombre et plat : Arc-Dark (GTK 3 et décorations xfwm4, ombres), icônes elementary-xfce-dark, police Ubuntu, fond d'écran SecMet, pas d'icônes système sur le bureau. SecMet (GTK 4) garde son thème par défaut sombre (`GTK_THEME=Default:dark` dans `secmet-live`). Paquets ajoutés : arc-theme, elementary-xfce-icon-theme, fonts-ubuntu, xfce4-whiskermenu-plugin, plymouth-theme-spinner (≈ 30 Mo avant compression). Écran de démarrage : thème Plymouth `secmet` (`usr/share/plymouth/themes/secmet/watermark.png` : lion et mention SecMet au centre ; roue et réglages repris du spinner à la construction, fond sombre, thème sélectionné par `plymouthd.conf` et l'alternative `default.plymouth`) |
| `etc/xdg/xfce4/xfconf/xfce-perchannel-xml/xfce4-power-manager.xml`, `etc/systemd/logind.conf.d/50-secmet-live.conf` | Rien n'interrompt un effacement : pas d'extinction d'écran ni de veille d'inactivité, capot fermé ignoré, pas de verrouillage au réveil |
| `etc/systemd/system.conf.d/50-secmet-live.conf` | Extinction : un service qui ne s'arrête pas est forcé après 15 s (système en mémoire, rien à préserver), au lieu des 90 s par défaut |
| `etc/sysctl.d/50-secmet-live.conf` | Écritures vers la clé au fil de l'eau (`vm.dirty_*` bas) : veille et extinction sans longue synchronisation |
| `usr/local/bin/secmet-app-update`, `etc/systemd/system/secmet-app-update.service` | Au démarrage, installe la version de SecMet conservée dans `app/<version>/` de la partition de données par le bouton « Installer la mise à jour », après vérification de sa signature avec la clé de version (Annexe N §N.6) |
| `usr/lib/systemd/system-sleep/secmet-display` | Avant la veille, copie le journal sur la clé ; au réveil, rallume l'écran de la session (`xset dpms force on`, `xrandr --auto`) |
| `etc/systemd/journald.conf.d/50-secmet-live.conf`, `usr/local/bin/secmet-journal-save`, `etc/systemd/system/secmet-journal-{sync.service,sync.timer,final.service}` | Journal système en mémoire, recopié en texte dans `logs/system/<boot-id>.log` de la partition de données toutes les minutes, avant la veille et à l'extinction (un journal persistant écrit directement sur la clé bloquait journald 40 s à l'extinction) |

## Démarrage

`grub.cfg` cherche la partition `SECMET-SYS` et démarre `live-boot` (`boot=live live-media-path=/live toram`). `toram` copie le système en mémoire au démarrage (1,3 Go, 4 Go de RAM conseillés) : la clé ne porte plus la racine, et la veille du dé-frozen, qui coupe le contrôleur USB, ne fige plus le poste au réveil. Une entrée sans `toram` reste pour les postes à moins de 4 Go. Ne jamais mettre `nomodeset` (ni `nomodeset=0` : le noyau ignore la valeur) sur l'entrée principale : cela désactive tous les pilotes graphiques et l'écran reste noir après la veille du dé-frozen (vu sur HP ProBook 455 G3, 2026-11-02). Menu caché (2 s, Échap pour l'afficher) avec deux entrées de secours : `nomodeset` (carte graphique récalcitrante) et console de dépannage.

## Limites connues

- Postes sans démarrage USB, BIOS verrouillé, Intel RST « RAID On » : voir Annexe M §M.9.
- Le système lui-même (noyau, bureau) ne se met pas à jour sur la clé : on reconstruit l'image et on réécrit la partition système (`--system-only`). SecMet, elle, se met à jour par le bouton des Réglages : le paquet est conservé sur la partition de données et réinstallé à chaque démarrage (Annexe N §N.6).
- Le bouton « Éteindre ce PC » dans SecMet n'existe pas : l'extinction se fait par le menu du bureau.
