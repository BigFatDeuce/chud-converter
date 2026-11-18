//! Schéma d'options déclaré par les moteurs.
//!
//! L'interface génère ses formulaires à partir des [`OptionSpec`] ; les profils et la CLI
//! fournissent des [`OptionValues`] qui sont validées et complétées par [`OptionValues::resolve`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl fmt::Display for OptionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionValue::Bool(b) => write!(f, "{b}"),
            OptionValue::Int(i) => write!(f, "{i}"),
            OptionValue::Float(x) => write!(f, "{x}"),
            OptionValue::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionKind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    Choice {
        choices: Vec<Choice>,
    },
    /// Texte libre validé par le moteur (jamais transmis à un shell).
    Text {
        max_len: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub help: Option<String>,
    pub kind: OptionKind,
    pub default: OptionValue,
    /// Masquée en mode simple.
    #[serde(default)]
    pub advanced: bool,
}

impl OptionSpec {
    pub fn bool(key: &str, label: &str, default: bool) -> Self {
        Self::make(key, label, OptionKind::Bool, OptionValue::Bool(default))
    }

    pub fn int(key: &str, label: &str, min: i64, max: i64, default: i64) -> Self {
        Self::make(key, label, OptionKind::Int { min, max }, OptionValue::Int(default))
    }

    pub fn float(key: &str, label: &str, min: f64, max: f64, default: f64) -> Self {
        Self::make(key, label, OptionKind::Float { min, max }, OptionValue::Float(default))
    }

    pub fn choice(key: &str, label: &str, choices: &[(&str, &str)], default: &str) -> Self {
        let choices = choices.iter().map(|(v, l)| Choice { value: (*v).to_owned(), label: (*l).to_owned() }).collect();
        Self::make(key, label, OptionKind::Choice { choices }, OptionValue::Text(default.to_owned()))
    }

    pub fn text(key: &str, label: &str, max_len: usize, default: &str) -> Self {
        Self::make(key, label, OptionKind::Text { max_len }, OptionValue::Text(default.to_owned()))
    }

    fn make(key: &str, label: &str, kind: OptionKind, default: OptionValue) -> Self {
        Self { key: key.to_owned(), label: label.to_owned(), help: None, kind, default, advanced: false }
    }

    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }

    pub fn help(mut self, help: &str) -> Self {
        self.help = Some(help.to_owned());
        self
    }

    /// Convertit et valide une valeur pour cette option.
    pub fn coerce(&self, v: &OptionValue) -> Result<OptionValue, String> {
        match (&self.kind, v) {
            (OptionKind::Bool, OptionValue::Bool(b)) => Ok(OptionValue::Bool(*b)),
            (OptionKind::Bool, OptionValue::Text(s)) => match s.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "oui" | "on" => Ok(OptionValue::Bool(true)),
                "0" | "false" | "no" | "non" | "off" => Ok(OptionValue::Bool(false)),
                _ => Err(format!("valeur booléenne attendue, reçu « {s} »")),
            },
            (OptionKind::Int { min, max }, v) => {
                let i = match v {
                    OptionValue::Int(i) => *i,
                    OptionValue::Float(f) if f.fract() == 0.0 => *f as i64,
                    OptionValue::Text(s) => s.trim().parse().map_err(|_| format!("entier attendu, reçu « {s} »"))?,
                    other => return Err(format!("entier attendu, reçu « {other} »")),
                };
                if i < *min || i > *max {
                    return Err(format!("doit être compris entre {min} et {max}"));
                }
                Ok(OptionValue::Int(i))
            }
            (OptionKind::Float { min, max }, v) => {
                let x = match v {
                    OptionValue::Float(f) => *f,
                    OptionValue::Int(i) => *i as f64,
                    OptionValue::Text(s) => s.trim().replace(',', ".").parse().map_err(|_| format!("nombre attendu, reçu « {s} »"))?,
                    other => return Err(format!("nombre attendu, reçu « {other} »")),
                };
                if !x.is_finite() || x < *min || x > *max {
                    return Err(format!("doit être compris entre {min} et {max}"));
                }
                Ok(OptionValue::Float(x))
            }
            (OptionKind::Choice { choices }, v) => {
                let s = v.to_string();
                if choices.iter().any(|c| c.value == s) {
                    Ok(OptionValue::Text(s))
                } else {
                    let list: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
                    Err(format!("valeur « {s} » invalide (attendu : {})", list.join(", ")))
                }
            }
            (OptionKind::Text { max_len }, v) => {
                let s = v.to_string();
                if s.chars().count() > *max_len {
                    return Err(format!("texte trop long ({max_len} caractères maximum)"));
                }
                if s.chars().any(|c| c.is_control()) {
                    return Err("caractères de contrôle interdits".into());
                }
                Ok(OptionValue::Text(s))
            }
            (OptionKind::Bool, other) => Err(format!("valeur booléenne attendue, reçu « {other} »")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionError {
    pub key: String,
    pub message: String,
}

impl fmt::Display for OptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "option « {} » : {}", self.key, self.message)
    }
}

impl std::error::Error for OptionError {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OptionValues(BTreeMap<String, OptionValue>);

impl OptionValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: &str, v: OptionValue) -> Self {
        self.set(key, v);
        self
    }

    pub fn set(&mut self, key: &str, v: OptionValue) {
        self.0.insert(key.to_owned(), v);
    }

    pub fn get(&self, key: &str) -> Option<&OptionValue> {
        self.0.get(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Analyse une affectation `clé=valeur` (CLI, profils texte).
    pub fn parse_assignment(&mut self, s: &str) -> Result<(), String> {
        let (k, v) = s.split_once('=').ok_or_else(|| format!("« {s} » : format attendu clé=valeur"))?;
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("« {s} » : clé vide"));
        }
        self.set(k, OptionValue::Text(v.trim().to_owned()));
        Ok(())
    }

    /// Applique les valeurs par défaut, convertit et valide selon `specs`.
    /// Les clés inconnues de ces specs sont ignorées (elles peuvent viser une autre étape).
    pub fn resolve(&self, specs: &[OptionSpec]) -> Result<OptionValues, OptionError> {
        let mut out = OptionValues::new();
        for spec in specs {
            let v = match self.0.get(&spec.key) {
                Some(v) => spec.coerce(v).map_err(|message| OptionError { key: spec.key.clone(), message })?,
                None => spec.default.clone(),
            };
            out.set(&spec.key, v);
        }
        Ok(out)
    }

    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.0.get(key) {
            Some(OptionValue::Bool(b)) => *b,
            _ => default,
        }
    }

    pub fn int_or(&self, key: &str, default: i64) -> i64 {
        match self.0.get(key) {
            Some(OptionValue::Int(i)) => *i,
            Some(OptionValue::Float(f)) => *f as i64,
            _ => default,
        }
    }

    pub fn float_or(&self, key: &str, default: f64) -> f64 {
        match self.0.get(key) {
            Some(OptionValue::Float(f)) => *f,
            Some(OptionValue::Int(i)) => *i as f64,
            _ => default,
        }
    }

    pub fn str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        match self.0.get(key) {
            Some(OptionValue::Text(s)) => s,
            _ => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_applies_defaults_and_validates() {
        let specs = vec![
            OptionSpec::int("quality", "Qualité", 1, 100, 90),
            OptionSpec::bool("strip", "Nettoyer", false),
            OptionSpec::choice("codec", "Codec", &[("h264", "H.264"), ("hevc", "H.265")], "h264"),
        ];
        let mut v = OptionValues::new();
        v.parse_assignment("quality=75").unwrap();
        v.parse_assignment("strip=oui").unwrap();
        v.parse_assignment("autre_etape=1").unwrap();
        let r = v.resolve(&specs).unwrap();
        assert_eq!(r.int_or("quality", 0), 75);
        assert!(r.bool_or("strip", false));
        assert_eq!(r.str_or("codec", ""), "h264");
        assert!(r.get("autre_etape").is_none());

        let mut bad = OptionValues::new();
        bad.parse_assignment("quality=500").unwrap();
        assert_eq!(bad.resolve(&specs).unwrap_err().key, "quality");
        let mut bad = OptionValues::new();
        bad.parse_assignment("codec=divx").unwrap();
        assert!(bad.resolve(&specs).is_err());
    }

    #[test]
    fn text_rejects_control_chars() {
        let s = OptionSpec::text("t", "T", 20, "");
        assert!(s.coerce(&OptionValue::Text("ok".into())).is_ok());
        assert!(s.coerce(&OptionValue::Text("a\nb".into())).is_err());
    }
}
