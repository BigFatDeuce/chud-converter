//! Opérations qpdf sur les pages : fusionner, diviser, pivoter.
//!
//! - Le contenu des pages n'est jamais décompressé ni recompressé.
//! - Les signets sont reconstruits : qpdf ne garde que ceux du premier fichier d'une fusion, et
//!   laisse des signets vers des pages absentes dans un extrait. Ils sont lus en JSON, renumérotés
//!   puis réécrits (`--update-from-json`, qpdf 11 ou plus récent).
//! - Chaque partie d'une division repart d'un document vide : ni balises, ni pièces jointes, ni
//!   titres de chapitres des autres pages n'y restent cachés.
//! - Tout ce qui ne suit pas (balises d'accessibilité, pièces jointes, liens nommés, signature) est
//!   détecté fichier par fichier et annoncé avant.

use crate::qpdf::{Qpdf, highest_page, valid_range};
use crate::tool::{self, Stage};
use chud_core::operation::{PDF_MERGE, PDF_ROTATE, PDF_SPLIT};
use chud_core::{EngineError, ExecContext, OperationReport, OperationTask, OptionSpec, OptionValues, StepOutput, Verification, tr, trf};
use chud_process::ProcessSpec;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Nombre maximal de fichiers produits par une division.
const MAX_PARTS: usize = 5000;
const JSON_LIMIT: usize = 256 * 1024 * 1024;
/// Profondeur maximale des signets lus (un PDF piégé peut en imbriquer des milliers).
const MAX_DEPTH: usize = 40;

/// Signet : titre, page (à partir de 1), position dans la page et sous-signets.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    pub title: String,
    pub page: Option<u32>,
    /// Suite de la destination après la page (« /XYZ », gauche, haut, zoom).
    pub view: Vec<Value>,
    pub kids: Vec<Node>,
}

/// Ce que l'on sait d'un PDF avant d'y toucher.
#[derive(Clone, Debug, Default)]
pub(crate) struct Doc {
    pub pages: u32,
    pub encrypted: bool,
    /// `false` : qpdf trop ancien pour décrire le document (avant la version 11).
    pub described: bool,
    pub outlines: Vec<Node>,
    pub attachments: bool,
    pub signed: bool,
    pub tagged: bool,
    pub named_links: bool,
}

type DocCache = Mutex<HashMap<PathBuf, (SystemTime, u64, Arc<Doc>)>>;

#[derive(Default)]
pub(crate) struct Cache(DocCache);

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_ref(s: &str) -> bool {
    s.strip_suffix(" R").is_some_and(|r| {
        let mut it = r.split(' ');
        let num = |x: Option<&str>| x.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        num(it.next()) && num(it.next()) && it.next().is_none()
    })
}

/// « 34 0 R » → « 34,0 », forme attendue par `--json-object`.
fn object_arg(r: &str) -> Option<String> {
    is_ref(r).then(|| format!("--json-object={}", r.trim_end_matches(" R").replace(' ', ",")))
}

fn parse_outlines(v: &Value, depth: usize) -> Vec<Node> {
    let Some(list) = v.as_array().filter(|_| depth < MAX_DEPTH) else { return Vec::new() };
    list.iter()
        .map(|o| Node {
            title: o["title"].as_str().unwrap_or_default().to_owned(),
            page: o["destpageposfrom1"].as_u64().and_then(|n| u32::try_from(n).ok()),
            view: o["dest"]
                .as_array()
                .filter(|d| d.first().and_then(Value::as_str).is_some_and(is_ref))
                .map(|d| {
                    d[1..]
                        .iter()
                        .take(6)
                        .filter(|x| x.is_number() || x.is_null() || x.as_str().is_some_and(|s| s.starts_with('/')))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            kids: parse_outlines(&o["kids"], depth + 1),
        })
        .collect()
}

pub(crate) fn count_nodes(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.kids)).sum()
}

/// Signets d'un extrait : pages renumérotées ; un signet dont la page n'est plus là disparaît, sauf
/// s'il regroupe des signets encore utiles (il mène alors au premier d'entre eux).
pub(crate) fn remap(nodes: &[Node], map: &dyn Fn(u32) -> Option<u32>) -> Vec<Node> {
    nodes
        .iter()
        .filter_map(|n| {
            let kids = remap(&n.kids, map);
            match n.page.and_then(map) {
                Some(p) => Some(Node { title: n.title.clone(), page: Some(p), view: n.view.clone(), kids }),
                None if !kids.is_empty() => Some(Node { title: n.title.clone(), page: first_page(&kids), view: Vec::new(), kids }),
                None => None,
            }
        })
        .collect()
}

fn first_page(nodes: &[Node]) -> Option<u32> {
    nodes.iter().find_map(|n| n.page.or_else(|| first_page(&n.kids)))
}

/// Pages désignées par une plage déjà validée, dans l'ordre écrit (« 5-3 » compte à rebours).
pub(crate) fn expand_range(range: &str, total: u32) -> Vec<u32> {
    let num = |s: &str| if s == "z" { total } else { s.parse().unwrap_or(0) };
    let mut out = Vec::new();
    for part in range.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (num(a), num(b));
                if a <= b {
                    out.extend(a..=b);
                } else {
                    out.extend((b..=a).rev());
                }
            }
            None => out.push(num(part)),
        }
    }
    out
}

/// Parties d'une division : plage qpdf et étiquette du nom de fichier.
pub(crate) fn split_plan(options: &OptionValues, total: u32) -> Result<Vec<(String, String)>, EngineError> {
    let invalid = |key: &str, message: String| EngineError::InvalidOption { key: key.into(), message };
    let ranges = options.str_or("ranges", "").replace(' ', "");
    let mut parts: Vec<(String, String)> = Vec::new();
    if ranges.is_empty() {
        let every = options.int_or("every", 1).clamp(1, 100_000) as u32;
        if every >= total {
            return Err(invalid(
                "every",
                trf!(
                    "le document n'a que {total} page(s) : il n'y a rien à diviser",
                    "the document only has {total} page(s): there is nothing to split"
                ),
            ));
        }
        let w = total.to_string().len();
        let mut first = 1;
        while first <= total {
            let last = (first + every - 1).min(total);
            parts.push(if first == last {
                (first.to_string(), format!("p{first:0w$}"))
            } else {
                (format!("{first}-{last}"), format!("p{first:0w$}-{last:0w$}"))
            });
            first = last + 1;
        }
    } else {
        for r in ranges.split(';').filter(|r| !r.is_empty()) {
            if !valid_range(r) {
                return Err(invalid(
                    "ranges",
                    trf!("plage invalide « {r} » (exemple : 1-3; 4-6; 7-z)", "invalid range “{r}” (example: 1-3; 4-6; 7-z)"),
                ));
            }
            if highest_page(r) > total {
                return Err(invalid(
                    "ranges",
                    trf!("« {r} » : le document n'a que {total} page(s)", "“{r}”: the document only has {total} page(s)"),
                ));
            }
            let mut label: String = format!("p{}", r.replace('z', &total.to_string())).chars().take(60).collect();
            let taken = |l: &str| parts.iter().any(|(_, x)| x == l);
            if taken(&label) {
                let n = (2..).find(|n| !taken(&format!("{label} ({n})"))).unwrap_or(2);
                label = format!("{label} ({n})");
            }
            parts.push((r.to_owned(), label));
        }
        if parts.is_empty() {
            return Err(invalid("ranges", tr!("aucune plage", "no range").into()));
        }
    }
    if parts.len() > MAX_PARTS {
        return Err(invalid(
            "every",
            trf!("{} fichiers : au-delà de la limite de {MAX_PARTS}", "{} files: over the limit of {MAX_PARTS}", parts.len()),
        ));
    }
    Ok(parts)
}

/// Écrit l'arbre des signets en objets PDF (JSON qpdf). Renvoie la référence de la racine.
fn outline_objects(nodes: &[Node], pages: &[String], next_id: &mut u64, objs: &mut Map<String, Value>) -> String {
    fn level(nodes: &[Node], parent: &str, pages: &[String], next_id: &mut u64, objs: &mut Map<String, Value>) -> Option<(String, String)> {
        let refs: Vec<String> = nodes
            .iter()
            .map(|_| {
                *next_id += 1;
                format!("{next_id} 0 R")
            })
            .collect();
        for (i, n) in nodes.iter().enumerate() {
            let mut d = Map::new();
            d.insert("/Title".into(), json!(format!("u:{}", n.title)));
            d.insert("/Parent".into(), json!(parent));
            if i > 0 {
                d.insert("/Prev".into(), json!(refs[i - 1]));
            }
            if i + 1 < refs.len() {
                d.insert("/Next".into(), json!(refs[i + 1]));
            }
            if let Some(page) = n.page.and_then(|p| pages.get(p as usize - 1)) {
                let mut dest = vec![json!(page)];
                if n.view.is_empty() {
                    dest.push(json!("/Fit"));
                } else {
                    dest.extend(n.view.iter().cloned());
                }
                d.insert("/Dest".into(), Value::Array(dest));
            }
            if let Some((first, last)) = level(&n.kids, &refs[i], pages, next_id, objs) {
                d.insert("/First".into(), json!(first));
                d.insert("/Last".into(), json!(last));
                // Négatif : replié à l'ouverture.
                d.insert("/Count".into(), json!(-(n.kids.len() as i64)));
            }
            objs.insert(format!("obj:{}", refs[i]), json!({ "value": d }));
        }
        Some((refs.first()?.clone(), refs.last()?.clone()))
    }
    *next_id += 1;
    let root = format!("{next_id} 0 R");
    let mut d = Map::new();
    d.insert("/Type".into(), json!("/Outlines"));
    if let Some((first, last)) = level(nodes, &root, pages, next_id, objs) {
        d.insert("/First".into(), json!(first));
        d.insert("/Last".into(), json!(last));
        d.insert("/Count".into(), json!(nodes.len()));
    }
    objs.insert(format!("obj:{root}"), json!({ "value": d }));
    root
}

/// Informations du document d'origine à remettre dans un extrait.
#[derive(Default)]
struct Extras {
    info: Option<Map<String, Value>>,
    lang: Option<Value>,
    xmp: Option<Value>,
}

struct Catalog {
    header: Map<String, Value>,
    trailer: Map<String, Value>,
    root: String,
    dict: Map<String, Value>,
}

impl Qpdf {
    fn json(&self, path: &Path, args: &[String]) -> Option<Value> {
        let spec = ProcessSpec::new(&self.exe)
            .arg("--json=2")
            .args(args.iter())
            .arg(path)
            .capture_stdout(JSON_LIMIT)
            .timeout(Some(Duration::from_secs(300)));
        let out = chud_process::run_capture(&spec, JSON_LIMIT).ok()?;
        if !matches!(out.code, Some(0 | 3)) || out.stdout_truncated {
            return None;
        }
        serde_json::from_str(&out.stdout_text()).ok()
    }

    /// Valeurs des objets demandés (« 34 0 R »).
    fn objects(&self, path: &Path, refs: &[&str], extra: &[&str]) -> Option<Map<String, Value>> {
        let mut args = vec!["--json-key=qpdf".to_owned()];
        args.extend(extra.iter().map(|s| (*s).to_owned()));
        for r in refs {
            args.push(object_arg(r)?);
        }
        self.json(path, &args)?["qpdf"].get(1)?.as_object().cloned()
    }

    fn catalog(&self, path: &Path) -> Option<Catalog> {
        let t = self.json(path, &["--json-key=qpdf".into(), "--json-object=trailer".into()])?;
        let header = t["qpdf"].get(0)?.as_object()?.clone();
        let trailer = t["qpdf"].get(1)?["trailer"]["value"].as_object()?.clone();
        let root = trailer.get("/Root")?.as_str()?.to_owned();
        let dict = self.objects(path, &[&root], &[])?.get(&format!("obj:{root}"))?["value"].as_object()?.clone();
        Some(Catalog { header, trailer, root, dict })
    }

    /// Valeur directe, ou celle de l'objet désigné.
    fn resolve(&self, path: &Path, v: &Value) -> Option<Value> {
        match v.as_str() {
            Some(r) if is_ref(r) => Some(self.objects(path, &[r], &[])?.get(&format!("obj:{r}"))?["value"].clone()),
            _ => Some(v.clone()),
        }
    }

    fn page_refs(&self, path: &Path) -> Option<Vec<String>> {
        let v = self.json(path, &["--json-key=pages".into()])?;
        v["pages"].as_array()?.iter().map(|p| p["object"].as_str().map(str::to_owned)).collect()
    }

    /// Analyse un PDF (résultat gardé tant que le fichier ne change pas).
    pub(crate) fn doc(&self, path: &Path) -> Result<Arc<Doc>, EngineError> {
        let meta = std::fs::metadata(path)?;
        let stamp = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
        let cached = self.docs.0.lock().unwrap_or_else(|e| e.into_inner()).get(path).cloned();
        if let Some((t, n, d)) = cached
            && (t, n) == stamp
        {
            return Ok(d);
        }
        let doc = Arc::new(self.analyze(path)?);
        let mut g = self.docs.0.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() > 2000 {
            g.clear();
        }
        g.insert(path.to_path_buf(), (stamp.0, stamp.1, doc.clone()));
        Ok(doc)
    }

    fn analyze(&self, path: &Path) -> Result<Doc, EngineError> {
        let name = name_of(path);
        let spec = ProcessSpec::new(&self.exe).arg("--requires-password").arg(path).timeout(Some(Duration::from_secs(120)));
        let code = chud_process::run_capture(&spec, 4096).map_err(crate::util::map_process_error)?.code;
        if code == Some(0) {
            return Err(EngineError::Refused(trf!("« {name} » est protégé par mot de passe", "“{name}” is password-protected")));
        }
        let pages = self.pages(path).filter(|n| *n > 0).ok_or_else(|| {
            EngineError::InvalidInput(trf!("« {name} » : PDF illisible ou endommagé", "“{name}”: unreadable or damaged PDF"))
        })?;
        let mut d = Doc { pages, encrypted: code == Some(3), ..Default::default() };
        let keys = ["--json-key=outlines", "--json-key=acroform", "--json-key=attachments"].map(String::from);
        let Some(v) = self.json(path, &keys) else { return Ok(d) };
        d.described = true;
        d.outlines = parse_outlines(&v["outlines"], 0);
        d.attachments = v["attachments"].as_object().is_some_and(|a| !a.is_empty());
        d.signed = v["acroform"]["fields"]
            .as_array()
            .is_some_and(|f| f.iter().any(|x| x["fieldtype"].as_str() == Some("/Sig") && !x["value"].is_null()));
        if let Some(c) = self.catalog(path) {
            d.tagged = c.dict.contains_key("/StructTreeRoot");
            d.named_links = c.dict.contains_key("/Dests")
                || c.dict.get("/Names").and_then(|n| self.resolve(path, n)).is_some_and(|n| n.get("/Dests").is_some());
        }
        Ok(d)
    }

    /// Analyse plusieurs PDF à la fois, dans l'ordre donné.
    fn analyze_all(&self, inputs: &[PathBuf]) -> Result<Vec<Arc<Doc>>, EngineError> {
        type Slot = Mutex<Option<Result<Arc<Doc>, EngineError>>>;
        let slots: Vec<Slot> = inputs.iter().map(|_| Mutex::new(None)).collect();
        parallel(inputs.len(), &|i| *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(self.doc(&inputs[i])));
        slots
            .into_iter()
            .map(|s| {
                s.into_inner().unwrap_or_else(|e| e.into_inner()).unwrap_or_else(|| Err(EngineError::Other("analyse interrompue".into())))
            })
            .collect()
    }

    /// Signets voulus pour une fusion, d'après l'option « bookmarks ».
    fn merged_outline(inputs: &[PathBuf], docs: &[Arc<Doc>], mode: &str) -> Vec<Node> {
        let mut out = Vec::new();
        let mut offset = 0;
        for (p, d) in inputs.iter().zip(docs) {
            let own = remap(&d.outlines, &|n| (n <= d.pages).then_some(n + offset));
            match mode {
                "none" => {}
                "keep" => out.extend(own),
                _ => out.push(Node { title: stem_of(p), page: Some(offset + 1), view: Vec::new(), kids: own }),
            }
            offset += d.pages;
        }
        out
    }

    pub(crate) fn op_options(&self, op: &str) -> Vec<OptionSpec> {
        match op {
            PDF_MERGE => vec![OptionSpec::choice(
                "bookmarks",
                tr!("Signets", "Bookmarks"),
                &[
                    ("files", tr!("Un par fichier, avec ses propres signets dedans", "One per file, with its own bookmarks inside")),
                    ("keep", tr!("Ceux des fichiers, à la suite", "Those of the files, one after the other")),
                    ("none", tr!("Aucun", "None")),
                ],
                "files",
            )],
            PDF_SPLIT => vec![
                OptionSpec::int("every", tr!("Pages par fichier", "Pages per file"), 1, 100_000, 1),
                OptionSpec::text("ranges", tr!("Ou vos plages, une par fichier", "Or your own ranges, one per file"), 2000, "").help(tr!(
                    "Exemple : 1-3; 4-6; 7-z (« z » : dernière page). Remplace le choix ci-dessus.",
                    "Example: 1-3; 4-6; 7-z (“z”: last page). Replaces the choice above."
                )),
            ],
            PDF_ROTATE => vec![
                OptionSpec::choice(
                    "angle",
                    tr!("Rotation", "Rotation"),
                    &[
                        ("90", tr!("90° vers la droite", "90° clockwise")),
                        ("180", tr!("180° (tête en bas)", "180° (upside down)")),
                        ("270", tr!("90° vers la gauche", "90° counterclockwise")),
                    ],
                    "90",
                ),
                OptionSpec::text("pages", tr!("Pages à pivoter ; vide : toutes", "Pages to rotate; empty: all"), 200, "")
                    .help(tr!("Exemple : 1-3,5 (« z » : dernière page)", "Example: 1-3,5 (“z”: last page)")),
            ],
            _ => Vec::new(),
        }
    }

    fn rotate_range(options: &OptionValues, pages: u32) -> Result<Option<String>, EngineError> {
        let r = options.str_or("pages", "").replace(' ', "");
        if r.is_empty() {
            return Ok(None);
        }
        let invalid = |message: String| EngineError::InvalidOption { key: "pages".into(), message };
        if !valid_range(&r) {
            return Err(invalid(tr!("plage invalide (exemples : 1-3, 2,4,6, 5-z)", "invalid range (examples: 1-3, 2,4,6, 5-z)").into()));
        }
        if highest_page(&r) > pages {
            return Err(invalid(trf!("le document n'a que {pages} page(s)", "the document only has {pages} page(s)")));
        }
        Ok(Some(r))
    }

    /// Refus d'un PDF dont l'auteur a limité l'usage : réunir ou diviser produirait un fichier sans
    /// ces limites.
    fn refuse_encrypted(path: &Path, d: &Doc) -> Result<(), EngineError> {
        if d.encrypted {
            return Err(EngineError::Refused(trf!(
                "« {} » est protégé : son auteur en limite l'usage (chiffrement)",
                "“{}” is protected: its author restricts its use (encryption)",
                name_of(path)
            )));
        }
        Ok(())
    }

    pub(crate) fn op_assess(&self, op: &str, inputs: &[PathBuf], options: &OptionValues) -> Result<OperationReport, EngineError> {
        let docs = self.analyze_all(inputs)?;
        let mut r = OperationReport::default();
        let names = |pick: &dyn Fn(&Doc) -> bool, skip: usize| -> Option<String> {
            let v: Vec<String> = inputs.iter().zip(&docs).skip(skip).filter(|(_, d)| pick(d)).map(|(p, _)| name_of(p)).collect();
            let shown = v.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            match v.len() {
                0 => None,
                n if n > 3 => Some(trf!("{shown} et {} autre(s)", "{shown} and {} more", n - 3)),
                _ => Some(shown),
            }
        };
        if let Some(n) = names(&|d| d.signed, 0) {
            r.lost.push(trf!(
                "la signature numérique, qui ne vaut que pour le fichier d'origine ({n})",
                "the digital signature, only valid for the original file ({n})"
            ));
        }
        if let Some(n) = names(&|d| !d.described, 0) {
            r.notes.push(trf!(
                "qpdf est trop ancien (version 11 ou plus récente nécessaire) pour examiner ce que perd {n} et pour reprendre ses signets",
                "qpdf is too old (version 11 or later needed) to examine what {n} loses and to carry its bookmarks over"
            ));
        }
        match op {
            PDF_MERGE => {
                for (p, d) in inputs.iter().zip(&docs) {
                    Self::refuse_encrypted(p, d)?;
                }
                let total: u32 = docs.iter().map(|d| d.pages).sum();
                r.notes.insert(0, trf!("{} fichiers, {total} pages", "{} files, {total} pages", docs.len()));
                r.notes.push(
                    tr!(
                        "titre, auteur et autres informations du document : ceux du premier fichier",
                        "title, author and other document information: those of the first file"
                    )
                    .into(),
                );
                if let Some(n) = names(&|d| d.tagged, 1) {
                    r.lost.push(trf!("les balises d'accessibilité ({n})", "the accessibility tags ({n})"));
                }
                if let Some(n) = names(&|d| d.attachments, 1) {
                    r.lost.push(trf!("les pièces jointes ({n})", "the attachments ({n})"));
                }
                if let Some(n) = names(&|d| d.named_links, 1) {
                    r.lost.push(trf!(
                        "les liens internes nommés, souvent ceux de la table des matières ({n})",
                        "the named internal links, often those of the table of contents ({n})"
                    ));
                }
                if options.str_or("bookmarks", "files") == "none"
                    && let Some(n) = names(&|d| !d.outlines.is_empty(), 0)
                {
                    r.lost.push(trf!("les signets ({n})", "the bookmarks ({n})"));
                }
            }
            PDF_SPLIT => {
                for (p, d) in inputs.iter().zip(&docs) {
                    Self::refuse_encrypted(p, d)?;
                    let parts = split_plan(options, d.pages)?;
                    r.notes.insert(
                        0,
                        trf!("{} pages → {} fichiers PDF dans un dossier", "{} pages → {} PDF files in a folder", d.pages, parts.len()),
                    );
                }
                if names(&|d| d.tagged, 0).is_some() {
                    r.lost.push(tr!("les balises d'accessibilité", "the accessibility tags").into());
                }
                if names(&|d| d.attachments, 0).is_some() {
                    r.lost.push(tr!("les pièces jointes", "the attachments").into());
                }
                if names(&|d| d.named_links, 0).is_some() {
                    r.lost.push(
                        tr!(
                            "les liens internes nommés, souvent ceux de la table des matières",
                            "the named internal links, often those of the table of contents"
                        )
                        .into(),
                    );
                }
            }
            PDF_ROTATE => {
                for d in &docs {
                    let n = match Self::rotate_range(options, d.pages)? {
                        Some(range) => expand_range(&range, d.pages).into_iter().collect::<std::collections::BTreeSet<_>>().len() as u32,
                        None => d.pages,
                    };
                    r.notes.insert(0, trf!("{n} page(s) pivotée(s) sur {}", "{n} of {} page(s) rotated", d.pages));
                }
            }
            _ => return Err(EngineError::Unsupported(op.into())),
        }
        Ok(r)
    }

    pub(crate) fn op_execute(&self, task: &OperationTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        // Mêmes refus que l'aperçu : rien n'est écrit pour un fichier protégé ou illisible.
        let report = self.op_assess(&task.operation, &task.inputs, &task.options)?;
        match task.operation.as_str() {
            PDF_MERGE => self.merge(task, ctx)?,
            PDF_SPLIT => self.split(task, ctx)?,
            PDF_ROTATE => self.rotate(task, ctx)?,
            o => return Err(EngineError::Unsupported(o.into())),
        }
        tool::report_fraction(ctx, 1.0);
        Ok(StepOutput { notes: report.notes })
    }

    fn run(&self, spec: ProcessSpec, ctx: &ExecContext) -> Result<(), EngineError> {
        // 3 : réussi avec des avertissements (petites erreurs de l'original réparées).
        tool::run_ok(spec, ctx, &[3], &mut |_| {}).map(|_| ())
    }

    fn merge(&self, task: &OperationTask, ctx: &ExecContext) -> Result<(), EngineError> {
        let stage = Stage::new(ctx, "merge")?;
        for (i, p) in task.inputs.iter().enumerate() {
            stage.input(p, &format!("in{i}.pdf"))?;
        }
        ctx.check_cancelled()?;
        tool::report_fraction(ctx, 0.1);
        let mut spec = ProcessSpec::new(&self.exe).args(["--object-streams=generate", "in0.pdf", "--pages", ".", "1-z"]);
        for i in 1..task.inputs.len() {
            spec = spec.arg(format!("in{i}.pdf")).arg("1-z");
        }
        self.run(spec.args(["--", "merged.pdf"]).cwd(&stage.dir), ctx)?;
        tool::report_fraction(ctx, 0.7);
        let docs = self.analyze_all(&task.inputs)?;
        let produced = if docs.iter().all(|d| d.described) {
            let outline = Self::merged_outline(&task.inputs, &docs, task.options.str_or("bookmarks", "files"));
            self.finish(&stage, "merged.pdf", "out.pdf", &outline, None, ctx)?
        } else {
            stage.path("merged.pdf")
        };
        tool::deliver(&produced, &task.output, "qpdf")
    }

    /// Remplace les signets (et, pour un extrait, remet les informations du document).
    fn finish(
        &self,
        stage: &Stage,
        from: &str,
        to: &str,
        outline: &[Node],
        extras: Option<&Extras>,
        ctx: &ExecContext,
    ) -> Result<PathBuf, EngineError> {
        let src = stage.path(from);
        let unreadable = || EngineError::Other(tr!("qpdf n'a pas pu relire son résultat", "qpdf could not read its result back").into());
        let mut c = self.catalog(&src).ok_or_else(unreadable)?;
        let had = c.dict.contains_key("/Outlines");
        let nothing = extras.is_none_or(|e| e.info.is_none() && e.lang.is_none() && e.xmp.is_none());
        if outline.is_empty() && !had && nothing {
            return Ok(src);
        }
        let mut next = c.header.get("maxobjectid").and_then(Value::as_u64).ok_or_else(unreadable)?;
        let mut objs = Map::new();
        c.dict.remove("/Outlines");
        if outline.is_empty() {
            c.dict.remove("/PageMode");
        } else {
            let pages = self.page_refs(&src).ok_or_else(unreadable)?;
            let root = outline_objects(outline, &pages, &mut next, &mut objs);
            c.dict.insert("/Outlines".into(), json!(root));
            // Le volet des signets s'ouvre avec le document.
            c.dict.insert("/PageMode".into(), json!("/UseOutlines"));
        }
        if let Some(e) = extras {
            if let Some(info) = &e.info {
                next += 1;
                objs.insert(format!("obj:{next} 0 R"), json!({ "value": info }));
                c.trailer.insert("/Info".into(), json!(format!("{next} 0 R")));
                objs.insert("trailer".into(), json!({ "value": c.trailer }));
            }
            if let Some(lang) = &e.lang {
                c.dict.insert("/Lang".into(), lang.clone());
            }
            if let Some(xmp) = &e.xmp {
                next += 1;
                objs.insert(format!("obj:{next} 0 R"), json!({ "stream": xmp }));
                c.dict.insert("/Metadata".into(), json!(format!("{next} 0 R")));
            }
        }
        objs.insert(format!("obj:{}", c.root), json!({ "value": c.dict }));
        c.header.insert("maxobjectid".into(), json!(next));
        let doc = json!({ "version": 2, "parameters": { "decodelevel": "generalized" }, "qpdf": [c.header, objs] });
        let upd = format!("{to}.json");
        std::fs::write(stage.path(&upd), serde_json::to_vec(&doc).map_err(|e| EngineError::Other(e.to_string()))?)?;
        let spec = ProcessSpec::new(&self.exe)
            .arg("--object-streams=generate")
            .arg(from)
            .arg(format!("--update-from-json={upd}"))
            .arg(to)
            .cwd(&stage.dir);
        self.run(spec, ctx)?;
        let _ = std::fs::remove_file(stage.path(&upd));
        Ok(stage.path(to))
    }

    /// Titre, auteur, langue et métadonnées XMP d'un document, pour ses extraits.
    fn extras(&self, path: &Path) -> Extras {
        let Some(c) = self.catalog(path) else { return Extras::default() };
        let info = c.trailer.get("/Info").and_then(|i| self.resolve(path, i)).and_then(|v| {
            let m: Map<String, Value> = v
                .as_object()?
                .iter()
                .filter(|(_, v)| v.as_str().is_some_and(|s| s.starts_with("u:") || s.starts_with("b:")))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (!m.is_empty()).then_some(m)
        });
        let xmp = c.dict.get("/Metadata").and_then(Value::as_str).and_then(|r| {
            let o = self.objects(path, &[r], &["--json-stream-data=inline"])?;
            let s = o.get(&format!("obj:{r}"))?["stream"].as_object()?;
            // Données décompressées par qpdf ; un filtre ou une clé indirecte n'a pas de sens ailleurs.
            let dict: Map<String, Value> = s["dict"]
                .as_object()?
                .iter()
                .filter(|(k, v)| k.as_str() != "/Length" && !v.as_str().is_some_and(is_ref))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Some(json!({ "dict": dict, "data": s["data"].as_str()? }))
        });
        Extras { info, lang: c.dict.get("/Lang").filter(|l| l.is_string()).cloned(), xmp }
    }

    fn split(&self, task: &OperationTask, ctx: &ExecContext) -> Result<(), EngineError> {
        let input = one_input(task)?;
        let doc = self.doc(input)?;
        let parts = split_plan(&task.options, doc.pages)?;
        let stage = Stage::new(ctx, "split")?;
        stage.input(input, "input.pdf")?;
        std::fs::create_dir_all(&task.output)?;
        let extras = if doc.described { self.extras(&stage.path("input.pdf")) } else { Extras::default() };
        let done = AtomicUsize::new(0);
        let failure: Mutex<Option<EngineError>> = Mutex::new(None);
        let stem = short_stem(input);
        parallel(parts.len(), &|i| {
            let failed = failure.lock().unwrap_or_else(|e| e.into_inner()).is_some();
            if failed || ctx.cancel.is_cancelled() {
                return;
            }
            let (range, label) = &parts[i];
            let r = (|| {
                let raw = format!("raw{i}.pdf");
                let spec = ProcessSpec::new(&self.exe)
                    .args(["--object-streams=generate", "--empty", "--pages", "input.pdf"])
                    .arg(range)
                    .args(["--", &raw])
                    .cwd(&stage.dir);
                self.run(spec, ctx)?;
                let produced = if doc.described {
                    let outline = part_outline(&doc, range);
                    self.finish(&stage, &raw, &format!("out{i}.pdf"), &outline, Some(&extras), ctx)?
                } else {
                    stage.path(&raw)
                };
                tool::deliver(&produced, &task.output.join(format!("{stem} - {label}.pdf")), "qpdf")?;
                let _ = std::fs::remove_file(stage.path(&raw));
                Ok(())
            })();
            match r {
                Ok(()) => {
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    tool::report_fraction(ctx, n as f64 / parts.len() as f64 * 0.8);
                }
                Err(e) => {
                    failure.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                }
            }
        });
        ctx.check_cancelled()?;
        match failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn rotate(&self, task: &OperationTask, ctx: &ExecContext) -> Result<(), EngineError> {
        let input = one_input(task)?;
        let pages = self.doc(input)?.pages;
        let stage = Stage::new(ctx, "rotate")?;
        stage.input(input, "input.pdf")?;
        let angle = rotate_angle(&task.options);
        let arg = match Self::rotate_range(&task.options, pages)? {
            Some(r) => format!("--rotate=+{angle}:{r}"),
            None => format!("--rotate=+{angle}"),
        };
        let spec = ProcessSpec::new(&self.exe).arg("--object-streams=generate").arg("input.pdf").arg(arg).arg("out.pdf").cwd(&stage.dir);
        self.run(spec, ctx)?;
        tool::deliver(&stage.path("out.pdf"), &task.output, "qpdf")
    }

    /// Rotation effective (0, 90, 180, 270) des premières pages, héritage compris.
    fn rotations(&self, path: &Path, limit: usize) -> Option<Vec<i64>> {
        let refs: Vec<String> = self.page_refs(path)?.into_iter().take(limit).collect();
        let mut known: HashMap<String, Value> = HashMap::new();
        let mut wanted: Vec<String> = refs.clone();
        // Remonte les parents, niveau par niveau.
        for _ in 0..MAX_DEPTH {
            wanted.sort();
            wanted.dedup();
            wanted.retain(|r| !known.contains_key(r));
            if wanted.is_empty() {
                break;
            }
            let mut parents = Vec::new();
            for chunk in wanted.chunks(200) {
                let names: Vec<&str> = chunk.iter().map(String::as_str).collect();
                let objs = self.objects(path, &names, &[])?;
                for r in chunk {
                    let v = objs.get(&format!("obj:{r}"))?["value"].clone();
                    if v.get("/Rotate").is_none()
                        && let Some(p) = v.get("/Parent").and_then(Value::as_str)
                    {
                        parents.push(p.to_owned());
                    }
                    known.insert(r.clone(), v);
                }
            }
            wanted = parents;
        }
        refs.iter()
            .map(|r| {
                let mut at = r.clone();
                for _ in 0..MAX_DEPTH {
                    let v = known.get(&at)?;
                    if let Some(a) = v.get("/Rotate").and_then(Value::as_i64) {
                        return Some(a.rem_euclid(360));
                    }
                    match v.get("/Parent").and_then(Value::as_str) {
                        Some(p) => at = p.to_owned(),
                        None => return Some(0),
                    }
                }
                Some(0)
            })
            .collect()
    }

    /// `qpdf --check` puis nombre de pages.
    fn check(&self, path: &Path, ctx: &ExecContext) -> Result<u32, String> {
        let spec = ProcessSpec::new(&self.exe).arg("--check").arg(path);
        let last = std::cell::RefCell::new(String::new());
        let o = crate::util::run_tool(spec, ctx, &mut |l| *last.borrow_mut() = l.to_owned(), &mut |l| *last.borrow_mut() = l.to_owned())
            .map_err(|e| e.to_string())?;
        if !matches!(o.code, Some(0 | 3)) {
            return Err(trf!("qpdf --check a trouvé des erreurs : {}", "qpdf --check found errors: {}", last.into_inner()));
        }
        self.pages(path).ok_or_else(|| tr!("nombre de pages illisible", "unreadable page count").to_owned())
    }

    pub(crate) fn op_verify(&self, task: &OperationTask, ctx: &ExecContext) -> Verification {
        let r = match task.operation.as_str() {
            PDF_MERGE => self.verify_merge(task, ctx),
            PDF_SPLIT => self.verify_split(task, ctx),
            PDF_ROTATE => self.verify_rotate(task, ctx),
            _ => return Verification::SignatureOnly,
        };
        match r {
            Ok(s) => Verification::Structural(s),
            Err(e) => Verification::Failed(e),
        }
    }

    fn verify_merge(&self, task: &OperationTask, ctx: &ExecContext) -> Result<String, String> {
        let docs = self.analyze_all(&task.inputs).map_err(|e| e.to_string())?;
        let expected: u32 = docs.iter().map(|d| d.pages).sum();
        let got = self.check(&task.output, ctx)?;
        if got != expected {
            return Err(trf!("{got} pages au lieu de {expected}", "{got} pages instead of {expected}"));
        }
        let marks = if docs.iter().all(|d| d.described) {
            let want = Self::merged_outline(&task.inputs, &docs, task.options.str_or("bookmarks", "files"));
            self.check_outline(&task.output, &want)?
        } else {
            0
        };
        Ok(trf!(
            "qpdf --check : structure valide, {got} pages (le compte est exact), {marks} signet(s) relu(s)",
            "qpdf --check: valid structure, {got} pages (exact count), {marks} bookmark(s) read back"
        ))
    }

    /// Les signets écrits sont relus : mêmes titres, mêmes pages, même imbrication.
    fn check_outline(&self, path: &Path, want: &[Node]) -> Result<usize, String> {
        fn same(a: &[Node], b: &[Node]) -> bool {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.title == y.title && x.page == y.page && same(&x.kids, &y.kids))
        }
        let v = self.json(path, &["--json-key=outlines".into()]);
        let got = v.map(|v| parse_outlines(&v["outlines"], 0)).unwrap_or_default();
        if same(&got, want) {
            return Ok(count_nodes(want));
        }
        Err(trf!(
            "signets différents de ceux attendus ({} au lieu de {})",
            "bookmarks differ from the expected ones ({} instead of {})",
            count_nodes(&got),
            count_nodes(want)
        ))
    }

    fn verify_split(&self, task: &OperationTask, ctx: &ExecContext) -> Result<String, String> {
        let input = one_input(task).map_err(|e| e.to_string())?;
        let doc = self.doc(input).map_err(|e| e.to_string())?;
        let parts = split_plan(&task.options, doc.pages).map_err(|e| e.to_string())?;
        let stem = short_stem(input);
        let produced = std::fs::read_dir(&task.output).map_err(|e| e.to_string())?.count();
        if produced != parts.len() {
            return Err(trf!("{produced} fichiers au lieu de {}", "{produced} files instead of {}", parts.len()));
        }
        let error: Mutex<Option<String>> = Mutex::new(None);
        let total = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        parallel(parts.len(), &|i| {
            if ctx.cancel.is_cancelled() || error.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                return;
            }
            let (range, label) = &parts[i];
            let file = task.output.join(format!("{stem} - {label}.pdf"));
            let want = expand_range(range, doc.pages).len() as u32;
            let r = self.check(&file, ctx).and_then(|n| {
                if n != want {
                    return Err(trf!("{label} : {n} pages au lieu de {want}", "{label}: {n} pages instead of {want}"));
                }
                if doc.described {
                    self.check_outline(&file, &part_outline(&doc, range)).map_err(|e| format!("{label} : {e}"))?;
                }
                Ok(n)
            });
            match r {
                Ok(n) => {
                    total.fetch_add(n as usize, Ordering::Relaxed);
                    let k = done.fetch_add(1, Ordering::Relaxed) + 1;
                    tool::report_fraction(ctx, 0.8 + 0.2 * k as f64 / parts.len() as f64);
                }
                Err(e) => {
                    error.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                }
            }
        });
        if let Some(e) = error.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(e);
        }
        Ok(trf!(
            "{} fichiers contrôlés un par un (qpdf --check), {} pages en tout, signets compris",
            "{} files checked one by one (qpdf --check), {} pages in all, bookmarks included",
            parts.len(),
            total.into_inner()
        ))
    }

    fn verify_rotate(&self, task: &OperationTask, ctx: &ExecContext) -> Result<String, String> {
        const LIMIT: usize = 500;
        let input = one_input(task).map_err(|e| e.to_string())?;
        let doc = self.doc(input).map_err(|e| e.to_string())?;
        let got = self.check(&task.output, ctx)?;
        if got != doc.pages {
            return Err(trf!("{got} pages au lieu de {}", "{got} pages instead of {}", doc.pages));
        }
        let (Some(before), Some(after)) = (self.rotations(input, LIMIT), self.rotations(&task.output, LIMIT)) else {
            return Ok(trf!("qpdf --check : structure valide, {got} pages", "qpdf --check: valid structure, {got} pages"));
        };
        let chosen = match Self::rotate_range(&task.options, doc.pages).map_err(|e| e.to_string())? {
            Some(r) => expand_range(&r, doc.pages),
            None => (1..=doc.pages).collect(),
        };
        let angle = rotate_angle(&task.options);
        for (i, (b, a)) in before.iter().zip(&after).enumerate() {
            let n = i as u32 + 1;
            let want = if chosen.contains(&n) { (b + angle).rem_euclid(360) } else { *b };
            if *a != want {
                return Err(trf!("page {n} : orientation {a}° au lieu de {want}°", "page {n}: orientation {a}° instead of {want}°"));
            }
        }
        Ok(trf!(
            "qpdf --check : structure valide, {got} pages, orientation relue sur {} page(s)",
            "qpdf --check: valid structure, {got} pages, orientation read back on {} page(s)",
            after.len()
        ))
    }
}

fn rotate_angle(options: &OptionValues) -> i64 {
    match options.str_or("angle", "90") {
        "180" => 180,
        "270" => 270,
        _ => 90,
    }
}

fn one_input(task: &OperationTask) -> Result<&Path, EngineError> {
    task.inputs.first().map(PathBuf::as_path).ok_or_else(|| EngineError::InvalidInput(tr!("aucun fichier", "no file").into()))
}

/// Nom de base des parties : raccourci, car il est répété dans le nom du dossier.
fn short_stem(p: &Path) -> String {
    let s: String = stem_of(p).chars().take(80).collect();
    s.trim_end().to_owned()
}

/// Signets d'une partie : renumérotés selon les pages gardées (première apparition de chacune).
fn part_outline(doc: &Doc, range: &str) -> Vec<Node> {
    let mut pos: HashMap<u32, u32> = HashMap::new();
    for (i, p) in expand_range(range, doc.pages).into_iter().enumerate() {
        pos.entry(p).or_insert(i as u32 + 1);
    }
    remap(&doc.outlines, &|n| pos.get(&n).copied())
}

/// Quelques qpdf à la fois : chacun est court, mais il peut y en avoir des centaines.
fn parallel(count: usize, f: &(dyn Fn(usize) + Sync)) {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8).min(count.max(1));
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= count {
                        break;
                    }
                    f(i);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chud_core::OptionValue;

    fn n(title: &str, page: Option<u32>, kids: Vec<Node>) -> Node {
        Node { title: title.into(), page, view: Vec::new(), kids }
    }

    #[test]
    fn ranges_expand_in_order() {
        assert_eq!(expand_range("1-3,5", 9), [1, 2, 3, 5]);
        assert_eq!(expand_range("7-z", 9), [7, 8, 9]);
        assert_eq!(expand_range("3-1", 9), [3, 2, 1]);
        assert_eq!(expand_range("z", 4), [4]);
    }

    #[test]
    fn split_plans() {
        let every = |k: i64, t: u32| split_plan(&OptionValues::new().with("every", OptionValue::Int(k)), t);
        let p = every(1, 12).unwrap();
        assert_eq!(p.len(), 12);
        assert_eq!(p[0], ("1".into(), "p01".into()));
        let p = every(5, 12).unwrap();
        assert_eq!(p.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["1-5", "6-10", "11-12"]);
        assert_eq!(p[2].1, "p11-12");
        assert!(every(1, 1).is_err(), "une seule page");
        assert!(every(3, 3).is_err());
        assert!(every(1, 6000).is_err(), "trop de fichiers");
        let ranges = |r: &str| split_plan(&OptionValues::new().with("ranges", OptionValue::Text(r.into())), 9);
        assert_eq!(ranges("1-3; 4-z").unwrap()[1], ("4-z".into(), "p4-9".into()));
        assert_eq!(ranges("2;2").unwrap()[1].1, "p2 (2)");
        for bad in ["1-12", "a", "1--2", ";;", "0"] {
            assert!(ranges(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bookmarks_follow_their_pages() {
        let tree = vec![
            n("A", Some(1), vec![]),
            n("B", Some(2), vec![n("B1", Some(2), vec![]), n("B2", Some(4), vec![])]),
            n("lien web", None, vec![]),
        ];
        // Extrait des pages 4 et 5 : B ne garde que B2 et mène à lui.
        let out = remap(&tree, &|p| (p >= 4).then(|| p - 3));
        assert_eq!(out, vec![n("B", Some(1), vec![n("B2", Some(1), vec![])])]);
        assert_eq!(count_nodes(&out), 2);
    }

    #[test]
    fn references_are_strict() {
        assert!(is_ref("34 0 R"));
        for bad in ["34 R", "a 0 R", "34 0 R 1", "/XYZ", "", " 0 R"] {
            assert!(!is_ref(bad), "{bad}");
        }
        assert_eq!(object_arg("34 0 R").as_deref(), Some("--json-object=34,0"));
    }
}
