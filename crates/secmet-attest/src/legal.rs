//! Mentions légales communes à l'application SecMet et à SecMet Clé USB :
//! titulaire du droit d'auteur, licence, adresse du dépôt des sources.
//!
//! **Seul endroit du code à changer si le titulaire change** (décision D1,
//! pas encore prise : « SecMet Contributors » en attendant). Les textes
//! `LICENSE` et `content/LICENSE` à la racine du dépôt portent aussi le nom
//! et sont à mettre à jour avec lui.

/// Titulaire du droit d'auteur.
pub const COPYRIGHT_HOLDER: &str = "SecMet Contributors";

/// Années du droit d'auteur.
pub const COPYRIGHT_YEARS: &str = "2026";

/// Identifiant SPDX de la licence du code de SecMet.
pub const LICENSE_SPDX: &str = "GPL-3.0-or-later";

/// Nom de la licence, en anglais (seule version qui fait foi).
pub const LICENSE_NAME: &str = "GNU General Public License, version 3 or later";

/// Dépôt public des sources.
pub const SOURCE_REPOSITORY: &str = "https://github.com/hclcdev12/secmet";

/// Notice de licence officielle (en anglais), dite « Appropriate Legal
/// Notices » par la GPL-3 §5(d) : liberté, absence de garantie, renvoi au
/// texte de la licence.
pub const LICENSE_NOTICE_EN: &str =
    "This program is free software: you can redistribute it and/or modify \
it under the terms of the GNU General Public License as published by the Free Software \
Foundation, either version 3 of the License, or (at your option) any later version.\n\n\
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; \
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. \
See the GNU General Public License for more details.";

/// `Copyright (C) <années> <titulaire>`.
pub fn copyright_line() -> String {
    format!("Copyright (C) {COPYRIGHT_YEARS} {COPYRIGHT_HOLDER}")
}

/// `© <années> <titulaire>`, pour une fenêtre « À propos ».
pub fn copyright_short() -> String {
    format!("© {COPYRIGHT_YEARS} {COPYRIGHT_HOLDER}")
}

/// Sources exactes d'une version : arborescence du dépôt au tag `v<version>`.
pub fn source_url_for(version: &str) -> String {
    format!("{SOURCE_REPOSITORY}/tree/v{version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_use_the_single_holder() {
        assert!(copyright_line().ends_with(COPYRIGHT_HOLDER));
        assert!(copyright_short().contains(COPYRIGHT_YEARS));
        assert_eq!(
            source_url_for("1.2.3"),
            format!("{SOURCE_REPOSITORY}/tree/v1.2.3")
        );
        assert!(LICENSE_NOTICE_EN.contains("WITHOUT ANY WARRANTY"));
    }

    /// `LICENSE` à la racine du dépôt nomme le même titulaire.
    #[test]
    fn license_file_names_the_same_holder() {
        let license = include_str!("../../../LICENSE");
        assert!(
            license.contains(&copyright_line()),
            "LICENSE : titulaire différent"
        );
    }
}
