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
    ("Non disponible", "Not available", "Nicht verfügbar"),
    ("INCONNU", "UNKNOWN", "UNBEKANNT"),
    ("non effectuée", "not performed", "nicht durchgeführt"),
];
