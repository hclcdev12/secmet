//! Table de traduction `(fr, en, de)` — textes libres des certificats PDF
//! (méthode, réserves, valeurs par défaut), rédigés dans la langue du PDF.

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    (
        "Écriture hôte : {n} passe pseudo-aléatoire",
        "Host overwrite: {n} pseudo-random pass",
        "Host-Überschreibung: {n} Pseudozufallsdurchgang",
    ),
    (
        "Écriture hôte : {n} passes pseudo-aléatoires",
        "Host overwrite: {n} pseudo-random passes",
        "Host-Überschreibung: {n} Pseudozufallsdurchgänge",
    ),
    ("{method} + zéros", "{method} + zeros", "{method} + Nullen"),
    (
        "NVMe Sanitize Overwrite ({n} passe)",
        "NVMe Sanitize Overwrite ({n} pass)",
        "NVMe Sanitize Overwrite ({n} Durchgang)",
    ),
    (
        "NVMe Sanitize Overwrite ({n} passes)",
        "NVMe Sanitize Overwrite ({n} passes)",
        "NVMe Sanitize Overwrite ({n} Durchgänge)",
    ),
    (
        "aucune catégorie NIST rapportée par le moteur",
        "no NIST category reported by the engine",
        "keine NIST-Kategorie vom Entscheidungsmodul gemeldet",
    ),
    (
        "disque derrière un pont USB : aucune commande firmware envoyée",
        "disk behind a USB bridge: no firmware command sent",
        "Datenträger hinter einer USB-Brücke: kein Firmware-Befehl gesendet",
    ),
    (
        "contrôleur NVMe à plusieurs espaces de noms : seul l'espace de noms effacé est couvert par l'étape exécutée, les autres restent à traiter",
        "NVMe controller with several namespaces: only the erased namespace is covered by the executed step, the others remain to be processed",
        "NVMe-Controller mit mehreren Namespaces: Nur der gelöschte Namespace ist durch den ausgeführten Schritt abgedeckt, die anderen sind noch zu behandeln",
    ),
    (
        "disque système de l'hôte non identifié lors de l'inventaire (racine non rattachée à un disque local : média live ou ZFS) ; protection du disque système par verrou exclusif du noyau uniquement",
        "host system disk not identified during inventory (root not attached to a local disk: live media or ZFS); system disk protected by the kernel exclusive lock only",
        "Systemdatenträger des Hosts bei der Bestandsaufnahme nicht erkannt (Root keinem lokalen Datenträger zugeordnet: Live-Medium oder ZFS); Schutz des Systemdatenträgers nur durch die exklusive Kernel-Sperre",
    ),
    (
        "aucune relecture enregistrée : catégorie NIST non revendiquée",
        "no read-back recorded: NIST category not claimed",
        "kein Rücklesen aufgezeichnet: NIST-Kategorie nicht beansprucht",
    ),
    (
        "numéro de série non exposé par le disque : identification par modèle, capacité et nom de périphérique",
        "serial number not exposed by the disk: identified by model, capacity and device name",
        "Seriennummer vom Datenträger nicht bereitgestellt: Identifizierung über Modell, Kapazität und Gerätename",
    ),
    (
        "horloge du poste non synchronisée (aucune source de temps réseau) : dates fournies par le poste",
        "system clock not synchronised (no network time source): dates as provided by the host",
        "Systemuhr nicht synchronisiert (keine Netzwerk-Zeitquelle): Daten vom Host übernommen",
    ),
    (
        "horloge du poste douteuse (antérieure à une référence connue), conservée sur confirmation de l'opérateur",
        "questionable system clock (earlier than a known reference), kept on the operator's confirmation",
        "zweifelhafte Systemuhr (vor einer bekannten Referenz), auf Bestätigung des Bedieners beibehalten",
    ),
    (
        "date et heure saisies par l'opérateur : horloge du poste non fiable",
        "date and time entered by the operator: unreliable system clock",
        "Datum und Uhrzeit vom Bediener eingegeben: Systemuhr unzuverlässig",
    ),
    // ── Intégrité du système de la clé USB (src/system_integrity.rs) ──
    (
        "système de la clé USB altéré : empreintes ou signature différentes de l'image publiée (contrôle au démarrage)",
        "USB key system altered: checksums or signature differ from the published image (boot-time check)",
        "System des USB-Sticks verändert: Prüfsummen oder Signatur weichen vom veröffentlichten Abbild ab (Prüfung beim Start)",
    ),
    (
        "intégrité du système de la clé USB non vérifiée : image sans empreintes signées",
        "USB key system integrity not verified: image without signed checksums",
        "Integrität des USB-Stick-Systems nicht geprüft: Abbild ohne signierte Prüfsummen",
    ),
    (
        "intégrité du système de la clé USB non vérifiée : contrôle au démarrage non terminé ou impossible",
        "USB key system integrity not verified: boot-time check not finished or not possible",
        "Integrität des USB-Stick-Systems nicht geprüft: Prüfung beim Start nicht abgeschlossen oder nicht möglich",
    ),
    ("Non disponible", "Not available", "Nicht verfügbar"),
    ("INCONNU", "UNKNOWN", "UNBEKANNT"),
    ("non effectuée", "not performed", "nicht durchgeführt"),
    // ── signing_key_ui.rs : clé de signature absente ──
    (
        "Clé de signature absente",
        "Signing key missing",
        "Signaturschlüssel fehlt",
    ),
    (
        "Clé de signature créée",
        "Signing key created",
        "Signaturschlüssel erstellt",
    ),
    (
        "Créer une nouvelle clé",
        "Create a new key",
        "Neuen Schlüssel erstellen",
    ),
    ("inconnu", "unknown", "unbekannt"),
    (
        "Clé de signature absente : aucun certificat n'est signé. Relancez SecMet pour créer une nouvelle clé.",
        "Signing key missing: no certificate is signed. Restart SecMet to create a new key.",
        "Signaturschlüssel fehlt: Es wird kein Zertifikat signiert. Starten Sie SecMet neu, um einen neuen Schlüssel zu erstellen.",
    ),
    (
        "La clé de signature des certificats de ce poste a disparu (ancien key_id : {key_id}).\n\nLes certificats déjà émis restent vérifiables avec l'ancienne clé publique, mais plus aucun certificat ne peut être signé par elle. Aucun certificat n'est signé tant qu'aucune clé n'est choisie.\n\nVous pouvez créer une nouvelle clé : elle aura un nouvel identifiant (key_id), à communiquer à vos clients pour qu'ils l'inscrivent dans leur liste de confiance (trusted-keys). Si l'ancienne clé a pu être copiée, demandez-leur aussi de retirer l'ancien identifiant.\n\nCréer une nouvelle clé de signature ?",
        "The certificate signing key of this workstation has disappeared (previous key_id: {key_id}).\n\nCertificates already issued remain verifiable with the previous public key, but no certificate can be signed with it any more. No certificate is signed until a key is chosen.\n\nYou can create a new key: it will have a new identifier (key_id), which you must give to your customers so that they add it to their trust list (trusted-keys). If the previous key may have been copied, also ask them to remove the previous identifier.\n\nCreate a new signing key?",
        "Der Zertifikat-Signaturschlüssel dieses Arbeitsplatzes ist verschwunden (bisherige key_id: {key_id}).\n\nBereits ausgestellte Zertifikate bleiben mit dem bisherigen öffentlichen Schlüssel prüfbar, aber kein Zertifikat kann mehr damit signiert werden. Solange kein Schlüssel gewählt ist, wird kein Zertifikat signiert.\n\nSie können einen neuen Schlüssel erstellen: Er erhält eine neue Kennung (key_id), die Sie Ihren Kunden mitteilen, damit sie sie in ihre Vertrauensliste (trusted-keys) aufnehmen. Falls der bisherige Schlüssel kopiert worden sein könnte, bitten Sie sie auch, die bisherige Kennung zu entfernen.\n\nNeuen Signaturschlüssel erstellen?",
    ),
    (
        "Cette clé USB SecMet n'a pas de clé de signature des certificats (elle est normalement créée au premier démarrage). Aucun certificat n'est signé tant qu'aucune clé n'est créée.\n\nSi cette clé USB a déjà signé des certificats, sa clé de signature a été perdue : une nouvelle clé aura un nouvel identifiant (key_id), à communiquer à vos clients pour qu'ils l'inscrivent dans leur liste de confiance (trusted-keys).\n\nCréer une clé de signature ?",
        "This SecMet USB key has no certificate signing key (it is normally created at first boot). No certificate is signed until a key is created.\n\nIf this USB key has already signed certificates, its signing key has been lost: a new key will have a new identifier (key_id), which you must give to your customers so that they add it to their trust list (trusted-keys).\n\nCreate a signing key?",
        "Dieser SecMet-USB-Stick hat keinen Zertifikat-Signaturschlüssel (er wird normalerweise beim ersten Start erstellt). Solange kein Schlüssel erstellt ist, wird kein Zertifikat signiert.\n\nFalls dieser USB-Stick bereits Zertifikate signiert hat, ist sein Signaturschlüssel verloren: Ein neuer Schlüssel erhält eine neue Kennung (key_id), die Sie Ihren Kunden mitteilen, damit sie sie in ihre Vertrauensliste (trusted-keys) aufnehmen.\n\nSignaturschlüssel erstellen?",
    ),
    (
        "Nouvelle clé de signature créée.\n\nkey_id : {key_id}\nClé publique : {public_key}\n\nCommuniquez ce nouvel identifiant à vos clients : il figure aussi sur chaque certificat signé avec cette clé.",
        "New signing key created.\n\nkey_id: {key_id}\nPublic key: {public_key}\n\nGive this new identifier to your customers: it also appears on every certificate signed with this key.",
        "Neuer Signaturschlüssel erstellt.\n\nkey_id: {key_id}\nÖffentlicher Schlüssel: {public_key}\n\nTeilen Sie diese neue Kennung Ihren Kunden mit: Sie steht auch auf jedem mit diesem Schlüssel signierten Zertifikat.",
    ),
];
