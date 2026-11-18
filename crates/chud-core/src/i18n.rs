//! Langue des textes destinés à l'utilisateur (français ou anglais).
//!
//! Les textes sont écrits dans le code, côte à côte, avec [`tr!`](crate::tr) et
//! [`trf!`](crate::trf) : la traduction ne peut pas être oubliée ni se désynchroniser d'un
//! fichier de chaînes. La langue est un réglage global du processus ; les tests peuvent la fixer
//! pour leur seul fil d'exécution avec [`with_lang`].

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Fr,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        let c = code.trim().to_ascii_lowercase();
        let base = c.split(['-', '_', '.', '@']).next().unwrap_or("");
        match base {
            "fr" => Some(Lang::Fr),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

/// Choix enregistré dans les réglages : la langue du système ou une langue imposée.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LangChoice {
    #[default]
    System,
    Fr,
    En,
}

impl LangChoice {
    pub fn resolve(self) -> Lang {
        match self {
            LangChoice::System => system_lang(),
            LangChoice::Fr => Lang::Fr,
            LangChoice::En => Lang::En,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "system" | "systeme" | "système" | "auto" => Some(LangChoice::System),
            other => Lang::parse(other).map(|l| match l {
                Lang::Fr => LangChoice::Fr,
                Lang::En => LangChoice::En,
            }),
        }
    }
}

static GLOBAL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

fn to_u8(l: Lang) -> u8 {
    match l {
        Lang::Fr => 0,
        Lang::En => 1,
    }
}

/// Langue courante (celle du fil d'exécution si un test l'a fixée, sinon celle du processus).
pub fn lang() -> Lang {
    if let Some(l) = OVERRIDE.with(Cell::get) {
        return l;
    }
    match GLOBAL.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Fr,
    }
}

pub fn set_lang(l: Lang) {
    GLOBAL.store(to_u8(l), Ordering::Relaxed);
}

/// Exécute `f` avec une langue fixée pour ce seul fil (tests).
pub fn with_lang<R>(l: Lang, f: impl FnOnce() -> R) -> R {
    let previous = OVERRIDE.with(|o| o.replace(Some(l)));
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(previous);
    f()
}

/// Langue du système : français si la langue préférée de l'utilisateur est le français,
/// anglais sinon (y compris pour les langues qui ne sont pas encore traduites).
pub fn system_lang() -> Lang {
    for var in ["CHUD_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        // Sous Windows, seule CHUD_LANG compte (LANG y vient souvent d'outils Unix).
        if (var == "CHUD_LANG" || cfg!(not(windows)))
            && let Ok(v) = std::env::var(var)
            && !v.is_empty()
            && v != "C"
            && v != "POSIX"
        {
            return Lang::parse(&v).unwrap_or(Lang::En);
        }
    }
    sys_locale::get_locale().and_then(|l| Lang::parse(&l)).unwrap_or(Lang::En)
}

/// Choisit le texte de la langue courante.
pub fn pick<'a>(fr: &'a str, en: &'a str) -> &'a str {
    match lang() {
        Lang::Fr => fr,
        Lang::En => en,
    }
}

/// Texte fixe dans la langue courante : `tr!("Annuler", "Cancel")`.
#[macro_export]
macro_rules! tr {
    ($fr:expr, $en:expr $(,)?) => {
        $crate::i18n::pick($fr, $en)
    };
}

/// Texte formaté dans la langue courante : `trf!("{n} fichiers", "{n} files")`.
/// Les arguments supplémentaires sont partagés par les deux versions.
#[macro_export]
macro_rules! trf {
    ($fr:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::Fr => format!($fr $(, $arg)*),
            $crate::i18n::Lang::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locales_and_choices() {
        assert_eq!(Lang::parse("fr-FR"), Some(Lang::Fr));
        assert_eq!(Lang::parse("fr_CA.UTF-8"), Some(Lang::Fr));
        assert_eq!(Lang::parse("en-US"), Some(Lang::En));
        assert_eq!(Lang::parse("de-DE"), None);
        assert_eq!(LangChoice::parse("System"), Some(LangChoice::System));
        assert_eq!(LangChoice::parse("EN"), Some(LangChoice::En));
        assert_eq!(LangChoice::parse("klingon"), None);
    }

    #[test]
    fn thread_override_is_scoped() {
        let n = 3;
        let fr = with_lang(Lang::Fr, || (tr!("Annuler", "Cancel").to_owned(), trf!("{n} fichiers", "{n} files")));
        let en = with_lang(Lang::En, || (tr!("Annuler", "Cancel").to_owned(), trf!("{} fichiers", "{} files", n)));
        assert_eq!(fr, ("Annuler".to_owned(), "3 fichiers".to_owned()));
        assert_eq!(en, ("Cancel".to_owned(), "3 files".to_owned()));
        // Imbriqué : la langue extérieure revient.
        with_lang(Lang::En, || {
            with_lang(Lang::Fr, || assert_eq!(lang(), Lang::Fr));
            assert_eq!(lang(), Lang::En);
        });
    }
}
