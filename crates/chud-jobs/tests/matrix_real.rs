//! Matrice réelle : des sources représentatives de chaque catégorie × **chaque** cible que le
//! logiciel propose pour elles, avec les vrais moteurs (FFmpeg, 7-Zip, moteurs natifs).
//!
//! Chaque conversion proposée doit réussir **et** passer la vérification propre à sa catégorie
//! (décodage intégral, durée, canaux, fréquence, dimensions, pixels ou échantillons identiques
//! quand la conversion est annoncée sans perte, relecture des données, test d'intégrité des
//! archives). Une cible proposée qui échoue est un bug : soit la conversion doit être corrigée,
//! soit elle ne doit plus être proposée.
//!
//! Les médias sont générés à la volée (mire, sinusoïde, bruit), aucun binaire n'est stocké.
//! Sans FFmpeg ou 7-Zip, le test est ignoré (« SKIP »), sauf si `CHUD_REQUIRE_ENGINES` est
//! défini (CI), où c'est un échec.

use chud_core::detect::detect_path;
use chud_core::{CancelToken, Fidelity, OptionValues, Registry, Verification};
use chud_enginemgr::{AppPaths, EngineManager, EngineState};
use chud_jobs::{Conflict, JobRunner, JobSpec};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn skip(why: &str) {
    if std::env::var_os("CHUD_REQUIRE_ENGINES").is_some() {
        panic!("{why} alors que CHUD_REQUIRE_ENGINES est défini");
    }
    eprintln!("SKIP : {why}");
}

struct Env {
    dir: PathBuf,
    runner: Arc<JobRunner>,
    ffmpeg: Option<PathBuf>,
    sevenzip: bool,
    sevenzip_path: Option<PathBuf>,
    /// Moteurs prêts (identifiants).
    ready: Vec<String>,
}

impl Env {
    fn has(&self, id: &str) -> bool {
        self.ready.iter().any(|r| r == id)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Env {
    let dir = std::env::temp_dir().join(format!("chud-matrix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = AppPaths::at(&dir.join("app"));
    // `CHUD_TEST_ENGINES_DATA` : dossier `data` d'une installation portable dont on reprend les
    // moteurs gérés (essai complet sous Windows, où ils ne sont pas dans le PATH).
    let discovery = match std::env::var_os("CHUD_TEST_ENGINES_DATA").map(PathBuf::from) {
        Some(d) => AppPaths { config_dir: d.join("config"), data_dir: d, cache_dir: paths.cache_dir.clone(), portable: true },
        None => paths.clone(),
    };
    let mgr = EngineManager::new(discovery).unwrap();
    let mut statuses = mgr.discover();
    let registry: Registry = mgr.build_registry(&mut statuses);
    let ready = |id: &str| statuses.iter().find(|s| s.id == id && s.state == EngineState::Ready);
    let ffmpeg = ready("ffmpeg").and_then(|s| s.path.clone());
    let sevenzip = ready("7zip").is_some();
    let sevenzip_path = ready("7zip").and_then(|s| s.path.clone());
    let ready = statuses.iter().filter(|s| s.state == EngineState::Ready).map(|s| s.id.clone()).collect();
    Env { runner: Arc::new(JobRunner::new(Arc::new(registry), paths.cache_dir.clone())), dir, ffmpeg, sevenzip, sevenzip_path, ready }
}

fn ff(ffmpeg: &Path, out: &Path, args: &[&str]) {
    let spec = ProcessSpec::new(ffmpeg).args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"]).args(args.iter().copied()).arg(out);
    let o = chud_process::run_capture(&spec, 0).expect("lancement ffmpeg");
    assert!(o.success(), "génération de {} échouée : {}", out.display(), o.stderr_tail);
}

const STEREO: &[&str] = &[
    "-f",
    "lavfi",
    "-i",
    "sine=f=440:d=4:sample_rate=44100",
    "-f",
    "lavfi",
    "-i",
    "anoisesrc=d=4:c=pink:r=44100:a=0.05",
    "-filter_complex",
    "[0][1]amerge=inputs=2[a]",
    "-map",
    "[a]",
];
const TAGS: &[&str] = &["-metadata", "title=Été indien", "-metadata", "artist=Ace Ünïcode", "-metadata", "album=Démo"];

/// Sources représentatives : profils courants et cas limites connus pour avoir posé problème.
fn media_samples(ffmpeg: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut make = |name: &str, args: Vec<&str>| {
        let p = dir.join(name);
        ff(ffmpeg, &p, &args);
        out.push(p);
    };
    // --- Audio
    make("pcm.wav", [STEREO, &["-c:a", "pcm_s16le"]].concat());
    make("tags.flac", [STEREO, TAGS, &["-c:a", "flac"]].concat());
    make("tags.mp3", [STEREO, TAGS, &["-c:a", "libmp3lame", "-q:a", "4"]].concat());
    make("aac.m4a", [STEREO, TAGS, &["-c:a", "aac", "-b:a", "128k"]].concat());
    make("raw.aac", [STEREO, &["-c:a", "aac", "-b:a", "128k"]].concat());
    make("vorbis.ogg", [STEREO, TAGS, &["-c:a", "libvorbis"]].concat());
    make("opus.opus", [STEREO, &["-c:a", "libopus"]].concat());
    make("s24_96k.flac", vec!["-f", "lavfi", "-i", "sine=f=1000:d=3:sample_rate=96000", "-ac", "2", "-c:a", "flac", "-sample_fmt", "s32"]);
    make(
        "surround.flac",
        vec!["-f", "lavfi", "-i", "sine=f=500:d=3:sample_rate=48000", "-af", "pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0", "-c:a", "flac"],
    );
    make("float.wav", [STEREO, &["-c:a", "pcm_f32le"]].concat());
    make("mono8k.wav", vec!["-f", "lavfi", "-i", "sine=f=300:d=3:sample_rate=8000", "-ac", "1", "-c:a", "pcm_s16le"]);
    // --- Vidéo
    let av = ["-f", "lavfi", "-i", "testsrc2=s=320x240:r=25:d=3", "-f", "lavfi", "-i", "sine=f=440:d=3:sample_rate=48000"];
    make("h264.mp4", [&av[..], &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]].concat());
    make("h264.ts", [&av[..], &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]].concat());
    make("vp9.webm", [&av[..], &["-c:v", "libvpx-vp9", "-b:v", "200k", "-c:a", "libopus", "-shortest"]].concat());
    make("mpeg4.avi", [&av[..], &["-c:v", "mpeg4", "-c:a", "libmp3lame", "-shortest"]].concat());
    make("odd.mkv", vec!["-f", "lavfi", "-i", "testsrc2=s=321x241:r=25:d=2", "-c:v", "ffv1"]);
    // MKV complet : deux pistes audio, sous-titres ASS et SRT (H.264 copié vers AVI : Annex B).
    let srt = dir.join("in.srt");
    std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:02,000\nBonjour, ça va ?\n\n").unwrap();
    let srt_s = srt.to_string_lossy().into_owned();
    make(
        "full.mkv",
        [
            &av[..],
            &["-f", "lavfi", "-i", "sine=f=880:d=3:sample_rate=48000", "-i", &srt_s],
            &["-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"],
            &["-c:s", "srt", "-t", "3"],
        ]
        .concat(),
    );
    // --- Images et animations
    make("anim.gif", vec!["-f", "lavfi", "-i", "testsrc2=s=80x60:r=5:d=1", "-loop", "0"]);
    make("photo.jpg", vec!["-f", "lavfi", "-i", "testsrc2=s=320x240:d=1", "-frames:v", "1", "-q:v", "3"]);
    make("still.webp", vec!["-f", "lavfi", "-i", "testsrc2=s=120x90:d=1", "-frames:v", "1"]);
    make(
        "rgba.png",
        vec![
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x48:d=1,format=rgba,geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='if(lt(X,32),255,80)'",
            "-frames:v",
            "1",
        ],
    );
    make("deep.tiff", vec!["-f", "lavfi", "-i", "testsrc2=s=64x48:d=1,format=rgb48le", "-frames:v", "1"]);
    make("odd.png", vec!["-f", "lavfi", "-i", "testsrc2=s=33x17:d=1", "-frames:v", "1"]);
    out
}

fn text_samples(dir: &Path) -> Vec<PathBuf> {
    let files: &[(&str, &[u8])] = &[
        ("subs.srt", "1\n00:00:00,500 --> 00:00:02,000\nBonjour, ça va ?\n\n2\n00:00:02,500 --> 00:00:03,500\nÀ bientôt.\n".as_bytes()),
        ("subs.vtt", "WEBVTT\n\n00:00.500 --> 00:02.000\nBonjour, ça va ?\n".as_bytes()),
        ("rows.json", r#"[{"id": 1, "nom": "Élise", "note": "a,b"}, {"id": 2, "nom": "Zoë \"Z\"", "note": null}]"#.as_bytes()),
        ("obj.json", r#"{"nom": "Café ☕", "n": 3, "imbrique": {"x": [1, 2]}}"#.as_bytes()),
        ("table.csv", "id,nom,note\n1,Élise,\"a,b\"\n2,\"Zoë \"\"Z\"\"\",\n".as_bytes()),
        ("conf.yaml", "nom: Café\nlistes:\n  - 1\n  - deux\n".as_bytes()),
        ("conf.toml", "nom = \"Café\"\n[imbrique]\na = true\n".as_bytes()),
    ];
    files
        .iter()
        .map(|(n, b)| {
            let p = dir.join(n);
            std::fs::write(&p, b).unwrap();
            p
        })
        .collect()
}

fn archive_samples(dir: &Path) -> Vec<PathBuf> {
    let folder = dir.join("Dossier éèà");
    std::fs::create_dir_all(folder.join("sous dossier")).unwrap();
    std::fs::write(folder.join("liste des courses.txt"), "pain\nlait\n").unwrap();
    std::fs::write(folder.join("sous dossier").join("données ☕.json"), "{\"a\": 1}").unwrap();
    vec![folder]
}

/// Trame Zstandard minimale : un seul bloc « brut » (non compressé), valide selon la RFC 8878.
fn zstd_raw(data: &[u8]) -> Vec<u8> {
    assert!(data.len() < 128 * 1024, "un seul bloc brut");
    let mut out = vec![0x28, 0xB5, 0x2F, 0xFD, 0xA0]; // magique ; segment unique, taille sur 4 octets
    out.extend((data.len() as u32).to_le_bytes());
    let header = ((data.len() as u32) << 3) | 1; // bloc brut, dernier bloc
    out.extend(&header.to_le_bytes()[..3]);
    out.extend(data);
    out
}

/// Archive `ar` (format des paquets Debian).
fn ar(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, data) in members {
        out.extend(format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", name, 0, 0, 0, 100644, data.len()).as_bytes());
        out.extend(*data);
        if data.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

/// Archive cpio « newc » (SVR4, sans somme de contrôle).
fn cpio(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut entry = |name: &str, data: &[u8], mode: u32, ino: u32| {
        let head = format!(
            "070701{ino:08X}{mode:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
            0,
            0,
            1,
            0,
            data.len(),
            0,
            0,
            0,
            0,
            name.len() + 1,
            0
        );
        out.extend(head.as_bytes());
        out.extend(name.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    };
    for (i, (name, data)) in files.iter().enumerate() {
        entry(name, data, 0o100644, i as u32 + 1);
    }
    entry("TRAILER!!!", &[], 0, 0);
    out
}

/// Archives que l'on ne sait écrire qu'en lecture : générées ici (7-Zip pour gz/bz2/xz,
/// octet par octet pour zst, tzst, deb et cpio, `makecab` sous Windows), puis converties.
fn read_only_archive_samples(e: &Env, dir: &Path) -> Vec<PathBuf> {
    let Some(z) = e.sevenzip_path.clone() else { return Vec::new() };
    let src = dir.join("lisez-moi é.txt");
    std::fs::write(&src, "Bonjour, archive de test.\n".repeat(20)).unwrap();
    let mut out = Vec::new();
    let run7 = |args: &[&str], archive: &Path, input: &Path| {
        let spec = ProcessSpec::new(&z).args(args.iter().copied()).arg(archive).arg(input);
        let o = chud_process::run_capture(&spec, 0).expect("lancement 7-Zip");
        assert!(o.success(), "7-Zip : {}", o.stderr_tail);
    };
    for (t, ext) in [("gzip", "gz"), ("bzip2", "bz2"), ("xz", "xz")] {
        let a = dir.join(format!("notes.txt.{ext}"));
        run7(&["a", "-bso0", "-bsp0", &format!("-t{t}")], &a, &src);
        out.push(a);
    }
    let tar = dir.join("paquet.tar");
    run7(&["a", "-bso0", "-bsp0", "-ttar"], &tar, &src);
    let tar_bytes = std::fs::read(&tar).unwrap();
    std::fs::remove_file(&tar).unwrap();
    let body = std::fs::read(&src).unwrap();
    for (name, bytes) in [
        ("notes.txt.zst", zstd_raw(&body)),
        ("paquet.tar.zst", zstd_raw(&tar_bytes)),
        ("paquet.deb", ar(&[("debian-binary", b"2.0\n"), ("data.tar", &tar_bytes)])),
        ("fichiers.cpio", cpio(&[("lisez-moi.txt", &body), ("autre.txt", b"deux\n")])),
    ] {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        out.push(p);
    }
    // CAB : « makecab » de Windows, lancé dans le dossier avec des noms simples (il gère mal
    // les chemins longs ou accentués).
    if let Some(root) = std::env::var_os("SystemRoot").filter(|_| cfg!(windows)) {
        let makecab = PathBuf::from(root).join("System32").join("makecab.exe");
        std::fs::copy(&src, dir.join("cab.txt")).unwrap();
        let spec = ProcessSpec::new(&makecab).args(["cab.txt", "fichiers.cab"]).cwd(dir);
        let o = chud_process::run_capture(&spec, 0).expect("lancement de makecab");
        assert!(o.success(), "makecab : {}", o.stderr_tail);
        std::fs::remove_file(dir.join("cab.txt")).unwrap();
        out.push(dir.join("fichiers.cab"));
    }
    std::fs::remove_file(&src).unwrap();
    out
}

/// Résultat d'une conversion de la matrice.
struct Outcome {
    label: String,
    target: String,
    output: Option<PathBuf>,
    error: Option<String>,
    weak_check: bool,
}

fn convert_all(e: &Env, sources: &[PathBuf]) -> Vec<Outcome> {
    let registry = e.runner.registry();
    let mut jobs: Vec<(PathBuf, String)> = Vec::new();
    for src in sources {
        let det = detect_path(src, registry.catalog()).unwrap();
        let fmt = det.format.unwrap_or_else(|| panic!("{} non reconnu", src.display()));
        let info = registry.probe(src, &fmt);
        let targets = registry.targets(&fmt, info.as_ref(), &OptionValues::new());
        assert!(!targets.is_empty(), "aucune cible pour {}", src.display());
        jobs.extend(targets.into_iter().map(|t| (src.clone(), t.format.to_string())));
    }
    let queue = Mutex::new(jobs.into_iter().enumerate().collect::<Vec<_>>());
    let results = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 8);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((i, (src, target))) = queue.lock().unwrap().pop() else { break };
                    let name = src.file_name().unwrap().to_string_lossy().into_owned();
                    let mut spec = JobSpec::new(&src, target.as_str());
                    spec.output_dir = Some(e.dir.join("out").join(format!("{i}-{target}")));
                    spec.conflict = Conflict::Overwrite;
                    let label = format!("{name} → {target}");
                    let fail =
                        |label, error| Outcome { label, target: target.clone(), output: None, error: Some(error), weak_check: false };
                    let outcome = match e.runner.run(&spec, &CancelToken::new(), &|_| {}) {
                        Ok(o) => {
                            let lossless = o.plan.as_ref().is_some_and(|p| p.report.overall == Fidelity::Lossless);
                            let weak = o.verifications.iter().any(|v| matches!(v, Verification::SignatureOnly));
                            match o.output {
                                Some(p) if p.exists() => {
                                    Outcome { label, target: target.clone(), output: Some(p), error: None, weak_check: weak && !lossless }
                                }
                                _ => fail(label, "aucun fichier produit".into()),
                            }
                        }
                        Err(err) => fail(label, err.to_string()),
                    };
                    results.lock().unwrap().push(outcome);
                }
            });
        }
    });
    results.into_inner().unwrap()
}

/// Refus attendu : les échantillons synthétiques ne contiennent aucun texte, la reconnaissance
/// (OCR) doit donc échouer clairement plutôt que produire un fichier vide. L'OCR sur une vraie
/// image de texte est vérifiée dans `chud-engines/tests/tools_real.rs`.
/// Refus honnêtes : une image sans texte pour l'OCR, une bande dessinée (« bd.* », que des
/// images) vers un format de texte.
fn expected_refusal(o: &Outcome) -> bool {
    o.error
        .as_deref()
        .is_some_and(|e| e.contains("aucun texte reconnu dans l'image") || (o.label.starts_with("bd.") && e.contains("aucun texte")))
}

fn report(results: &[Outcome]) {
    let refused = results.iter().filter(|o| expected_refusal(o)).count();
    if refused > 0 {
        eprintln!("{refused} conversion(s) refusée(s) faute de texte, comme attendu");
    }
    let failures: Vec<String> =
        results.iter().filter(|o| !expected_refusal(o)).filter_map(|o| Some(format!("  {} : {}", o.label, o.error.as_ref()?))).collect();
    let weak: Vec<&str> = results.iter().filter(|o| o.weak_check).map(|o| o.label.as_str()).collect();
    eprintln!("{} conversions, {} échecs, {} vérifiées par signature seulement", results.len(), failures.len(), weak.len());
    assert!(failures.is_empty(), "{} conversion(s) proposée(s) en échec :\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn every_offered_media_conversion_works_and_is_verified() {
    let e = env("media");
    let Some(ffmpeg) = e.ffmpeg.clone() else {
        skip("FFmpeg absent");
        return;
    };
    let src = e.dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let sources = media_samples(&ffmpeg, &src);
    let results = convert_all(&e, &sources);
    report(&results);
}

#[test]
fn every_offered_text_data_and_archive_conversion_works() {
    let e = env("text");
    let src = e.dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut sources = text_samples(&src);
    if e.sevenzip {
        sources.extend(archive_samples(&src));
    } else {
        skip("7-Zip absent");
    }
    let mut results = convert_all(&e, &sources);
    report(&results);
    // Second tour : chaque archive produite est relue à son tour comme source (lecture,
    // extraction et reconversion réelles de chaque format d'archive que l'on sait écrire).
    let archives: Vec<PathBuf> = results
        .iter()
        .filter(|o| o.target != "directory" && registry_kind(&e, &o.target) == Some(chud_core::Category::Archive))
        .filter_map(|o| o.output.clone())
        .collect();
    if e.sevenzip {
        assert!(archives.len() >= 7, "archives produites : {archives:?}");
    }
    let mut archives = archives;
    let extra = e.dir.join("lecture");
    std::fs::create_dir_all(&extra).unwrap();
    archives.extend(read_only_archive_samples(&e, &extra));
    let second = convert_all(&e, &archives);
    eprintln!("second tour : {} archives relues", archives.len());
    report(&second);
    results.extend(second);
}

fn registry_kind(e: &Env, id: &str) -> Option<chud_core::Category> {
    e.runner.registry().catalog().get(id).map(|d| d.category)
}

/// Présentation OpenDocument minimale écrite à la main (aucun moteur ne sait en créer une à
/// partir d'un autre format) : deux diapositives avec du texte accentué.
fn odp(path: &Path) {
    use std::io::Write;
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default();
    z.start_file("mimetype", stored).unwrap();
    z.write_all(b"application/vnd.oasis.opendocument.presentation").unwrap();
    z.start_file("META-INF/manifest.xml", deflated).unwrap();
    z.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
 <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.presentation"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
    )
    .unwrap();
    z.start_file("content.xml", deflated).unwrap();
    let slide = |n: u32, text: &str| {
        format!(
            r#"<draw:page draw:name="Diapo {n}"><draw:frame svg:x="2cm" svg:y="2cm" svg:width="20cm" svg:height="4cm"><draw:text-box><text:p>{text}</text:p></draw:text-box></draw:frame></draw:page>"#
        )
    };
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" office:version="1.2"><office:body><office:presentation>{}{}</office:presentation></office:body></office:document-content>"#,
        slide(1, "Bonjour, présentation é"),
        slide(2, "Deuxième diapositive")
    );
    z.write_all(content.as_bytes()).unwrap();
    z.finish().unwrap();
}

/// Deux pages PNG de bande dessinée.
fn comic_pages(dir: &Path) -> Vec<PathBuf> {
    (1..=2u32)
        .map(|n| {
            let p = dir.join(format!("page{n:02}.png"));
            let img = image::RgbImage::from_fn(120, 160, |x, y| image::Rgb([(x * 2) as u8, (y + n * 40) as u8, 90]));
            img.save(&p).unwrap();
            p
        })
        .collect()
}

fn cbz(path: &Path, pages: &[PathBuf]) {
    use std::io::Write;
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for p in pages {
        z.start_file(p.file_name().unwrap().to_string_lossy(), zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&std::fs::read(p).unwrap()).unwrap();
    }
    z.finish().unwrap();
}

/// Sources « documents » : textes balisés écrits ici, plus ce que seuls certains moteurs savent
/// lire (PostScript, SVG, présentation, bandes dessinées).
fn document_samples(e: &Env, dir: &Path) -> Vec<PathBuf> {
    let files: &[(&str, &str)] = &[
        (
            "notes é.md",
            "# Réunion du 3 mars\n\nTexte avec **gras**, *italique* et un [lien](https://example.org).\n\n- un\n- deux\n\n| Nom | Note |\n|-----|------|\n| Élise | 12 |\n| Zoë | 15 |\n\n```\ncode();\n```\n",
        ),
        (
            "page.html",
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Page é</title></head><body><h1>Titre</h1><p>Un paragraphe <b>gras</b>.</p><table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table></body></html>",
        ),
        ("lettre.txt", "Madame, Monsieur,\n\nVoici une lettre accentuée : été, Noël, cœur.\n\nCordialement.\n"),
        ("doc.rst", "Titre\n=====\n\nUn paragraphe *accentué* : été.\n\n- un\n- deux\n"),
        ("doc.org", "* Titre\nUn paragraphe /accentué/ : été.\n- un\n- deux\n"),
        ("doc.tex", "\\documentclass{article}\n\\begin{document}\n\\section{Titre}\nUn paragraphe accentu\\'e.\n\\end{document}\n"),
        (
            "carnet.ipynb",
            r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Carnet\n","Texte é."]},{"cell_type":"code","execution_count":1,"metadata":{},"outputs":[],"source":["print(1)"]}],"metadata":{},"nbformat":4,"nbformat_minor":5}"##,
        ),
        ("affiche.ps", "%!PS-Adobe-3.0\n/Helvetica findfont 48 scalefont setfont\n72 600 moveto (Bonjour Chud) show\nshowpage\n"),
        (
            "dessin.svg",
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80"><rect width="120" height="80" fill="#3a6"/><circle cx="60" cy="40" r="25" fill="#fc0"/><text x="10" y="75" font-size="12">Chud</text></svg>"##,
        ),
    ];
    let mut out: Vec<PathBuf> = files
        .iter()
        .map(|(n, b)| {
            let p = dir.join(n);
            std::fs::write(&p, b).unwrap();
            p
        })
        .collect();
    let p = dir.join("diapos é.odp");
    odp(&p);
    out.push(p);
    let pages_dir = dir.join("pages");
    std::fs::create_dir_all(&pages_dir).unwrap();
    let pages = comic_pages(&pages_dir);
    let p = dir.join("bd.cbz");
    cbz(&p, &pages);
    out.push(p);
    if let Some(z) = &e.sevenzip_path {
        for (t, ext) in [("tar", "cbt"), ("7z", "cb7")] {
            let a = dir.join(format!("bd.{ext}"));
            let spec = ProcessSpec::new(z).args(["a", "-bso0", "-bsp0", &format!("-t{t}")]).arg(&a).args(pages.iter());
            let o = chud_process::run_capture(&spec, 0).expect("lancement 7-Zip");
            assert!(o.success(), "7-Zip : {}", o.stderr_tail);
            out.push(a);
        }
    }
    out
}

/// Documents, tableurs, présentations, PDF, livres et images avancées : chaque cible proposée
/// pour des sources écrites ici, puis **chaque format obtenu** relu comme source à son tour (un
/// fichier par format) vers chacune de ses cibles. Demande LibreOffice et Pandoc (sinon
/// « SKIP », ou échec si `CHUD_REQUIRE_TOOLS` est défini).
#[test]
fn every_offered_document_conversion_works() {
    let e = env("docs");
    if let Some(missing) = ["libreoffice", "pandoc"].iter().find(|id| !e.has(id)) {
        assert!(std::env::var_os("CHUD_REQUIRE_TOOLS").is_none(), "{missing} absent alors que CHUD_REQUIRE_TOOLS est défini");
        eprintln!("SKIP : {missing} absent");
        return;
    }
    eprintln!("moteurs prêts : {}", e.ready.join(", "));
    let src = e.dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let sources = document_samples(&e, &src);
    let start = std::time::Instant::now();
    let first = convert_all(&e, &sources);
    eprintln!("premier tour : {} sources, {:?}", sources.len(), start.elapsed());
    use chud_core::Category as C;
    let relu = [C::Document, C::Spreadsheet, C::Presentation, C::Pdf, C::Ebook, C::Image];
    let already: std::collections::HashSet<String> =
        sources.iter().filter_map(|p| detect_path(p, e.runner.registry().catalog()).ok()?.format.map(|f| f.to_string())).collect();
    let mut seen = std::collections::HashSet::new();
    // Un format par fichier relu : de préférence tiré d'une source qui a du texte (une BD
    // convertie en RTF ou en EPUB ne contient que des images).
    let mut ordered: Vec<&Outcome> = first.iter().collect();
    ordered.sort_by_key(|o| o.label.starts_with("bd."));
    let produced: Vec<(String, PathBuf)> = ordered
        .into_iter()
        .filter(|o| o.error.is_none() && registry_kind(&e, &o.target).is_some_and(|k| relu.contains(&k)))
        .filter(|o| !already.contains(&o.target) && seen.insert(o.target.clone()))
        .filter_map(|o| Some((o.target.clone(), o.output.clone()?)))
        .collect();
    // Formats que l'on sait écrire sans savoir les relire (AsciiDoc, Typst… : Pandoc ne les lit
    // pas) : ils doivent au moins être reconnus pour ce qu'ils sont.
    let (second_sources, write_only): (Vec<_>, Vec<_>) = produced.into_iter().partition(|(fmt, p)| {
        let det = detect_path(p, e.runner.registry().catalog()).unwrap();
        assert_eq!(det.format.as_ref().map(|f| f.as_str()), Some(fmt.as_str()), "{} mal reconnu", p.display());
        let info = e.runner.registry().probe(p, &det.format.unwrap());
        !e.runner.registry().targets(&fmt.as_str().into(), info.as_ref(), &OptionValues::new()).is_empty()
    });
    let second_sources: Vec<PathBuf> = second_sources.into_iter().map(|(_, p)| p).collect();
    let start = std::time::Instant::now();
    let second = convert_all(&e, &second_sources);
    let mut formats: Vec<String> = seen.into_iter().collect();
    formats.sort();
    eprintln!("second tour : {} formats relus ({}), {:?}", second_sources.len(), formats.join(" "), start.elapsed());
    eprintln!("écrits sans relecture possible : {}", write_only.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(" "));
    let mut all = first;
    all.extend(second);
    report(&all);
}
