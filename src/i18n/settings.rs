//! Table de traduction `(fr, en, de)` — dialogue Réglages.

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    ("Lots", "Batches", "Chargen"),
    (
        "Mode lot : regrouper les effacements en lots",
        "Batch mode: group erasures into batches",
        "Chargenmodus: Löschungen zu Chargen zusammenfassen",
    ),
    (
        "Au lancement, SecMet propose d'ouvrir ou de continuer un lot. Les disques effacés au fil des sessions (brancher, effacer, débrancher, recommencer) y sont enregistrés avec leur certificat ; la clôture produit le certificat groupé et le manifeste signé.",
        "At startup, SecMet offers to open or continue a batch. Disks erased over successive sessions (plug in, erase, unplug, repeat) are recorded in it with their certificate; closing produces the grouped certificate and the signed manifest.",
        "Beim Start bietet SecMet an, eine Charge zu eröffnen oder fortzusetzen. Die über mehrere Sitzungen gelöschten Datenträger (anschließen, löschen, abziehen, wiederholen) werden darin mit ihrem Zertifikat erfasst; der Abschluss erzeugt das Sammelzertifikat und das signierte Manifest.",
    ),
    ("Réglages", "Settings", "Einstellungen"),
    ("Langue de l'interface", "Interface language", "Sprache der Oberfläche"),
    (
        "Le changement de langue s'applique immédiatement à la fenêtre principale et aux certificats PDF générés ensuite.",
        "The language change applies immediately to the main window and to PDF certificates generated afterwards.",
        "Die Sprachänderung gilt sofort für das Hauptfenster und für danach erstellte PDF-Zertifikate.",
    ),
    ("Logo des certificats PDF", "PDF certificate logo", "Logo der PDF-Zertifikate"),
    (
        "Image PNG, JPEG, GIF, BMP, WebP ou TIFF. Elle est convertie en JPEG sur fond blanc et placée en haut à gauche de chaque page à la place du logo SecMet.",
        "PNG, JPEG, GIF, BMP, WebP or TIFF image. It is converted to JPEG on a white background and placed at the top left of every page instead of the SecMet logo.",
        "PNG-, JPEG-, GIF-, BMP-, WebP- oder TIFF-Bild. Es wird als JPEG auf weißem Hintergrund umgewandelt und oben links auf jeder Seite anstelle des SecMet-Logos platziert.",
    ),
    ("Aperçu indisponible", "Preview unavailable", "Vorschau nicht verfügbar"),
    ("Logo actuel : personnalisé", "Current logo: custom", "Aktuelles Logo: benutzerdefiniert"),
    ("Logo actuel : SecMet", "Current logo: SecMet", "Aktuelles Logo: SecMet"),
    ("Choisir une image…", "Choose an image…", "Bild auswählen…"),
    ("Logo SecMet", "SecMet logo", "SecMet-Logo"),
    ("Choisir le logo", "Choose the logo", "Logo auswählen"),
    ("Dossier des certificats", "Certificates folder", "Zertifikatsordner"),
    (
        "Certificats PDF et attestations signées déjà produits : lot en cours, ou lots clôturés et certificats du poste.",
        "PDF certificates and signed attestations already produced: current batch, or closed batches and the machine's certificates.",
        "Bereits erzeugte PDF-Zertifikate und signierte Bescheinigungen: laufende Charge, sonst abgeschlossene Chargen und Zertifikate des Rechners.",
    ),
    (
        "Ouvrir le dossier des certificats",
        "Open the certificates folder",
        "Zertifikatsordner öffnen",
    ),
    ("Annuler", "Cancel", "Abbrechen"),
    ("Ouvrir", "Open", "Öffnen"),
    ("Images", "Images", "Bilder"),
    (
        "Logo choisi : {file} ({w}×{h} px) — cliquez Valider pour l'enregistrer",
        "Logo selected: {file} ({w}×{h} px) — click Apply to save it",
        "Logo gewählt: {file} ({w}×{h} px) — zum Speichern auf Übernehmen klicken",
    ),
    ("Image refusée : {error}", "Image rejected: {error}", "Bild abgelehnt: {error}"),
    (
        "Logo SecMet — cliquez Valider pour l'enregistrer",
        "SecMet logo — click Apply to save it",
        "SecMet-Logo — zum Speichern auf Übernehmen klicken",
    ),
    ("Valider", "Apply", "Übernehmen"),
    (
        "Langue verrouillée pendant une opération en cours",
        "Language locked while an operation is running",
        "Sprache während eines laufenden Vorgangs gesperrt",
    ),
    ("Logo non enregistré : {error}", "Logo not saved: {error}", "Logo nicht gespeichert: {error}"),
    ("Opérateur", "Operator", "Bediener"),
    (
        "Nom pré-rempli dans le dialogue de génération des certificats PDF (modifiable à chaque génération). Laisser vide pour le saisir à chaque fois.",
        "Name pre-filled in the PDF certificate generation dialog (can be changed for each generation). Leave empty to enter it every time.",
        "Im Dialog zur Erstellung der PDF-Zertifikate vorausgefüllter Name (bei jeder Erstellung änderbar). Leer lassen, um ihn jedes Mal einzugeben.",
    ),
];
