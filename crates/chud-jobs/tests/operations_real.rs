//! Opérations réelles (fusionner, diviser, pivoter, archiver) sur des fichiers générés, aux noms
//! piégés, relus ensuite par un chemin indépendant.
//!
//! Sans qpdf ou 7-Zip, le test concerné est ignoré (« SKIP »), sauf si `CHUD_REQUIRE_TOOLS` est
//! défini. `CHUD_TEST_ENGINES_DATA` : voir `matrix_real.rs`.

use chud_core::operation::{ARCHIVE_CREATE, PDF_MERGE, PDF_ROTATE, PDF_SPLIT};
use chud_core::{CancelToken, OptionValue, Registry, Verification};
use chud_enginemgr::{AppPaths, EngineManager, EngineState};
use chud_jobs::{Conflict, JobError, JobOutcome, JobRunner, JobSpec, OperationSpec};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Env {
    runner: JobRunner,
    dir: PathBuf,
    tools: BTreeMap<String, PathBuf>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `None` : moteur absent (test ignoré).
fn env(name: &str, needs: &[&str]) -> Option<Env> {
    let dir = std::env::temp_dir().join(format!("chud-ops-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = AppPaths::at(&dir.join("app"));
    let discovery = match std::env::var_os("CHUD_TEST_ENGINES_DATA").map(PathBuf::from) {
        Some(d) => AppPaths { config_dir: d.join("config"), data_dir: d, cache_dir: paths.cache_dir.clone(), portable: true },
        None => paths.clone(),
    };
    let mgr = EngineManager::new(discovery).unwrap();
    let mut statuses = mgr.discover();
    let registry: Registry = mgr.build_registry(&mut statuses);
    let tools: BTreeMap<String, PathBuf> =
        statuses.iter().filter(|s| s.state == EngineState::Ready).filter_map(|s| Some((s.id.clone(), s.path.clone()?))).collect();
    let e = Env { runner: JobRunner::new(Arc::new(registry), paths.cache_dir.clone()), dir, tools };
    for n in needs {
        if !e.tools.contains_key(*n) {
            assert!(std::env::var_os("CHUD_REQUIRE_TOOLS").is_none(), "{n} absent alors que CHUD_REQUIRE_TOOLS est défini");
            eprintln!("SKIP : {n} absent");
            return None;
        }
    }
    Some(e)
}

type Mark = (&'static str, usize, Option<(&'static str, usize)>);

/// PDF minimal et valide : une ligne de texte par page (« marque-N »), signets, titre, et au choix
/// une pièce jointe, des balises ou des liens nommés.
#[derive(Default)]
struct Pdf {
    pages: usize,
    mark: &'static str,
    /// (titre, page à partir de 1, sous-signet éventuel)
    outline: Vec<Mark>,
    title: Option<&'static str>,
    tagged: bool,
    named: bool,
    attachment: bool,
    /// Rotation héritée (posée sur le nœud parent des pages).
    rotate: Option<i32>,
}

impl Pdf {
    fn write(&self, path: &Path) {
        let mut objs: Vec<String> = Vec::new(); // objet n = indice + 1
        let page0 = 4; // 1 catalogue, 2 pages, 3 police
        let page_id = |i: usize| page0 + i * 2;
        let after_pages = page0 + self.pages * 2;
        let flat: usize = self.outline.iter().map(|o| 1 + usize::from(o.2.is_some())).sum();
        let outline_id = after_pages;
        let mut next = after_pages + if flat > 0 { 1 + flat } else { 0 };
        let mut catalog = String::from("/Type /Catalog /Pages 2 0 R /Lang (fr-FR)");
        let mut extra: Vec<(usize, String)> = Vec::new();
        if flat > 0 {
            catalog += &format!(" /Outlines {outline_id} 0 R");
        }
        if self.tagged {
            catalog += &format!(" /MarkInfo << /Marked true >> /StructTreeRoot {next} 0 R");
            extra.push((next, "<< /Type /StructTreeRoot /Alt (secret-structure) >>".into()));
            next += 1;
        }
        let mut names = String::new();
        if self.named {
            names += &format!(" /Dests << /Names [(ch1) [{} 0 R /Fit]] >>", page_id(0));
        }
        if self.attachment {
            names += &format!(" /EmbeddedFiles << /Names [(joint.txt) {next} 0 R] >>");
            extra.push((next, format!("<< /Type /Filespec /F (joint.txt) /UF (joint.txt) /EF << /F {} 0 R >> >>", next + 1)));
            extra.push((next + 1, "<< /Type /EmbeddedFile /Length 12 >>\nstream\npiece-jointe\nendstream".into()));
            next += 2;
        }
        if !names.is_empty() {
            catalog += &format!(" /Names <<{names} >>");
        }
        let info_id = next;
        objs.push(format!("<< {catalog} >>"));
        let kids: Vec<String> = (0..self.pages).map(|i| format!("{} 0 R", page_id(i))).collect();
        let rot = self.rotate.map(|r| format!(" /Rotate {r}")).unwrap_or_default();
        objs.push(format!("<< /Type /Pages /Count {} /Kids [{}] /MediaBox [0 0 300 400]{rot} >>", self.pages, kids.join(" ")));
        objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        for i in 0..self.pages {
            objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", page_id(i) + 1));
            let s = format!("BT /F1 20 Tf 40 300 Td ({}-{}) Tj ET", self.mark, i + 1);
            objs.push(format!("<< /Length {} >>\nstream\n{s}\nendstream", s.len()));
        }
        if flat > 0 {
            let tops: Vec<usize> = {
                let mut id = outline_id + 1;
                self.outline
                    .iter()
                    .map(|o| {
                        let t = id;
                        id += 1 + usize::from(o.2.is_some());
                        t
                    })
                    .collect()
            };
            objs.push(format!("<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>", tops[0], tops[tops.len() - 1], tops.len()));
            for (k, (title, page, kid)) in self.outline.iter().enumerate() {
                let mut d = format!("/Title ({title}) /Parent {outline_id} 0 R /Dest [{} 0 R /XYZ 40 320 0]", page_id(page - 1));
                if k > 0 {
                    d += &format!(" /Prev {} 0 R", tops[k - 1]);
                }
                if k + 1 < tops.len() {
                    d += &format!(" /Next {} 0 R", tops[k + 1]);
                }
                if kid.is_some() {
                    d += &format!(" /First {0} 0 R /Last {0} 0 R /Count -1", tops[k] + 1);
                }
                objs.push(format!("<< {d} >>"));
                if let Some((t, p)) = kid {
                    objs.push(format!("<< /Title ({t}) /Parent {} 0 R /Dest [{} 0 R /Fit] >>", tops[k], page_id(p - 1)));
                }
            }
        }
        for (id, body) in extra {
            assert_eq!(objs.len() + 1, id);
            objs.push(body);
        }
        if let Some(t) = self.title {
            assert_eq!(objs.len() + 1, info_id);
            objs.push(format!("<< /Title ({t}) /Author (Essai) >>"));
        }
        let mut out = String::from("%PDF-1.7\n");
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
        }
        let xref = out.len();
        out += &format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
        for o in offsets {
            out += &format!("{o:010} 00000 n \n");
        }
        let info = if self.title.is_some() { format!(" /Info {info_id} 0 R") } else { String::new() };
        out += &format!("trailer\n<< /Size {} /Root 1 0 R{info} >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1);
        std::fs::write(path, out).unwrap();
    }
}

impl Env {
    fn run(&self, op: &str, files: &[&Path], set: &[(&str, &str)]) -> Result<JobOutcome, JobError> {
        self.run_with(op, files, set, |_| {})
    }

    fn run_with(
        &self,
        op: &str,
        files: &[&Path],
        set: &[(&str, &str)],
        f: impl FnOnce(&mut OperationSpec),
    ) -> Result<JobOutcome, JobError> {
        let mut s = OperationSpec::new(op, files.iter().map(|p| p.to_path_buf()).collect());
        for (k, v) in set {
            s.options.set(k, OptionValue::Text((*v).to_owned()));
        }
        f(&mut s);
        let before: Vec<(PathBuf, Option<Vec<u8>>)> = files.iter().map(|p| (p.to_path_buf(), std::fs::read(p).ok())).collect();
        let r = self.runner.run_operation(&s, &CancelToken::new(), &|_| {});
        for (p, b) in before {
            assert_eq!(std::fs::read(&p).ok(), b, "{} modifié", p.display());
        }
        let jobs = self.dir.join("app").join("cache").join("jobs");
        if let Ok(rd) = std::fs::read_dir(&jobs) {
            assert_eq!(rd.count(), 0, "dossier de travail abandonné");
        }
        if let Ok(o) = &r {
            assert!(o.verifications.iter().all(|v| matches!(v, Verification::Structural(_) | Verification::Decoded(_))));
            eprintln!("{op} → {:?}\n   {:?}\n   {:?}", o.output, o.report.as_ref().map(|r| r.details()), o.verifications);
        }
        r
    }

    fn qpdf(&self, args: &[&str], file: &Path) -> Vec<u8> {
        let o = std::process::Command::new(&self.tools["qpdf"]).args(args).arg(file).output().unwrap();
        assert!(matches!(o.status.code(), Some(0 | 3)), "qpdf {args:?} : {}", String::from_utf8_lossy(&o.stderr));
        o.stdout
    }

    /// Texte brut du PDF décompressé, pour y chercher des marques.
    fn raw(&self, file: &Path) -> String {
        let o = std::process::Command::new(&self.tools["qpdf"])
            .args(["--qdf", "--object-streams=disable"])
            .arg(file)
            .arg("-")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// Marques dans l'ordre des pages (« a-1 », « b-2 »…).
    fn marks(&self, file: &Path) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_slice(&self.qpdf(&["--json=2", "--json-key=pages"], file)).unwrap();
        v["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let c = p["contents"][0].as_str().unwrap().trim_end_matches(" R").replace(' ', ",");
                let o = std::process::Command::new(&self.tools["qpdf"])
                    .args([&format!("--show-object={c}"), "--filtered-stream-data"])
                    .arg(file)
                    .output()
                    .unwrap();
                let s = String::from_utf8_lossy(&o.stdout).into_owned();
                s.split('(').nth(1).and_then(|x| x.split(')').next()).unwrap_or("?").to_owned()
            })
            .collect()
    }

    /// Signets « titre@page », imbrication par des espaces.
    fn outline(&self, file: &Path) -> Vec<String> {
        fn walk(v: &serde_json::Value, depth: usize, out: &mut Vec<String>) {
            for o in v.as_array().unwrap() {
                out.push(format!("{}{}@{}", "  ".repeat(depth), o["title"].as_str().unwrap(), o["destpageposfrom1"]));
                walk(&o["kids"], depth + 1, out);
            }
        }
        let v: serde_json::Value = serde_json::from_slice(&self.qpdf(&["--json=2", "--json-key=outlines"], file)).unwrap();
        let mut out = Vec::new();
        walk(&v["outlines"], 0, &mut out);
        out
    }
}

#[test]
fn merge_keeps_order_bookmarks_and_says_what_is_lost() {
    let Some(e) = env("merge", &["qpdf"]) else { return };
    let a = e.dir.join("é [1] %d.pdf");
    let b = e.dir.join("@b 文.pdf");
    Pdf {
        pages: 3,
        mark: "a",
        outline: vec![("A1", 1, None), ("A2", 2, Some(("A2 bis", 3)))],
        title: Some("Premier"),
        ..Default::default()
    }
    .write(&a);
    Pdf { pages: 2, mark: "b", outline: vec![("B1", 2, None)], tagged: true, named: true, attachment: true, ..Default::default() }
        .write(&b);

    let o = e.run(PDF_MERGE, &[&a, &b, &a], &[]).unwrap();
    let out = o.output.clone().unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "é [1] %d - fusion.pdf");
    assert_eq!(e.marks(&out), ["a-1", "a-2", "a-3", "b-1", "b-2", "a-1", "a-2", "a-3"]);
    assert_eq!(
        e.outline(&out),
        ["é [1] %d@1", "  A1@1", "  A2@2", "    A2 bis@3", "@b 文@4", "  B1@5", "é [1] %d@6", "  A1@6", "  A2@7", "    A2 bis@8"]
    );
    let raw = e.raw(&out);
    assert!(raw.contains("/Title (Premier)"), "titre du premier fichier");
    // La position dans la page est gardée, pas seulement la page.
    assert!(raw.contains("320"), "destination /XYZ d'origine");
    let lost = o.report.unwrap().lost.join(" | ");
    for word in ["balises", "pièces jointes", "liens internes"] {
        assert!(lost.contains(word), "{word} : {lost}");
    }

    let keep = e.run(PDF_MERGE, &[&b, &a], &[("bookmarks", "keep")]).unwrap();
    assert_eq!(e.outline(&keep.output.clone().unwrap()), ["B1@2", "A1@3", "A2@4", "  A2 bis@5"]);
    assert!(keep.report.unwrap().lost.is_empty(), "b est le premier : balises, pièces jointes et liens restent");
    let raw = e.raw(&keep.output.unwrap());
    assert!(raw.contains("piece-jointe") && raw.contains("secret-structure"));

    let none = e.run(PDF_MERGE, &[&a, &a], &[("bookmarks", "none")]).unwrap();
    assert!(e.outline(&none.output.unwrap()).is_empty());
    assert!(none.report.unwrap().lost.iter().any(|l| l.contains("signets")));
}

#[test]
fn split_produces_clean_parts() {
    let Some(e) = env("split", &["qpdf"]) else { return };
    let a = e.dir.join("Rapport é.pdf");
    Pdf {
        pages: 5,
        mark: "p",
        outline: vec![("Un", 1, None), ("Deux", 2, Some(("Deux bis", 4))), ("Cinq", 5, None)],
        title: Some("Titre du rapport"),
        tagged: true,
        attachment: true,
        ..Default::default()
    }
    .write(&a);

    let o = e.run(PDF_SPLIT, &[&a], &[("every", "2")]).unwrap();
    let dir = o.output.unwrap();
    assert_eq!(dir.file_name().unwrap().to_string_lossy(), "Rapport é - pages");
    let mut files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["Rapport é - p1-2.pdf", "Rapport é - p3-4.pdf", "Rapport é - p5.pdf"]);
    assert_eq!(e.marks(&dir.join(&files[1])), ["p-3", "p-4"]);
    assert_eq!(e.outline(&dir.join(&files[0])), ["Un@1", "Deux@2"]);
    assert_eq!(e.outline(&dir.join(&files[1])), ["Deux@2", "  Deux bis@2"]);
    assert_eq!(e.outline(&dir.join(&files[2])), ["Cinq@1"]);
    for f in &files {
        let raw = e.raw(&dir.join(f));
        assert!(raw.contains("/Title (Titre du rapport)"), "{f} : titre repris");
        assert!(raw.contains("/Lang (fr-FR)"), "{f} : langue reprise");
        // Rien des autres pages ne reste caché dans la partie.
        assert!(!raw.contains("secret-structure") && !raw.contains("piece-jointe"), "{f} : restes cachés");
        assert_eq!(raw.matches("/Type /Page\n").count() as u32, e.marks(&dir.join(f)).len() as u32, "{f}");
    }
    assert!(!e.raw(&dir.join(&files[0])).contains("Cinq"), "aucun titre de chapitre d'une autre partie");

    let o = e.run(PDF_SPLIT, &[&a], &[("ranges", "5; 3-1 ; 2,z")]).unwrap();
    let dir = o.output.unwrap();
    assert_eq!(dir.file_name().unwrap().to_string_lossy(), "Rapport é - pages (1)", "le premier dossier est gardé");
    assert_eq!(e.marks(&dir.join("Rapport é - p3-1.pdf")), ["p-3", "p-2", "p-1"]);
    assert_eq!(e.marks(&dir.join("Rapport é - p2,5.pdf")), ["p-2", "p-5"]);

    for (k, v, code) in [("every", "5", "invalid_option"), ("ranges", "1-9", "invalid_option"), ("ranges", "x", "invalid_option")] {
        assert_eq!(e.run(PDF_SPLIT, &[&a], &[(k, v)]).unwrap_err().code(), code, "{k}={v}");
    }
}

#[test]
fn a_large_split_checks_every_part() {
    let Some(e) = env("bigsplit", &["qpdf"]) else { return };
    let a = e.dir.join("gros.pdf");
    Pdf { pages: 150, mark: "g", outline: vec![("Début", 1, None), ("Fin", 150, None)], ..Default::default() }.write(&a);
    let o = e.run(PDF_SPLIT, &[&a], &[]).unwrap();
    let dir = o.output.unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 150);
    assert_eq!(e.marks(&dir.join("gros - p077.pdf")), ["g-77"]);
}

#[test]
fn rotate_all_or_some_pages() {
    let Some(e) = env("rotate", &["qpdf"]) else { return };
    let a = e.dir.join("a.pdf");
    // Rotation héritée du parent : elle doit s'ajouter, page par page.
    Pdf { pages: 4, mark: "r", outline: vec![("R", 3, None)], rotate: Some(90), ..Default::default() }.write(&a);
    let o = e.run(PDF_ROTATE, &[&a], &[("angle", "270"), ("pages", "2-3")]).unwrap();
    let out = o.output.unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "a - pivoté.pdf");
    assert_eq!(e.marks(&out), ["r-1", "r-2", "r-3", "r-4"]);
    assert_eq!(e.outline(&out), ["R@3"], "signets gardés");
    assert!(o.report.unwrap().lost.is_empty());
    let o = e.run(PDF_ROTATE, &[&a], &[("angle", "180")]).unwrap();
    assert!(o.output.unwrap().exists());
    assert_eq!(e.run(PDF_ROTATE, &[&a], &[("pages", "9")]).unwrap_err().code(), "invalid_option");
    assert_eq!(e.run(PDF_ROTATE, &[&a], &[("angle", "45")]).unwrap_err().code(), "invalid_option");
}

#[test]
fn protected_damaged_and_wrong_files_are_refused() {
    let Some(e) = env("refus", &["qpdf"]) else { return };
    let ok = e.dir.join("ok.pdf");
    Pdf { pages: 2, mark: "x", ..Default::default() }.write(&ok);
    let encrypt = |name: &str, user: &str| {
        let out = e.dir.join(name);
        let s = std::process::Command::new(&e.tools["qpdf"])
            .args(["--encrypt", user, "maitre", "256", "--modify=none", "--"])
            .arg(&ok)
            .arg(&out)
            .status()
            .unwrap();
        assert!(s.success());
        out
    };
    let locked = encrypt("mot de passe.pdf", "secret");
    let limited = encrypt("limité.pdf", "");
    for op in [PDF_MERGE, PDF_SPLIT, PDF_ROTATE] {
        let files: Vec<&Path> = if op == PDF_MERGE { vec![&ok, &locked] } else { vec![&locked] };
        let err = e.run(op, &files, &[]).unwrap_err();
        assert_eq!(err.code(), "bad_input", "{op} : {err}");
        assert!(err.to_string().contains("mot de passe"), "{err}");
    }
    assert!(e.run(PDF_MERGE, &[&ok, &limited], &[]).unwrap_err().to_string().contains("protégé"));
    assert!(e.run(PDF_SPLIT, &[&limited], &[]).unwrap_err().to_string().contains("protégé"));
    // Pivoter garde la protection telle quelle.
    let r = e.run(PDF_ROTATE, &[&limited], &[]).unwrap().output.unwrap();
    let s = std::process::Command::new(&e.tools["qpdf"]).arg("--is-encrypted").arg(&r).status().unwrap();
    assert_eq!(s.code(), Some(0), "toujours chiffré");

    let broken = e.dir.join("abîmé.pdf");
    std::fs::write(&broken, b"%PDF-1.7\nrien d'utile ici\n%%EOF\n").unwrap();
    assert_eq!(e.run(PDF_MERGE, &[&ok, &broken], &[]).unwrap_err().code(), "bad_input");
    let text = e.dir.join("notes.txt");
    std::fs::write(&text, "bonjour tout le monde").unwrap();
    let err = e.run(PDF_MERGE, &[&ok, &text], &[]).unwrap_err().to_string();
    assert!(err.contains("notes.txt") && err.contains("convertissez-le"), "{err}");
    assert_eq!(e.run(PDF_MERGE, &[&ok], &[]).unwrap_err().code(), "bad_input", "un seul fichier");
    assert_eq!(e.run(PDF_MERGE, &[&ok, &e.dir.join("absent.pdf")], &[]).unwrap_err().code(), "not_found");
    // Aucun résultat n'est resté.
    assert!(!e.dir.join("ok - fusion.pdf").exists());
}

#[test]
fn conflicts_names_and_cancellation() {
    let Some(e) = env("conflits", &["qpdf"]) else { return };
    let a = e.dir.join("a.pdf");
    let b = e.dir.join("b.pdf");
    Pdf { pages: 1, mark: "a", ..Default::default() }.write(&a);
    Pdf { pages: 1, mark: "b", ..Default::default() }.write(&b);
    let named = |c: Conflict, n: &'static str| e.run_with(PDF_MERGE, &[&a, &b], &[], move |s| (s.name, s.conflict) = (Some(n.into()), c));
    assert!(named(Conflict::Rename, "tout.PDF").unwrap().output.unwrap().ends_with("tout.pdf"));
    assert!(named(Conflict::Rename, "tout").unwrap().output.unwrap().ends_with("tout (1).pdf"));
    assert!(named(Conflict::Skip, "tout").unwrap().output.is_none());
    assert!(named(Conflict::Overwrite, "tout").unwrap().output.unwrap().ends_with("tout.pdf"));
    // Le résultat ne remplace jamais un des fichiers fusionnés, même le deuxième.
    assert_eq!(named(Conflict::Overwrite, "b").unwrap_err().code(), "same_file");
    // Un nom piégé reste un simple nom, dans le dossier prévu.
    let o = named(Conflict::Rename, "../évasion").unwrap().output.unwrap();
    assert_eq!(o.parent().unwrap(), e.dir);

    let token = CancelToken::new();
    token.cancel();
    let s = OperationSpec::new(PDF_MERGE, vec![a.clone(), b.clone()]);
    assert_eq!(e.runner.run_operation(&s, &token, &|_| {}).unwrap_err().code(), "cancelled");
    assert!(!e.dir.join("a - fusion.pdf").exists());
}

#[test]
fn archives_hold_exactly_the_chosen_files() {
    let Some(e) = env("archives", &["7zip"]) else { return };
    let one = e.dir.join("un");
    let two = e.dir.join("deux é");
    std::fs::create_dir_all(one.join("sous dossier/vide")).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("sous dossier/é 文.txt"), "contenu").unwrap();
    std::fs::write(one.join("-tiret.txt"), "tiret").unwrap();
    std::fs::write(two.join("@liste.txt"), "arobase").unwrap();
    std::fs::write(two.join("photo [1].bin"), vec![7u8; 200_000]).unwrap();
    std::fs::write(e.dir.join("photo [1].bin"), b"homonyme").unwrap();
    let files = [
        one.clone(),
        two.join("@liste.txt"),
        two.join("photo [1].bin"),
        e.dir.join("photo [1].bin"),
        two.join("@liste.txt"), // deux fois : une seule dans l'archive
    ];
    let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let mut formats = vec![("zip", "zip"), ("7z", "7z"), ("tar", "tar"), ("tgz", "tar.gz"), ("tbz2", "tar.bz2"), ("txz", "tar.xz")];
    let offered = e.runner.operation_options(ARCHIVE_CREATE).unwrap();
    let chud_core::OptionKind::Choice { choices } = &offered[0].kind else { panic!("format") };
    formats.retain(|(f, _)| choices.iter().any(|c| c.value == *f));
    assert!(formats.len() >= 3, "{choices:?}");
    for (format, ext) in formats {
        let o = e.run_with(ARCHIVE_CREATE, &refs, &[("format", format)], |s| s.name = Some(format!("tout-{format}"))).unwrap();
        let out = o.output.unwrap();
        assert!(out.to_string_lossy().ends_with(&format!(".{ext}")), "{}", out.display());
        assert!(o.report.unwrap().notes.iter().any(|n| n.contains("photo [1] (2).bin")), "homonyme annoncé");
        // Extraction par la conversion ordinaire, puis comparaison octet par octet.
        let mut s = JobSpec::new(&out, "directory");
        s.output_dir = Some(e.dir.join("extrait"));
        let dir = e.runner.run(&s, &CancelToken::new(), &|_| {}).unwrap().output.unwrap();
        let same = |a: &str, b: &Path| assert_eq!(std::fs::read(dir.join(a)).unwrap(), std::fs::read(b).unwrap(), "{format} {a}");
        same("un/sous dossier/é 文.txt", &one.join("sous dossier/é 文.txt"));
        same("un/-tiret.txt", &one.join("-tiret.txt"));
        same("@liste.txt", &two.join("@liste.txt"));
        same("photo [1].bin", &two.join("photo [1].bin"));
        same("photo [1] (2).bin", &e.dir.join("photo [1].bin"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 4, "{format} : rien de plus");
        assert!(dir.join("un/sous dossier/vide").is_dir(), "{format} : dossier vide gardé");
    }
    // Un dossier seul, un fichier seul : nom d'après l'élément.
    let o = e.run(ARCHIVE_CREATE, &[&one], &[]).unwrap();
    assert!(o.output.unwrap().ends_with("un.zip"));
    let empty = e.dir.join("rien");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(e.run(ARCHIVE_CREATE, &[&empty], &[]).unwrap_err().code(), "bad_input");
    assert_eq!(e.run(ARCHIVE_CREATE, &[&one], &[("format", "rar")]).unwrap_err().code(), "invalid_option");
}

/// Fusion d'une fusion : ce que l'application produit reste utilisable par elle-même.
#[test]
fn results_can_be_processed_again() {
    let Some(e) = env("chaine", &["qpdf"]) else { return };
    let a = e.dir.join("a.pdf");
    Pdf { pages: 2, mark: "a", outline: vec![("A", 2, None)], title: Some("T"), ..Default::default() }.write(&a);
    let m = e.run(PDF_MERGE, &[&a, &a], &[]).unwrap().output.unwrap();
    let r = e.run(PDF_ROTATE, &[&m], &[]).unwrap().output.unwrap();
    let s = e.run(PDF_SPLIT, &[&r], &[("every", "2")]).unwrap().output.unwrap();
    let mut parts: Vec<PathBuf> = std::fs::read_dir(&s).unwrap().map(|f| f.unwrap().path()).collect();
    parts.sort();
    assert_eq!(e.outline(&parts[1]), ["a@1", "  A@2"]);
    let refs: Vec<&Path> = parts.iter().map(PathBuf::as_path).collect();
    let back = e.run(PDF_MERGE, &refs, &[("bookmarks", "keep")]).unwrap().output.unwrap();
    assert_eq!(e.marks(&back), ["a-1", "a-2", "a-1", "a-2"]);
    assert_eq!(e.outline(&back), ["a@1", "  A@2", "a@3", "  A@4"]);
}
