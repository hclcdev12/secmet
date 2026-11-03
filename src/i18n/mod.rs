//! Internationalisation minimale : le français est la langue native et sert
//! de clé ; l'anglais et l'allemand sont des tables `(fr, en, de)`.
//!
//! `tr("Rafraîchir")` renvoie la traduction dans la langue courante, ou la
//! clé elle-même si elle n'est pas traduite (jamais de chaîne vide). Les
//! chaînes composées utilisent [`trf`] avec des marqueurs `{nom}`.
//!
//! Les certificats PDF sont rédigés dans la langue de l'interface au moment
//! de leur génération ; les journaux de session restent en français.

mod certificate;
mod clock;
mod lot;
mod misc;
mod settings;
mod ui_a;
mod ui_b;
mod update;

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Langues disponibles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// Français (natif).
    #[default]
    Fr,
    /// Anglais.
    En,
    /// Allemand.
    De,
}

impl Lang {
    /// Toutes les langues, dans l'ordre d'affichage.
    pub const ALL: [Lang; 3] = [Lang::Fr, Lang::En, Lang::De];

    /// Code court persistant (`fr`, `en`, `de`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// Nom de la langue, dans la langue elle-même.
    pub fn label(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }

    /// Depuis un code ; inconnu → français.
    pub fn from_code(code: &str) -> Lang {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Lang::En,
            "de" => Lang::De,
            _ => Lang::Fr,
        }
    }
}

static CURRENT: RwLock<Lang> = RwLock::new(Lang::Fr);

/// Langue courante.
pub fn current() -> Lang {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// Change la langue courante (les libellés déjà construits ne bougent pas :
/// l'UI est reconstruite par l'appelant).
pub fn set_current(lang: Lang) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = lang;
}

type Table = HashMap<&'static str, (&'static str, &'static str)>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for entries in [
            ui_a::TABLE,
            ui_b::TABLE,
            misc::TABLE,
            settings::TABLE,
            certificate::TABLE,
            update::TABLE,
            lot::TABLE,
            clock::TABLE,
        ] {
            for (fr, en, de) in entries {
                map.insert(*fr, (*en, *de));
            }
        }
        map
    })
}

/// Traduction de `key` (texte français) dans la langue courante.
pub fn tr(key: &str) -> String {
    tr_in(current(), key).to_string()
}

/// Traduction de `key` dans `lang`, ou `key` si absente ou vide.
pub fn tr_in(lang: Lang, key: &str) -> &str {
    if lang == Lang::Fr {
        return key;
    }
    match table().get(key) {
        Some((en, de)) => {
            let text = match lang {
                Lang::En => *en,
                Lang::De => *de,
                Lang::Fr => key,
            };
            if text.is_empty() {
                key
            } else {
                text
            }
        }
        None => key,
    }
}

/// Traduction puis substitution des marqueurs `{nom}`.
pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    trf_in(current(), key, args)
}

/// [`trf`] dans une langue donnée (journaux en français, affichage dans
/// la langue courante).
pub fn trf_in(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr_in(lang, key).to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Clés traduites dans les deux langues (pour les tests de complétude).
pub fn untranslated(lang: Lang) -> Vec<&'static str> {
    let mut missing: Vec<&'static str> = table()
        .iter()
        .filter(|(_, (en, de))| match lang {
            Lang::En => en.is_empty(),
            Lang::De => de.is_empty(),
            Lang::Fr => false,
        })
        .map(|(fr, _)| *fr)
        .collect();
    missing.sort_unstable();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_is_identity_and_unknown_keys_pass_through() {
        assert_eq!(tr_in(Lang::Fr, "Rafraîchir"), "Rafraîchir");
        assert_eq!(tr_in(Lang::En, "clé inconnue §"), "clé inconnue §");
        assert_eq!(Lang::from_code("DE"), Lang::De);
        assert_eq!(Lang::from_code("xx"), Lang::Fr);
    }

    #[test]
    fn placeholders_are_substituted() {
        let text = trf("Étape {n} : {m}", &[("n", "1"), ("m", "x")]);
        assert_eq!(text, "Étape 1 : x");
    }

    #[test]
    fn every_key_has_both_translations() {
        assert!(
            untranslated(Lang::En).is_empty(),
            "{:?}",
            untranslated(Lang::En)
        );
        assert!(
            untranslated(Lang::De).is_empty(),
            "{:?}",
            untranslated(Lang::De)
        );
    }

    /// Une clé peut apparaître dans plusieurs tables (« Annuler »…) à
    /// condition d'y recevoir les mêmes traductions.
    #[test]
    fn duplicate_keys_translate_identically() {
        let mut seen: HashMap<&str, (&str, &str)> = HashMap::new();
        for entries in [
            ui_a::TABLE,
            ui_b::TABLE,
            misc::TABLE,
            settings::TABLE,
            certificate::TABLE,
            update::TABLE,
            lot::TABLE,
            clock::TABLE,
        ] {
            for (fr, en, de) in entries {
                if let Some(previous) = seen.insert(fr, (en, de)) {
                    assert_eq!(
                        previous,
                        (*en, *de),
                        "traductions divergentes pour « {fr} »"
                    );
                }
            }
        }
    }

    #[test]
    fn placeholders_survive_translation() {
        for entries in [
            ui_a::TABLE,
            ui_b::TABLE,
            misc::TABLE,
            settings::TABLE,
            certificate::TABLE,
            update::TABLE,
            lot::TABLE,
            clock::TABLE,
        ] {
            for (fr, en, de) in entries {
                let names = |s: &str| {
                    let mut v: Vec<String> = s
                        .split('{')
                        .skip(1)
                        .filter_map(|p| p.split('}').next().map(str::to_string))
                        .collect();
                    v.sort();
                    v
                };
                assert_eq!(names(fr), names(en), "marqueurs EN pour « {fr} »");
                assert_eq!(names(fr), names(de), "marqueurs DE pour « {fr} »");
            }
        }
    }
}
