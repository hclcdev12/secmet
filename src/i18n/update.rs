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
];
