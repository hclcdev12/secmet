//! Table de traduction `(fr, en, de)` — fenêtre « À propos » (mentions
//! légales, licence, avis tiers). Les traductions de la notice de licence
//! sont officieuses : seule la version anglaise fait foi.

pub(super) const TABLE: &[(&str, &str, &str)] = &[
    ("À propos", "About", "Über"),
    ("À propos de SecMet", "About SecMet", "Über SecMet"),
    ("Licence", "License", "Lizenz"),
    ("Licences tierces", "Third-party licenses", "Lizenzen Dritter"),
    ("Fermer", "Close", "Schließen"),
    ("Version {version}", "Version {version}", "Version {version}"),
    (
        "Effacement sécurisé de disques avec certificat signé, méthodes alignées sur NIST SP 800-88.",
        "Secure disk erasure with a signed certificate, methods aligned with NIST SP 800-88.",
        "Sichere Datenträgerlöschung mit signiertem Zertifikat, Verfahren nach NIST SP 800-88.",
    ),
    (
        "Logiciel libre sous licence {license}.",
        "Free software under the {license}.",
        "Freie Software unter der {license}.",
    ),
    (
        "Code source de cette version",
        "Source code of this version",
        "Quellcode dieser Version",
    ),
    ("Dépôt des sources", "Source repository", "Quellcode-Repository"),
    (
        "Ce programme est un logiciel libre : vous pouvez le redistribuer et/ou le modifier selon les termes de la GNU General Public License publiée par la Free Software Foundation, soit la version 3 de la licence, soit (à votre choix) toute version ultérieure.\n\nCe programme est distribué dans l'espoir qu'il sera utile, mais SANS AUCUNE GARANTIE, sans même la garantie implicite de QUALITÉ MARCHANDE ou d'ADÉQUATION À UN USAGE PARTICULIER. Consultez la GNU General Public License pour plus de détails.",
        "This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.\n\nThis program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.",
        "Dieses Programm ist freie Software: Sie können es unter den Bedingungen der GNU General Public License, wie von der Free Software Foundation veröffentlicht, weitergeben und/oder verändern, entweder gemäß Version 3 der Lizenz oder (nach Ihrer Wahl) jeder späteren Version.\n\nDieses Programm wird in der Hoffnung verbreitet, dass es nützlich sein wird, aber OHNE JEDE GEWÄHRLEISTUNG, sogar ohne die implizite Gewährleistung der MARKTREIFE oder der EIGNUNG FÜR EINEN BESTIMMTEN ZWECK. Einzelheiten finden Sie in der GNU General Public License.",
    ),
    (
        "Traduction officieuse : seule la version anglaise de la licence, reproduite ci-dessous, fait foi.",
        "The full text of the license follows.",
        "Inoffizielle Übersetzung: Verbindlich ist allein die unten wiedergegebene englische Fassung der Lizenz.",
    ),
    (
        "Le nom SecMet, le logo et les images du dossier content/ relèvent de leur propre licence (content/LICENSE, installée avec la documentation).",
        "The SecMet name, the logo and the images of the content/ folder are covered by their own license (content/LICENSE, installed with the documentation).",
        "Der Name SecMet, das Logo und die Bilder des Ordners content/ unterliegen einer eigenen Lizenz (content/LICENSE, mit der Dokumentation installiert).",
    ),
    (
        "SecMet intègre des bibliothèques Rust distribuées sous leurs propres licences, reproduites ci-dessous.",
        "SecMet includes Rust libraries distributed under their own licenses, reproduced below.",
        "SecMet enthält Rust-Bibliotheken, die unter ihren eigenen Lizenzen verbreitet werden; diese sind unten wiedergegeben.",
    ),
];
