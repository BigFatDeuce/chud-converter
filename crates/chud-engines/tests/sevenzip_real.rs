//! Tests réels de l'adaptateur 7-Zip. Ignorés (« SKIP ») si 7-Zip est absent.

use chud_core::{Engine, EngineError, EngineOrigin, ExecContext, FormatId, OptionValues, StepTask, Verification};
use chud_engines::sevenzip::SevenZip;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Moteur absent : test ignoré, sauf si `CHUD_REQUIRE_ENGINES` est défini (CI), où c'est un échec.
fn skip(why: &str) {
    if std::env::var_os("CHUD_REQUIRE_ENGINES").is_some() {
        panic!("{why} alors que CHUD_REQUIRE_ENGINES est défini");
    }
    eprintln!("SKIP : {why}");
}

fn seven_zip() -> Option<PathBuf> {
    let exes: &[&str] = if cfg!(windows) { &["7z.exe"] } else { &["7zz", "7z"] };
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut cands: Vec<PathBuf> = exes.iter().flat_map(|e| dirs.iter().map(move |d| d.join(e))).collect();
    if cfg!(windows) {
        cands.push(PathBuf::from(r"C:\Program Files\7-Zip\7z.exe"));
    }
    let found = cands.into_iter().find(|p| p.is_file());
    if found.is_none() {
        skip("7-Zip introuvable");
    }
    found
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chud-7z-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, data) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap();
}

fn run(e: &SevenZip, input: &Path, from: &str, to: &str, out: &Path) -> Result<Verification, EngineError> {
    let (from, to) = (FormatId::from(from), FormatId::from(to));
    let options = OptionValues::new().resolve(&e.options(&from, &to)).unwrap();
    let source = e.probe(input, &from);
    let task = StepTask { input: input.into(), output: out.into(), from, to, options, source };
    let ctx = ExecContext::new(out.parent().unwrap().into());
    e.execute(&task, &ctx)?;
    Ok(e.verify(out, &task, &ctx))
}

fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for ent in std::fs::read_dir(&d).unwrap() {
            let p = ent.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                v.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    v.sort();
    v
}

#[test]
fn real_archive_round_trips() {
    let Some(exe) = seven_zip() else { return };
    let e = SevenZip::new(exe, None, EngineOrigin::System).expect("sonde 7-Zip");
    let dir = workdir("rt");
    let src = dir.join("-source é.zip");
    let content: &[(&str, &[u8])] = &[
        ("dossier/é 文件.txt", "Bonjour à tous".as_bytes()),
        ("dossier/sous/données.bin", &[0u8, 1, 2, 3, 255]),
        ("racine.txt", b"racine"),
    ];
    make_zip(&src, content);
    let before = std::fs::read(&src).unwrap();

    let info = e.probe(&src, &FormatId::from("zip")).expect("sonde");
    assert!(info.summary.iter().any(|(k, v)| k == "Entrées" && v.starts_with("3 fichier")), "{:?}", info.summary);

    // zip → 7z (vérifié par comparaison d'index), 7z → dossier, dossier → tar.gz → dossier.
    let v = run(&e, &src, "zip", "7z", &dir.join("a.7z")).expect("zip → 7z");
    assert!(matches!(v, Verification::Decoded(_)), "{v:?}");
    let v = run(&e, &dir.join("a.7z"), "7z", "directory", &dir.join("extrait")).expect("7z → dossier");
    assert!(v.is_ok(), "{v:?}");
    let expected: Vec<(String, Vec<u8>)> = {
        let mut x: Vec<_> = content.iter().map(|(n, d)| (n.to_string(), d.to_vec())).collect();
        x.sort();
        x
    };
    assert_eq!(tree(&dir.join("extrait")), expected);

    let v = run(&e, &dir.join("extrait"), "directory", "tgz", &dir.join("b.tar.gz")).expect("dossier → tgz");
    assert!(v.is_ok(), "{v:?}");
    let v = run(&e, &dir.join("b.tar.gz"), "tgz", "directory", &dir.join("extrait2")).expect("tgz → dossier");
    assert!(v.is_ok(), "{v:?}");
    assert_eq!(tree(&dir.join("extrait2").join("extrait")), expected);

    let v = run(&e, &dir.join("extrait"), "directory", "zip", &dir.join("c.zip")).expect("dossier → zip");
    assert!(matches!(v, Verification::Decoded(_)), "{v:?}");

    assert_eq!(before, std::fs::read(&src).unwrap(), "la source ne doit jamais être modifiée");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_archive_attacks_are_refused() {
    let Some(exe) = seven_zip() else { return };
    let e = SevenZip::new(exe, None, EngineOrigin::System).unwrap();
    let dir = workdir("attack");

    // Zip-slip : rien ne doit être écrit hors du dossier de destination.
    let slip = dir.join("slip.zip");
    make_zip(&slip, &[("ok.txt", b"ok"), ("../../evil.txt", b"pwned")]);
    let out = dir.join("x").join("extrait");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let r = run(&e, &slip, "zip", "directory", &out);
    assert!(matches!(r, Err(EngineError::Refused(_))), "{r:?}");
    assert!(!dir.join("evil.txt").exists() && !dir.join("x").join("evil.txt").exists());

    // Chemin absolu.
    let abs = dir.join("abs.zip");
    make_zip(&abs, &[("/tmp/evil.txt", b"pwned")]);
    assert!(matches!(run(&e, &abs, "zip", "directory", &dir.join("abs-out")), Err(EngineError::Refused(_))));

    // Entrées en double.
    let dup = dir.join("dup.zip");
    make_zip(&dup, &[("a.txt", b"1"), ("A.TXT", b"2")]);
    let r = run(&e, &dup, "zip", "directory", &dir.join("dup-out"));
    if cfg!(windows) {
        assert!(matches!(r, Err(EngineError::Refused(_))), "doublon insensible à la casse : {r:?}");
    }

    // Fichier qui n'est pas une archive.
    let fake = dir.join("faux.zip");
    std::fs::write(&fake, b"ceci n'est pas une archive").unwrap();
    assert!(run(&e, &fake, "zip", "directory", &dir.join("fake-out")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
