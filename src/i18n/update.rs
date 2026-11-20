//! Table de traduction `(fr, en, de)` — mises à jour (Réglages, pastille).

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    ("Mises à jour", "Updates", "Aktualisierungen"),
    (
        "Vérifier automatiquement les mises à jour",
        "Check for updates automatically",
        "Automatisch nach Updates suchen",
    ),
    (
        "Une requête HTTPS par jour vers le canal de publication, sans donnée personnelle.",
        "One HTTPS request per day to the release channel, no personal data.",
        "Eine HTTPS-Anfrage pro Tag an den Veröffentlichungskanal, keine personenbezogenen Daten.",
    ),
    (
        "Version installée : {version}",
        "Installed version: {version}",
        "Installierte Version: {version}",
    ),
    (
        "Nouvelle version disponible : {version} ({date})",
        "New version available: {version} ({date})",
        "Neue Version verfügbar: {version} ({date})",
    ),
    (
        "Vous utilisez la dernière version.",
        "You are running the latest version.",
        "Sie verwenden die neueste Version.",
    ),
    (
        "Dernière vérification : {date}",
        "Last check: {date}",
        "Letzte Prüfung: {date}",
    ),
    ("Jamais vérifié", "Never checked", "Noch nie geprüft"),
    ("Vérification en cours…", "Checking…", "Prüfung läuft…"),
    ("Vérifier maintenant", "Check now", "Jetzt prüfen"),
    (
        "Ouvrir la page de téléchargement",
        "Open the download page",
        "Download-Seite öffnen",
    ),
    (
        "Vérification impossible : {error}",
        "Check failed: {error}",
        "Prüfung fehlgeschlagen: {error}",
    ),
    (
        "Une mise à jour est disponible",
        "An update is available",
        "Ein Update ist verfügbar",
    ),
    (
        "latest.json illisible",
        "latest.json unreadable",
        "latest.json nicht lesbar",
    ),
    (
        "latest.json signé par une clé inconnue",
        "latest.json signed by an unknown key",
        "latest.json mit unbekanntem Schlüssel signiert",
    ),
    (
        "Impossible d'ouvrir le navigateur : {error}",
        "Could not open the browser: {error}",
        "Browser konnte nicht geöffnet werden: {error}",
    ),
    (
        "Vérification automatique désactivée",
        "Automatic check disabled",
        "Automatische Prüfung deaktiviert",
    ),
    (
        "Installer la mise à jour",
        "Install the update",
        "Update installieren",
    ),
    ("Relancer SecMet", "Restart SecMet", "SecMet neu starten"),
    (
        "Téléchargement de la version {version}…",
        "Downloading version {version}…",
        "Version {version} wird heruntergeladen…",
    ),
    (
        "Version {version} installée. Relancez SecMet pour l'utiliser.",
        "Version {version} installed. Restart SecMet to use it.",
        "Version {version} installiert. Starten Sie SecMet neu, um sie zu verwenden.",
    ),
    (
        "Mise à jour impossible : {error}",
        "Update failed: {error}",
        "Aktualisierung fehlgeschlagen: {error}",
    ),
    (
        "Installation impossible pour l'instant : {what}.",
        "Installation not possible right now: {what}.",
        "Installation derzeit nicht möglich: {what}.",
    ),
    (
        "effacement en cours",
        "erasure in progress",
        "Löschung läuft",
    ),
    (
        "analyse des disques en cours",
        "disk analysis in progress",
        "Analyse der Datenträger läuft",
    ),
    (
        "TestDisk en cours",
        "TestDisk in progress",
        "TestDisk läuft",
    ),
    (
        "chargement de la liste des disques en cours",
        "loading the disk list",
        "Liste der Datenträger wird geladen",
    ),
    (
        "génération d'un certificat PDF en cours",
        "generating a PDF certificate",
        "PDF-Zertifikat wird erstellt",
    ),
    (
        "clôture du lot en cours",
        "closing the batch",
        "Charge wird abgeschlossen",
    ),
    (
        "déverrouillage d'un disque en cours",
        "unlocking a disk",
        "Datenträger wird entsperrt",
    ),
    (
        "sortie de l'état gelé d'un disque en cours",
        "taking a disk out of the frozen state",
        "Datenträger wird aus dem eingefrorenen Zustand geholt",
    ),
    (
        "Lot {id} ouvert : clôturez-le avant d'installer la mise à jour (un lot n'est traité que par une seule version de SecMet).",
        "Batch {id} is open: close it before installing the update (a batch is processed by a single version of SecMet).",
        "Charge {id} ist offen: Schließen Sie sie vor der Installation des Updates ab (eine Charge wird nur von einer einzigen SecMet-Version bearbeitet).",
    ),
    (
        "Clôturer le lot…",
        "Close the batch…",
        "Charge abschließen…",
    ),
    (
        "AppImage : téléchargez la nouvelle AppImage et remplacez le fichier ; le composant système sera mis à jour au lancement suivant.",
        "AppImage: download the new AppImage and replace the file; the system component will be updated at the next launch.",
        "AppImage: Laden Sie das neue AppImage herunter und ersetzen Sie die Datei; die Systemkomponente wird beim nächsten Start aktualisiert.",
    ),
];
