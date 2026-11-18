//! Moteur de données natif : JSON, NDJSON, YAML, TOML, CSV, TSV.
//!
//! Modèle pivot : `serde_json::Value` (ordre des clés conservé). Chaque conversion déclarée
//! « sans perte » est **vérifiée par aller-retour** : le fichier produit est relu et comparé
//! à la valeur écrite ; en cas d'écart, la conversion échoue au lieu d'être déclarée réussie.

use crate::text;
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, Fidelity, FormatId, OptionSpec, OptionValues,
    Progress, StepAssessment, StepOutput, StepQuery, StepTask, Verification,
};
use serde_json::{Map, Value};
use std::path::Path;

const FORMATS: &[&str] = &["json", "ndjson", "yaml", "toml", "csv", "tsv"];
/// Au-delà, le moteur natif refuse (tout est chargé en mémoire) ; DuckDB prendra le relais.
const MAX_INPUT: u64 = 512 * 1024 * 1024;

pub struct DataNative {
    info: EngineInfo,
    caps: Vec<ConversionCap>,
}

impl Default for DataNative {
    fn default() -> Self {
        Self::new()
    }
}

fn is_table(f: &str) -> bool {
    matches!(f, "csv" | "tsv")
}

fn declared(from: &str, to: &str) -> Fidelity {
    match (is_table(from), is_table(to)) {
        (true, true) => Fidelity::Lossless,
        (true, false) => Fidelity::Lossless,
        (false, true) => Fidelity::Depends,
        (false, false) if to == "toml" => Fidelity::Depends,
        (false, false) => Fidelity::Lossless,
    }
}

impl DataNative {
    pub fn new() -> Self {
        let keep = features(&["nesting", "types", "null", "datetime"]);
        let mut caps = Vec::new();
        for from in FORMATS {
            for to in FORMATS {
                if from == to && !matches!(*to, "json" | "csv") {
                    continue;
                }
                caps.push(ConversionCap::new(from, to, declared(from, to), &keep, 1));
            }
        }
        Self {
            info: EngineInfo {
                id: EngineId::new("data"),
                name: "Données (natif)".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                license: "MIT OR Apache-2.0".into(),
                homepage: String::new(),
                origin: EngineOrigin::Builtin,
                path: None,
                description: "JSON, NDJSON, YAML, TOML, CSV et TSV en Rust pur, avec détection d'encodage.".into(),
            },
            caps,
        }
    }
}

fn invalid(msg: impl Into<String>) -> EngineError {
    EngineError::InvalidInput(msg.into())
}

fn delimiter_of(name: &str, fallback: u8) -> u8 {
    match name {
        "comma" | "," => b',',
        "semicolon" | ";" => b';',
        "tab" => b'\t',
        "pipe" | "|" => b'|',
        _ => fallback,
    }
}

// ───────────── Lecture ─────────────

fn parse(format: &str, src: &str, opts: &OptionValues, notes: &mut Vec<String>) -> Result<Value, EngineError> {
    match format {
        "json" => serde_json::from_str(src).map_err(|e| invalid(format!("JSON invalide : {e}"))),
        "ndjson" => src
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| serde_json::from_str(l).map_err(|e| invalid(format!("ligne {} : JSON invalide : {e}", i + 1))))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        "yaml" => {
            let mut docs = Vec::new();
            for doc in serde_norway::Deserializer::from_str(src) {
                let v: Value = serde::Deserialize::deserialize(doc).map_err(|e| invalid(format!("YAML invalide : {e}")))?;
                docs.push(v);
            }
            match docs.len() {
                0 => Ok(Value::Null),
                1 => Ok(docs.remove(0)),
                n => {
                    notes.push(format!("{n} documents YAML regroupés dans un tableau"));
                    Ok(Value::Array(docs))
                }
            }
        }
        "toml" => {
            let table: toml::Table = src.parse().map_err(|e: toml::de::Error| invalid(format!("TOML invalide : {e}")))?;
            Ok(toml_to_json(toml::Value::Table(table)))
        }
        "csv" | "tsv" => {
            let auto = if format == "tsv" { b'\t' } else { sniff(src) };
            let delim = delimiter_of(opts.str_or("input_delimiter", "auto"), auto);
            read_table(src, delim, opts.bool_or("infer_types", false))
        }
        other => Err(EngineError::Unsupported(format!("lecture {other}"))),
    }
}

fn sniff(src: &str) -> u8 {
    let lines: Vec<&str> = src.lines().take(20).collect();
    chud_core::detect::sniff_delimiter(&lines).unwrap_or(b',')
}

fn read_table(src: &str, delim: u8, infer: bool) -> Result<Value, EngineError> {
    let mut rdr = csv::ReaderBuilder::new().delimiter(delim).flexible(true).from_reader(src.as_bytes());
    let mut headers: Vec<String> = Vec::new();
    for (i, h) in rdr.headers().map_err(|e| invalid(format!("en-tête CSV illisible : {e}")))?.iter().enumerate() {
        let mut name = if h.trim().is_empty() { format!("colonne_{}", i + 1) } else { h.to_owned() };
        let base = name.clone();
        let mut n = 2;
        while headers.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        headers.push(name);
    }
    let mut rows = Vec::new();
    for (line, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| invalid(format!("ligne {} : {e}", line + 2)))?;
        let mut obj = Map::new();
        for (i, field) in rec.iter().enumerate() {
            let key = headers.get(i).cloned().unwrap_or_else(|| format!("colonne_{}", i + 1));
            obj.insert(key, if infer { infer_value(field) } else { Value::String(field.to_owned()) });
        }
        rows.push(Value::Object(obj));
    }
    Ok(Value::Array(rows))
}

fn infer_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return Value::Null;
    }
    match t {
        "true" | "TRUE" | "True" => return Value::Bool(true),
        "false" | "FALSE" | "False" => return Value::Bool(false),
        _ => {}
    }
    // Les zéros non significatifs (« 007 ») restent du texte.
    let leading_zero = t.len() > 1 && t.starts_with('0') && !t.starts_with("0.");
    if !leading_zero {
        if let Ok(i) = t.parse::<i64>() {
            return Value::from(i);
        }
        if let Ok(f) = t.parse::<f64>()
            && f.is_finite()
        {
            return Value::from(f);
        }
    }
    Value::String(s.to_owned())
}

fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect()),
    }
}

fn json_to_toml(v: &Value, path: &str) -> Result<toml::Value, EngineError> {
    Ok(match v {
        Value::Null => {
            return Err(EngineError::Unsupported(format!("TOML ne peut pas représenter la valeur nulle en {path}")));
        }
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => toml::Value::Integer(i),
            (None, Some(f)) => toml::Value::Float(f),
            _ => return Err(EngineError::Unsupported(format!("nombre hors limites TOML en {path}"))),
        },
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(a) => {
            toml::Value::Array(a.iter().enumerate().map(|(i, x)| json_to_toml(x, &format!("{path}[{i}]"))).collect::<Result<_, _>>()?)
        }
        Value::Object(o) => {
            let mut t = toml::Table::new();
            for (k, x) in o {
                t.insert(k.clone(), json_to_toml(x, &format!("{path}.{k}"))?);
            }
            toml::Value::Table(t)
        }
    })
}

// ───────────── Écriture ─────────────

fn flatten_into(prefix: &str, v: &Value, out: &mut Map<String, Value>) {
    match v {
        Value::Object(o) if !o.is_empty() => {
            for (k, x) in o {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_into(&key, x, out);
            }
        }
        other => {
            out.insert(prefix.to_owned(), other.clone());
        }
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn write_table(v: &Value, delim: u8, flatten: bool, notes: &mut Vec<String>) -> Result<String, EngineError> {
    let Value::Array(items) = v else {
        return Err(EngineError::Unsupported("un tableau CSV nécessite une liste d'enregistrements à la racine".into()));
    };
    let mut rows: Vec<Map<String, Value>> = Vec::with_capacity(items.len());
    let mut nested = false;
    for item in items {
        let mut row = Map::new();
        match item {
            Value::Object(o) if flatten => flatten_into("", &Value::Object(o.clone()), &mut row),
            Value::Object(o) => row = o.clone(),
            scalar => {
                row.insert("value".into(), scalar.clone());
            }
        }
        nested |= row.values().any(|x| x.is_object() || x.is_array());
        rows.push(row);
    }
    if nested {
        notes.push("valeurs imbriquées écrites en JSON dans les cellules".into());
    }
    let mut headers: Vec<String> = Vec::new();
    for r in &rows {
        for k in r.keys() {
            if !headers.contains(k) {
                headers.push(k.clone());
            }
        }
    }
    let mut w = csv::WriterBuilder::new().delimiter(delim).from_writer(Vec::new());
    w.write_record(&headers).map_err(|e| EngineError::Other(e.to_string()))?;
    for r in &rows {
        let rec: Vec<String> = headers.iter().map(|h| r.get(h).map(cell).unwrap_or_default()).collect();
        w.write_record(&rec).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    let bytes = w.into_inner().map_err(|e| EngineError::Other(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| EngineError::Other(e.to_string()))
}

fn serialize(format: &str, v: &Value, opts: &OptionValues, notes: &mut Vec<String>) -> Result<String, EngineError> {
    let other = |e: &dyn std::fmt::Display| EngineError::Other(e.to_string());
    match format {
        "json" => {
            let indent = opts.int_or("indent", 2).clamp(0, 8) as usize;
            if indent == 0 {
                serde_json::to_string(v).map_err(|e| other(&e))
            } else {
                let ind = vec![b' '; indent];
                let mut buf = Vec::new();
                let fmt = serde_json::ser::PrettyFormatter::with_indent(&ind);
                let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
                serde::Serialize::serialize(v, &mut ser).map_err(|e| other(&e))?;
                buf.push(b'\n');
                String::from_utf8(buf).map_err(|e| other(&e))
            }
        }
        "ndjson" => {
            let items: Vec<&Value> = match v {
                Value::Array(a) => a.iter().collect(),
                single => vec![single],
            };
            let mut s = String::new();
            for it in items {
                s.push_str(&serde_json::to_string(it).map_err(|e| other(&e))?);
                s.push('\n');
            }
            Ok(s)
        }
        "yaml" => serde_norway::to_string(v).map_err(|e| other(&e)),
        "toml" => {
            let t = json_to_toml(v, "$")?;
            let toml::Value::Table(table) = t else {
                return Err(EngineError::Unsupported("la racine d'un document TOML doit être un objet".into()));
            };
            toml::to_string_pretty(&table).map_err(|e| other(&e))
        }
        "csv" => write_table(v, delimiter_of(opts.str_or("delimiter", "comma"), b','), opts.bool_or("flatten", true), notes),
        "tsv" => write_table(v, b'\t', opts.bool_or("flatten", true), notes),
        f => Err(EngineError::Unsupported(format!("écriture {f}"))),
    }
}

impl Engine for DataNative {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        let mut o = Vec::new();
        if is_table(from.as_str()) {
            if from.as_str() == "csv" {
                o.push(
                    OptionSpec::choice(
                        "input_delimiter",
                        "Séparateur du fichier source",
                        &[
                            ("auto", "Détection automatique"),
                            ("comma", "Virgule"),
                            ("semicolon", "Point-virgule"),
                            ("tab", "Tabulation"),
                            ("pipe", "Barre verticale"),
                        ],
                        "auto",
                    )
                    .advanced(),
                );
            }
            if !is_table(to.as_str()) {
                o.push(
                    OptionSpec::bool("infer_types", "Deviner les types (nombres, booléens)", false)
                        .help("Désactivé par défaut : toutes les cellules restent du texte, ce qui est sans perte."),
                );
            }
        }
        match to.as_str() {
            "json" => o.push(OptionSpec::int("indent", "Indentation (0 = compact)", 0, 8, 2)),
            "csv" => {
                o.push(OptionSpec::choice(
                    "delimiter",
                    "Séparateur",
                    &[
                        ("comma", "Virgule"),
                        ("semicolon", "Point-virgule (Excel français)"),
                        ("tab", "Tabulation"),
                        ("pipe", "Barre verticale"),
                    ],
                    "comma",
                ));
                o.push(OptionSpec::bool("bom", "Ajouter un BOM UTF-8 (compatibilité Excel)", false));
                o.push(OptionSpec::bool("flatten", "Aplatir les objets imbriqués (a.b)", true).advanced());
            }
            "tsv" => {
                o.push(OptionSpec::bool("bom", "Ajouter un BOM UTF-8 (compatibilité Excel)", false));
                o.push(OptionSpec::bool("flatten", "Aplatir les objets imbriqués (a.b)", true).advanced());
            }
            _ => {}
        }
        o
    }

    fn assess(&self, q: &StepQuery<'_>) -> StepAssessment {
        let mut fidelity = declared(q.from.as_str(), q.to.as_str());
        let mut notes = Vec::new();
        if q.options.bool_or("infer_types", false) {
            fidelity = Fidelity::Lossy;
            notes.push("typage automatique : « 1.50 » devient 1.5, « TRUE » devient true".into());
        }
        if is_table(q.to.as_str()) && !is_table(q.from.as_str()) {
            notes.push("CSV/TSV : toutes les valeurs deviennent du texte".into());
        }
        StepAssessment { fidelity, notes, unavailable: None, preserves: None }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let size = std::fs::metadata(&task.input)?.len();
        if size > MAX_INPUT {
            return Err(EngineError::Refused(format!(
                "fichier de {} Mo : le moteur natif est limité à {} Mo",
                size / 1_048_576,
                MAX_INPUT / 1_048_576
            )));
        }
        let bytes = std::fs::read(&task.input)?;
        let decoded = text::decode(&bytes);
        let mut notes = Vec::new();
        if decoded.encoding != "UTF-8" {
            notes.push(format!("encodage source {} converti en UTF-8", decoded.encoding));
        }
        if decoded.lossy {
            notes.push("octets invalides remplacés lors du décodage".into());
        }
        ctx.report(Progress { fraction: Some(0.2), message: Some("analyse".into()), ..Default::default() });
        let value = parse(task.from.as_str(), &decoded.text, &task.options, &mut notes)?;
        ctx.check_cancelled()?;
        ctx.report(Progress { fraction: Some(0.6), message: Some("écriture".into()), ..Default::default() });
        let mut out = serialize(task.to.as_str(), &value, &task.options, &mut notes)?;

        // Vérification d'aller-retour pour les formats structurés.
        if matches!(task.to.as_str(), "json" | "ndjson" | "yaml" | "toml") {
            let mut sink = Vec::new();
            let back = parse(task.to.as_str(), &out, &OptionValues::new(), &mut sink)?;
            let expected = match (task.to.as_str(), &value) {
                ("ndjson", Value::Array(_)) => value.clone(),
                ("ndjson", single) => Value::Array(vec![single.clone()]),
                _ => value.clone(),
            };
            if !same_value(&back, &expected) {
                return Err(EngineError::Other("vérification d'aller-retour échouée : le résultat relu diffère des données source".into()));
            }
            notes.push("aller-retour vérifié : les données relues sont identiques".into());
        }
        if is_table(task.to.as_str()) && task.options.bool_or("bom", false) {
            out.insert(0, '\u{feff}');
        }
        std::fs::write(&task.output, out)?;
        ctx.report(Progress { fraction: Some(1.0), ..Default::default() });
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        let Ok(bytes) = std::fs::read(output) else { return Verification::Failed("fichier illisible".into()) };
        let decoded = text::decode(&bytes);
        let mut sink = Vec::new();
        let mut opts = OptionValues::new();
        if task.to.as_str() == "csv" {
            opts.set("input_delimiter", chud_core::OptionValue::Text(task.options.str_or("delimiter", "comma").to_owned()));
        }
        match parse(task.to.as_str(), &decoded.text, &opts, &mut sink) {
            Ok(v) => {
                let n = match &v {
                    Value::Array(a) => format!("{} éléments", a.len()),
                    Value::Object(o) => format!("{} clés", o.len()),
                    _ => "valeur scalaire".into(),
                };
                Verification::Decoded(format!("ré-analysé ({n})"))
            }
            Err(e) => Verification::Failed(format!("le fichier produit ne se relit pas : {e}")),
        }
    }
}

/// Égalité de valeurs tolérante sur la représentation des nombres (1.0 == 1).
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => p == q,
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_value(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_value(v, w))),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chud_core::OptionValue;

    fn run(from: &str, to: &str, input: &[u8], opts: OptionValues) -> Result<(String, StepOutput), EngineError> {
        let e = DataNative::new();
        let dir = std::env::temp_dir().join(format!("chud-data-{}-{from}-{to}-{}", std::process::id(), input.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let inp = dir.join(format!("in.{from}"));
        let out = dir.join(format!("out.{to}"));
        std::fs::write(&inp, input).unwrap();
        let specs = e.options(&from.into(), &to.into());
        let task = StepTask {
            input: inp,
            output: out.clone(),
            from: from.into(),
            to: to.into(),
            options: opts.resolve(&specs).unwrap(),
            source: None,
        };
        let ctx = ExecContext::new(dir.clone());
        let r = e.execute(&task, &ctx)?;
        assert!(e.verify(&out, &task, &ctx).is_ok());
        Ok((std::fs::read_to_string(&out).unwrap(), r))
    }

    #[test]
    fn json_yaml_toml_roundtrip_keeps_order_and_values() {
        let src = r#"{"zeta": 1, "alpha": {"liste": [1, 2.5, "trois"], "ok": true}, "texte": "é à \"guillemets\""}"#.as_bytes();
        let (yaml, out) = run("json", "yaml", src, OptionValues::new()).unwrap();
        assert!(yaml.find("zeta").unwrap() < yaml.find("alpha").unwrap(), "ordre des clés conservé");
        assert!(out.notes.iter().any(|n| n.contains("aller-retour")));
        let (json, _) = run("yaml", "json", yaml.as_bytes(), OptionValues::new()).unwrap();
        let a: Value = serde_json::from_slice(src).unwrap();
        let b: Value = serde_json::from_str(&json).unwrap();
        assert!(same_value(&a, &b));
        let (toml, _) = run("json", "toml", src, OptionValues::new()).unwrap();
        assert!(toml.contains("zeta = 1"));
    }

    #[test]
    fn toml_refuses_null_with_path() {
        let err = run("json", "toml", br#"{"a": {"b": null}}"#, OptionValues::new()).unwrap_err();
        assert!(err.to_string().contains("$.a.b"), "{err}");
    }

    #[test]
    fn french_excel_csv_in_windows_1252() {
        let src = b"Nom;Montant;Code\nZo\xe9;1,50;007\nL\xe9a;12;042\n";
        let (json, out) = run("csv", "json", src, OptionValues::new()).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["Nom"], "Zoé");
        assert_eq!(v[0]["Code"], "007", "texte conservé tel quel, sans perte");
        assert!(out.notes.iter().any(|n| n.contains("encodage source")));
    }

    #[test]
    fn infer_types_keeps_leading_zeros() {
        let opts = OptionValues::new().with("infer_types", OptionValue::Bool(true));
        let (json, _) = run("csv", "json", b"a,b,c\n007,12,true\n", opts).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["a"], "007");
        assert_eq!(v[0]["b"], 12);
        assert_eq!(v[0]["c"], true);
    }

    #[test]
    fn json_to_csv_flattens_and_bom() {
        let opts = OptionValues::new().with("delimiter", OptionValue::Text("semicolon".into())).with("bom", OptionValue::Bool(true));
        let (csv, _) = run("json", "csv", br#"[{"id":1,"pos":{"x":2,"y":3}},{"id":2,"extra":"z"}]"#, opts).unwrap();
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("id;pos.x;pos.y;extra"), "{csv}");
    }

    #[test]
    fn ndjson_roundtrip() {
        let (nd, _) = run("json", "ndjson", br#"[{"a":1},{"a":2}]"#, OptionValues::new()).unwrap();
        assert_eq!(nd, "{\"a\":1}\n{\"a\":2}\n");
        let (json, _) = run("ndjson", "json", nd.as_bytes(), OptionValues::new().with("indent", OptionValue::Int(0))).unwrap();
        assert_eq!(json, r#"[{"a":1},{"a":2}]"#);
    }

    #[test]
    fn invalid_input_is_reported() {
        let err = run("json", "yaml", b"{ pas du json", OptionValues::new()).unwrap_err();
        assert!(matches!(err, EngineError::InvalidInput(_)));
    }
}
