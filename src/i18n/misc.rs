//! Table de traduction `(fr, en, de)` — modèle disque, fenêtre Infos disque.

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    // ── src/disk.rs ──
    (
        "Ce disque est le disque système",
        "This disk is the system disk",
        "Dieser Datenträger ist der Systemdatenträger",
    ),
    (
        "ATA Security LOCKED (mot de passe requis)",
        "ATA Security LOCKED (password required)",
        "ATA Security LOCKED (Passwort erforderlich)",
    ),
    ("monté", "mounted", "eingehängt"),
    (
        "Ce disque est utilisé par le système ({detail})",
        "This disk is in use by the system ({detail})",
        "Dieser Datenträger wird vom System verwendet ({detail})",
    ),
    ("Disque en lecture seule", "Read-only disk", "Schreibgeschützter Datenträger"),
    (
        "Type de disque inconnu (inspection impossible).\nRebranchez le disque puis Rafraîchir.",
        "Unknown disk type (inspection impossible).\nReconnect the disk, then Refresh.",
        "Unbekannter Datenträgertyp (Prüfung nicht möglich).\nDatenträger neu anschließen, dann Aktualisieren.",
    ),
    (
        "Refusé par le moteur : {reasons}",
        "Refused by the engine: {reasons}",
        "Vom Entscheidungsmodul abgelehnt: {reasons}",
    ),
    (
        "Bloqué : {reasons} — consigne : {guidance}",
        "Blocked: {reasons} — guidance: {guidance}",
        "Blockiert: {reasons} — Hinweis: {guidance}",
    ),
    (
        "USB : écriture hôte (Clear), pas de commande firmware",
        "USB: host overwrite (Clear), no firmware command",
        "USB: Host-Überschreibung (Clear), kein Firmware-Befehl",
    ),
    (", {n} repli(s)", ", {n} fallback(s)", ", {n} Ausweichmethode(n)"),
    (
        " — dégradé {from} → {to}",
        " — downgraded {from} → {to}",
        " — herabgestuft {from} → {to}",
    ),
    (
        "méthode décidée au lancement",
        "method decided at launch",
        "Methode wird beim Start festgelegt",
    ),
    (
        "Session administrateur absente.\nRelancez SecMet et validez le mot de passe au démarrage.",
        "No administrator session.\nRestart SecMet and enter the password at startup.",
        "Keine Administratorsitzung.\nSecMet neu starten und beim Start das Passwort eingeben.",
    ),
    // ── src/disk_info/ui/mod.rs ──
    ("Infos disque", "Disk info", "Datenträgerinfo"),
    (
        "Disque verrouillé (ATA Security LOCKED).\nAnalyse SMART non lancée : les lectures timeout / échouent sans le mot de passe.\nEffacement impossible tant que le mot de passe utilisateur ou maître n'est pas connu.\nSi vous le connaissez : bouton « Déverrouiller » en bas de la fenêtre.",
        "Disk locked (ATA Security LOCKED).\nSMART analysis not started: reads time out or fail without the password.\nErasure is impossible until the user or master password is known.\nIf you know it: use the “Unlock” button at the bottom of the window.",
        "Datenträger gesperrt (ATA Security LOCKED).\nSMART-Analyse nicht gestartet: Lesezugriffe laufen ohne Passwort ins Timeout oder schlagen fehl.\nLöschung unmöglich, solange das Benutzer- oder Master-Passwort unbekannt ist.\nFalls bekannt: Schaltfläche „Entsperren“ unten im Fenster.",
    ),
    ("Lecture SMART en cours…", "Reading SMART…", "SMART wird gelesen…"),
    ("Erreur SMART: {error}", "SMART error: {error}", "SMART-Fehler: {error}"),
    ("S.M.A.R.T. Actif", "S.M.A.R.T. Active", "S.M.A.R.T. Aktiv"),
    ("S.M.A.R.T. Alerte", "S.M.A.R.T. Warning", "S.M.A.R.T. Warnung"),
    ("S.M.A.R.T. Indisponible", "S.M.A.R.T. Unavailable", "S.M.A.R.T. Nicht verfügbar"),
    ("SecMet — Infos disque", "SecMet — Disk info", "SecMet — Datenträgerinfo"),
    // ── src/disk_info/ui/overview.rs ──
    ("État de santé", "Health status", "Gesundheitszustand"),
    ("Température", "Temperature", "Temperatur"),
    ("Informations", "Information", "Informationen"),
    ("Lectures", "Reads", "Lesevorgänge"),
    ("Écritures", "Writes", "Schreibvorgänge"),
    ("Heures", "Hours", "Stunden"),
    ("Démarrages", "Power cycles", "Einschaltzyklen"),
    ("Réalloués", "Reallocated", "Neu zugewiesen"),
    ("Pending", "Pending", "Ausstehend"),
    // ── src/disk_info/ui/identity.rs ──
    ("Disque inconnu", "Unknown disk", "Unbekannter Datenträger"),
    ("Nœud", "Node", "Knoten"),
    ("Interface", "Interface", "Schnittstelle"),
    ("Capacité", "Capacity", "Kapazität"),
    ("Firmware", "Firmware", "Firmware"),
    ("Série", "Serial", "Seriennummer"),
    ("Type", "Type", "Typ"),
    // ── src/disk_info/ui/attributes.rs ──
    (
        "Aucun attribut SMART disponible pour ce protocole.",
        "No SMART attribute available for this protocol.",
        "Für dieses Protokoll sind keine SMART-Attribute verfügbar.",
    ),
    ("ID", "ID", "ID"),
    ("Attribute Name", "Attribute Name", "Attributname"),
    ("Current", "Current", "Aktuell"),
    ("Worst", "Worst", "Schlechtester"),
    ("Thresh", "Thresh", "Schwelle"),
    ("Raw Values", "Raw Values", "Rohwerte"),
    ("État", "Status", "Status"),
    ("Op", "Op", "Op"),
    ("Corrigées", "Corrected", "Korrigiert"),
    ("Invocations", "Invocations", "Aufrufe"),
    ("Go traités", "GB processed", "GB verarbeitet"),
    ("Erreurs non corr.", "Uncorrected errors", "Unkorrigierte Fehler"),
    // État de santé SMART (smart_models::HealthStatus).
    ("Excellent", "Excellent", "Ausgezeichnet"),
    ("Bon", "Good", "Gut"),
    ("Attention", "Caution", "Achtung"),
    ("Mauvais", "Bad", "Schlecht"),
    ("Inconnu", "Unknown", "Unbekannt"),
    ("Indice", "Index", "Index"),
];
