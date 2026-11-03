//! Table de traduction `(fr, en, de)` — partie `ui_a` : fenêtre principale,
//! liste des disques, dialogues (dé-frozen, déverrouillage, confirmation),
//! autorisation au démarrage, enrichissement.

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    // ── auth.rs ──
    (
        "Composant système requis",
        "System component required",
        "Systemkomponente erforderlich",
    ),
    (
        "SecMet doit installer son composant système sur cette machine :\nhelper privilégié, policy PolicyKit et outils disque embarqués.\n\nMotif : {detail}\n\nInstaller maintenant ?\n(un mot de passe administrateur sera demandé)",
        "SecMet must install its system component on this machine:\nprivileged helper, PolicyKit policy and bundled disk tools.\n\nReason: {detail}\n\nInstall now?\n(an administrator password will be requested)",
        "SecMet muss seine Systemkomponente auf diesem Rechner installieren:\nprivilegierter Helper, PolicyKit-Richtlinie und mitgelieferte Datenträger-Werkzeuge.\n\nGrund: {detail}\n\nJetzt installieren?\n(ein Administratorpasswort wird abgefragt)",
    ),
    (
        "Mise à jour du composant système",
        "System component update",
        "Aktualisierung der Systemkomponente",
    ),
    (
        "La version embarquée du composant système (helper, outils) diffère\nde celle installée sur cette machine.\n\nMettre à jour maintenant ?\n(un mot de passe administrateur sera demandé)",
        "The bundled version of the system component (helper, tools) differs\nfrom the one installed on this machine.\n\nUpdate now?\n(an administrator password will be requested)",
        "Die mitgelieferte Version der Systemkomponente (Helper, Werkzeuge) unterscheidet sich\nvon der auf diesem Rechner installierten.\n\nJetzt aktualisieren?\n(ein Administratorpasswort wird abgefragt)",
    ),
    (
        "Installation impossible",
        "Installation failed",
        "Installation nicht möglich",
    ),
    (
        "{error}\n\nL'application va se fermer.",
        "{error}\n\nThe application will now close.",
        "{error}\n\nDie Anwendung wird beendet.",
    ),
    (
        "Composant système absent",
        "System component missing",
        "Systemkomponente fehlt",
    ),
    (
        "Sans le composant système, SecMet ne peut pas effectuer\nd'opérations sur les disques.\n\nL'application va se fermer.",
        "Without the system component, SecMet cannot perform\nany operation on disks.\n\nThe application will now close.",
        "Ohne die Systemkomponente kann SecMet keine\nVorgänge auf Datenträgern ausführen.\n\nDie Anwendung wird beendet.",
    ),
    (
        "Autorisation refusée",
        "Authorization denied",
        "Berechtigung verweigert",
    ),
    (
        "Authentification administrateur requise pour lire les disques.\n\nL'application va se fermer.",
        "Administrator authentication is required to read the disks.\n\nThe application will now close.",
        "Zum Lesen der Datenträger ist eine Administrator-Authentifizierung erforderlich.\n\nDie Anwendung wird beendet.",
    ),
    // ── enrichment.rs / disk_list.rs : badges et sous-titres ──
    ("INACTIF", "IDLE", "INAKTIV"),
    ("INDISPONIBLE", "UNAVAILABLE", "NICHT VERFÜGBAR"),
    ("ANALYSE…", "ANALYZING…", "ANALYSE…"),
    (" — Inspection…", " — Inspecting…", " — Prüfung…"),
    (
        "Détails vérification",
        "Verification details",
        "Prüfdetails",
    ),
    (
        " — USB (écriture hôte)",
        " — USB (host write)",
        " — USB (Host-Schreibvorgang)",
    ),
    (
        " — mot de passe ATA actif",
        " — ATA password active",
        " — ATA-Passwort aktiv",
    ),
    (
        " — mot de passe ATA SecMet levé",
        " — SecMet ATA password lifted",
        " — SecMet-ATA-Passwort aufgehoben",
    ),
    (
        " — mot de passe ATA SecMet non levé",
        " — SecMet ATA password not lifted",
        " — SecMet-ATA-Passwort nicht aufgehoben",
    ),
    (
        "Total sélectionné : 0 Go",
        "Total selected: 0 GB",
        "Gesamt ausgewählt: 0 GB",
    ),
    (
        "Total sélectionné : {size} To",
        "Total selected: {size} TB",
        "Gesamt ausgewählt: {size} TB",
    ),
    (
        "Total sélectionné : {size} Go",
        "Total selected: {size} GB",
        "Gesamt ausgewählt: {size} GB",
    ),
    (
        "❌ Inventaire impossible : {error}",
        "❌ Inventory failed: {error}",
        "❌ Inventar nicht möglich: {error}",
    ),
    (
        "❌ Aucun disque détecté",
        "❌ No disk detected",
        "❌ Kein Datenträger erkannt",
    ),
    // ── dialogs.rs : dé-frozen ──
    ("Dé-frozen SATA", "SATA un-freeze", "SATA-Auftauen"),
    (
        "Le BIOS/UEFI peut geler les commandes ATA Security (état « frozen ») au démarrage.\n\nProcédure :\n1. Vérifier la liste des disques gelés ci-dessous.\n2. Cliquer « Mettre en veille » : la machine s'endort quelques secondes et se réveille d'elle-même ; le gel est levé sur la plupart des machines.\n3. Cliquer « Rafraîchir » pour mettre à jour les cartes.\n\nSi l'écran reste noir au réveil, maintenir le bouton d'alimentation pour éteindre, puis redémarrer : rien n'est perdu (le lot est conservé). Sans dégel, l'effacement par écriture (NIST Clear) reste possible ; sur un SSD qui supporte SANITIZE, le gel n'empêche pas le Purge.",
        "The BIOS/UEFI may freeze ATA Security commands (\"frozen\" state) at boot.\n\nProcedure:\n1. Check the list of frozen disks below.\n2. Click \"Suspend\": the machine sleeps for a few seconds and wakes up by itself; the freeze is lifted on most machines.\n3. Click \"Refresh\" to update the cards.\n\nIf the screen stays black after waking, hold the power button to switch off, then restart: nothing is lost (the batch is kept). Without un-freezing, host-write erasure (NIST Clear) remains possible; on an SSD that supports SANITIZE, the freeze does not prevent Purge.",
        "Das BIOS/UEFI kann ATA-Security-Befehle beim Start einfrieren (Zustand „frozen“).\n\nVorgehen:\n1. Die Liste der eingefrorenen Datenträger unten prüfen.\n2. Auf „In Standby versetzen“ klicken: der Rechner schläft einige Sekunden und wacht von selbst auf; auf den meisten Rechnern ist die Sperre danach aufgehoben.\n3. Auf „Aktualisieren“ klicken, um die Karten zu aktualisieren.\n\nBleibt der Bildschirm nach dem Aufwachen schwarz: Einschalttaste gedrückt halten, ausschalten, neu starten; nichts geht verloren (die Charge bleibt erhalten). Ohne Auftauen bleibt das Löschen durch Überschreiben (NIST Clear) möglich; bei einer SSD mit SANITIZE verhindert die Sperre den Purge nicht.",
    ),
    (
        "<b>Aucun disque SATA gelé détecté.</b>",
        "<b>No frozen SATA disk detected.</b>",
        "<b>Kein eingefrorener SATA-Datenträger erkannt.</b>",
    ),
    (
        "Disques gelés :\n{list}",
        "Frozen disks:\n{list}",
        "Eingefrorene Datenträger:\n{list}",
    ),
    ("Fermer", "Close", "Schließen"),
    ("Mettre en veille", "Suspend", "In Standby versetzen"),
    (
        "Session administrateur absente",
        "Administrator session missing",
        "Administratorsitzung fehlt",
    ),
    ("Dé-frozen", "Un-freeze", "Auftauen"),
    (
        "Le disque est encore gelé après la reprise.\nEssayez un hot-plug SATA ou le mode AHCI dans le BIOS.",
        "The disk is still frozen after resume.\nTry a SATA hot-plug or AHCI mode in the BIOS.",
        "Der Datenträger ist nach dem Aufwachen weiterhin eingefroren.\nVersuchen Sie SATA-Hot-Plug oder den AHCI-Modus im BIOS.",
    ),
    (
        "Échec mise en veille",
        "Suspend failed",
        "Standby fehlgeschlagen",
    ),
    // ── dialogs.rs : déverrouillage ATA ──
    (
        "Déverrouiller un disque ATA",
        "Unlock an ATA disk",
        "ATA-Datenträger entsperren",
    ),
    (
        "Ce disque porte un mot de passe ATA Security (verrouillé, ou simplement actif).\n\nSaisissez le mot de passe connu du propriétaire : le helper déverrouille le disque puis lève ce mot de passe, afin de rendre l'effacement firmware possible. Sans le bon mot de passe, le disque refuse et rien n'est modifié. Le compteur de tentatives du disque peut se bloquer après cinq refus (cycle d'alimentation requis).",
        "This disk carries an ATA Security password (locked, or simply active).\n\nEnter the password known to the owner: the helper unlocks the disk and then lifts this password so that a firmware erase becomes possible. Without the correct password the disk refuses and nothing is changed. The disk's attempt counter may lock after five refusals (power cycle required).",
        "Dieser Datenträger trägt ein ATA-Security-Passwort (gesperrt oder lediglich aktiv).\n\nGeben Sie das dem Eigentümer bekannte Passwort ein: der Helper entsperrt den Datenträger und hebt dieses Passwort anschließend auf, damit eine Firmware-Löschung möglich wird. Ohne das richtige Passwort lehnt der Datenträger ab und nichts wird verändert. Der Versuchszähler des Datenträgers kann nach fünf Ablehnungen sperren (Stromzyklus erforderlich).",
    ),
    (
        "Mot de passe ATA (1 à 32 caractères ASCII)",
        "ATA password (1 to 32 ASCII characters)",
        "ATA-Passwort (1 bis 32 ASCII-Zeichen)",
    ),
    (
        "Il s'agit du mot de passe maître",
        "This is the master password",
        "Dies ist das Master-Passwort",
    ),
    ("Annuler", "Cancel", "Abbrechen"),
    ("Déverrouiller", "Unlock", "Entsperren"),
    (
        "Le mot de passe doit faire 1 à 32 caractères ASCII.",
        "The password must be 1 to 32 ASCII characters.",
        "Das Passwort muss 1 bis 32 ASCII-Zeichen lang sein.",
    ),
    (
        "Déverrouillage en cours…",
        "Unlocking…",
        "Entsperrung läuft…",
    ),
    (
        "Le disque est toujours verrouillé.",
        "The disk is still locked.",
        "Der Datenträger ist weiterhin gesperrt.",
    ),
    ("Refusé : {error}", "Refused: {error}", "Abgelehnt: {error}"),
    (
        "Réponse du helper absente.",
        "No response from the helper.",
        "Keine Antwort vom Helper.",
    ),
    // ── dialogs.rs : confirmation finale ──
    (
        "\n\nNote : un ou plusieurs disques sont en USB. Aucune commande firmware n'est envoyée à travers un pont USB : écriture hôte (NIST Clear).\n",
        "\n\nNote: one or more disks are connected over USB. No firmware command is sent through a USB bridge: host write (NIST Clear).\n",
        "\n\nHinweis: ein oder mehrere Datenträger sind per USB angeschlossen. Über eine USB-Bridge wird kein Firmware-Befehl gesendet: Host-Schreibvorgang (NIST Clear).\n",
    ),
    (
        "Vous êtes sur le point d'effacer DÉFINITIVEMENT les disques suivants :\n\n{disks}\n{note}\nCette action est IRRÉVERSIBLE.\nToutes les données seront PERDUES.\n\nÊtes-vous ABSOLUMENT SÛR de vouloir continuer ?",
        "You are about to PERMANENTLY erase the following disks:\n\n{disks}\n{note}\nThis action is IRREVERSIBLE.\nAll data will be LOST.\n\nAre you ABSOLUTELY SURE you want to continue?",
        "Sie sind dabei, die folgenden Datenträger ENDGÜLTIG zu löschen:\n\n{disks}\n{note}\nDieser Vorgang ist UNUMKEHRBAR.\nAlle Daten gehen VERLOREN.\n\nSind Sie ABSOLUT SICHER, dass Sie fortfahren möchten?",
    ),
    (
        "⚠️ CONFIRMATION FINALE",
        "⚠️ FINAL CONFIRMATION",
        "⚠️ LETZTE BESTÄTIGUNG",
    ),
    (
        "Rapide — 1 passe + zéros (recommandé, NIST Clear)",
        "Fast — 1 pass + zeros (recommended, NIST Clear)",
        "Schnell — 1 Durchgang + Nullen (empfohlen, NIST Clear)",
    ),
    (
        "Historique — 3 passes aléatoires + zéros",
        "Legacy — 3 random passes + zeros",
        "Klassisch — 3 Zufallsdurchgänge + Nullen",
    ),
    (
        "<small>⏱ Rapide ≈ 2× moins long. Une seule passe suffit (NIST SP 800-88).\nLes SSD passent par les commandes firmware (Purge) — ce choix ne les concerne pas.</small>",
        "<small>⏱ Fast ≈ 2× shorter. A single pass is enough (NIST SP 800-88).\nSSDs use firmware commands (Purge) — this choice does not apply to them.</small>",
        "<small>⏱ Schnell ≈ 2× kürzer. Ein einziger Durchgang genügt (NIST SP 800-88).\nSSDs verwenden Firmware-Befehle (Purge) — diese Wahl betrifft sie nicht.</small>",
    ),
    (
        "<span size='large' weight='bold'>🔄 Opération en cours...</span>",
        "<span size='large' weight='bold'>🔄 Operation in progress...</span>",
        "<span size='large' weight='bold'>🔄 Vorgang läuft...</span>",
    ),
    // ── app.rs ──
    (
        "{app} – Effacement sécurisé",
        "{app} – Secure erase",
        "{app} – Sicheres Löschen",
    ),
    (
        "<b>ATTENTION :</b> Cette opération effacera <b>DÉFINITIVEMENT</b> et <b>IRRÉMÉDIABLEMENT</b> toutes les données des disques sélectionnés.",
        "<b>WARNING:</b> This operation will <b>PERMANENTLY</b> and <b>IRREVERSIBLY</b> erase all data on the selected disks.",
        "<b>ACHTUNG:</b> Dieser Vorgang löscht <b>ENDGÜLTIG</b> und <b>UNWIDERRUFLICH</b> alle Daten der ausgewählten Datenträger.",
    ),
    (
        "Cette action ne peut PAS être annulée (sauf interruption immédiate du processus).",
        "This action can NOT be undone (except by stopping the process immediately).",
        "Dieser Vorgang kann NICHT rückgängig gemacht werden (außer durch sofortiges Abbrechen des Prozesses).",
    ),
    ("Tout sélectionner", "Select all", "Alle auswählen"),
    (
        "Authentification administrateur en cours…",
        "Administrator authentication in progress…",
        "Administrator-Authentifizierung läuft…",
    ),
    (
        "Lancer la suppression sécurisée",
        "Start secure erase",
        "Sicheres Löschen starten",
    ),
    (
        "Relancer les échoués",
        "Retry failed",
        "Fehlgeschlagene wiederholen",
    ),
    (
        "Déverrouiller un disque ATA Security avec un mot de passe connu",
        "Unlock an ATA Security disk with a known password",
        "ATA-Security-Datenträger mit bekanntem Passwort entsperren",
    ),
    ("Générer le PDF", "Generate PDF", "PDF erzeugen"),
    ("Rafraîchir", "Refresh", "Aktualisieren"),
    (
        "Autorisation requise",
        "Authorization required",
        "Berechtigung erforderlich",
    ),
    ("Opération en cours", "Operation in progress", "Vorgang läuft"),
    (
        "Terminez ou attendez la fin des effacements avant de déverrouiller un disque.",
        "Finish or wait for the erasures to complete before unlocking a disk.",
        "Beenden Sie die Löschvorgänge oder warten Sie deren Ende ab, bevor Sie einen Datenträger entsperren.",
    ),
    (
        "Aucun disque à déverrouiller",
        "No disk to unlock",
        "Kein Datenträger zum Entsperren",
    ),
    (
        "Aucun disque de la liste ne rapporte de mot de passe ATA Security.",
        "No disk in the list reports an ATA Security password.",
        "Kein Datenträger in der Liste meldet ein ATA-Security-Passwort.",
    ),
    (
        "Perdre les résultats affichés ?",
        "Discard the displayed results?",
        "Angezeigte Ergebnisse verwerfen?",
    ),
    (
        "Des résultats d'effacement sont affichés.\nRafraîchir va les effacer de cet ordinateur ; les certificats PDF déjà générés sont conservés.\n\nContinuer ?",
        "Erasure results are displayed.\nRefreshing will remove them from this computer; PDF certificates already generated are kept.\n\nContinue?",
        "Es werden Löschergebnisse angezeigt.\nAktualisieren entfernt sie von diesem Rechner; bereits erzeugte PDF-Zertifikate bleiben erhalten.\n\nFortfahren?",
    ),
    (
        "Aucun disque sélectionné",
        "No disk selected",
        "Kein Datenträger ausgewählt",
    ),
    (
        "Veuillez sélectionner au moins un disque.",
        "Please select at least one disk.",
        "Bitte wählen Sie mindestens einen Datenträger aus.",
    ),
    (
        "Impossible d'effacer {disk}",
        "Cannot erase {disk}",
        "{disk} kann nicht gelöscht werden",
    ),
    (
        "Terminez ou attendez la fin des effacements avant d'utiliser Dé-frozen.",
        "Finish or wait for the erasures to complete before using Un-freeze.",
        "Beenden Sie die Löschvorgänge oder warten Sie deren Ende ab, bevor Sie Auftauen verwenden.",
    ),
    (
        "Opérations en cours",
        "Operations in progress",
        "Vorgänge laufen",
    ),
    (
        "Des opérations sont déjà en cours.",
        "Operations are already in progress.",
        "Es laufen bereits Vorgänge.",
    ),
    ("Aucun échec", "No failure", "Kein Fehler"),
    (
        "Aucun disque n'est en échec.",
        "No disk has failed.",
        "Kein Datenträger ist fehlgeschlagen.",
    ),
    (
        "Effacement en cours",
        "Erasure in progress",
        "Löschung läuft",
    ),
    (
        "Un effacement est en cours.\n\nSi vous fermez SecMet maintenant, le suivi, la vérification et le certificat de cette session seront perdus. L'opération en cours continue jusqu'à son terme dans le disque (une commande firmware ne peut pas être interrompue).\n\nFermer quand même ?",
        "An erasure is in progress.\n\nIf you close SecMet now, the tracking, verification and certificate of this session will be lost. The current operation continues to completion inside the disk (a firmware command cannot be interrupted).\n\nClose anyway?",
        "Eine Löschung läuft.\n\nWenn Sie SecMet jetzt schließen, gehen Verfolgung, Prüfung und Zertifikat dieser Sitzung verloren. Der laufende Vorgang wird im Datenträger bis zum Ende fortgesetzt (ein Firmware-Befehl kann nicht unterbrochen werden).\n\nTrotzdem schließen?",
    ),
];
