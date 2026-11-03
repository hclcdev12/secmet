//! Table de traduction `(fr, en, de)` — horloge du poste (annexe M, §M.7).

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    ("Horloge du poste", "System clock", "Systemuhr"),
    (
        "L'horloge du poste indique {now}, ce qui est antérieur à {floor} ({reason}). Les dates des certificats et l'ordre du lot en dépendent. Vous pouvez saisir la date et l'heure réelles : elles seront utilisées par SecMet, sans modifier l'horloge du poste, et le certificat le mentionnera.",
        "The system clock reads {now}, which is earlier than {floor} ({reason}). Certificate dates and the batch order depend on it. You can enter the actual date and time: SecMet will use them without changing the system clock, and the certificate will say so.",
        "Die Systemuhr zeigt {now}, das liegt vor {floor} ({reason}). Zertifikatsdaten und die Reihenfolge der Charge hängen davon ab. Sie können Datum und Uhrzeit eingeben: SecMet verwendet sie, ohne die Systemuhr zu ändern, und das Zertifikat vermerkt dies.",
    ),
    (
        "construction du logiciel",
        "software build",
        "Erstellung der Software",
    ),
    (
        "dernière entrée du lot",
        "last batch entry",
        "letzter Chargeneintrag",
    ),
    (
        "dernier lot clôturé",
        "last closed batch",
        "letzte abgeschlossene Charge",
    ),
    (
        "Date et heure réelles (AAAA-MM-JJ HH:MM)",
        "Actual date and time (YYYY-MM-DD HH:MM)",
        "Tatsächliches Datum und Uhrzeit (JJJJ-MM-TT HH:MM)",
    ),
    (
        "Garder l'horloge du poste",
        "Keep the system clock",
        "Systemuhr beibehalten",
    ),
    ("Utiliser cette date", "Use this date", "Dieses Datum verwenden"),
    (
        "Date illisible : AAAA-MM-JJ HH:MM attendu",
        "Unreadable date: YYYY-MM-DD HH:MM expected",
        "Datum nicht lesbar: JJJJ-MM-TT HH:MM erwartet",
    ),
    (
        "La date saisie est antérieure à {floor}.",
        "The entered date is earlier than {floor}.",
        "Das eingegebene Datum liegt vor {floor}.",
    ),
];
