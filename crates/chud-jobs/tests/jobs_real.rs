//! Tests de bout en bout des tâches : conflits, protection de la source, annulation réelle,
//! fichiers corrompus. Les tests qui exigent FFmpeg sont ignorés (« SKIP ») s'il est absent.

use chud_core::{CancelToken, Registry};
use chud_enginemgr::{AppPaths, EngineManager, EngineState};
use chud_jobs::{Conflict, JobError, JobQueue, JobRunner, JobSpec, QueueEvent};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// PNG 1×1 valide (pixel rouge).
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63,
    0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

struct Env {
    dir: PathBuf,
    runner: Arc<JobRunner>,
    ffmpeg: Option<PathBuf>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Env {
    let dir = std::env::temp_dir().join(format!("chud-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = AppPaths::at(&dir.join("app"));
    let mgr = EngineManager::new(paths.clone()).unwrap();
    let mut statuses = mgr.discover();
    let registry: Registry = mgr.build_registry(&mut statuses);
    let ffmpeg = statuses.iter().find(|s| s.id == "ffmpeg" && s.state == EngineState::Ready).and_then(|s| s.path.clone());
    Env { runner: Arc::new(JobRunner::new(Arc::new(registry), paths.cache_dir.clone())), dir, ffmpeg }
}

fn no_events(_: chud_jobs::JobEvent) {}

/// Moteur absent : test ignoré, sauf si `CHUD_REQUIRE_ENGINES` est défini (CI), où c'est un échec.
fn skip(why: &str) {
    if std::env::var_os("CHUD_REQUIRE_ENGINES").is_some() {
        panic!("{why} alors que CHUD_REQUIRE_ENGINES est défini");
    }
    eprintln!("SKIP : {why}");
}

fn jobs_left(e: &Env) -> usize {
    std::fs::read_dir(e.dir.join("app").join("cache").join("jobs")).map(|r| r.count()).unwrap_or(0)
}

#[test]
fn conflicts_and_source_protection() {
    let e = env("conflicts");
    let src = e.dir.join("-image é.png");
    std::fs::write(&src, PNG_1X1).unwrap();
    let cancel = CancelToken::new();

    // PNG → PNG dans le même dossier : la sortie désignerait la source.
    let mut spec = JobSpec::new(&src, "png");
    spec.conflict = Conflict::Overwrite;
    let r = e.runner.run(&spec, &cancel, &no_events);
    assert!(matches!(r, Err(JobError::SameFile)), "{r:?}");
    assert_eq!(std::fs::read(&src).unwrap(), PNG_1X1, "la source ne doit jamais être écrasée");

    // Renommage automatique.
    spec.conflict = Conflict::Rename;
    let out = e.runner.run(&spec, &cancel, &no_events).unwrap().output.unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "-image é (1).png");
    assert_eq!(std::fs::read(&src).unwrap(), PNG_1X1);

    // Conversion puis conflit : renommer, ignorer, remplacer.
    let mut bmp = JobSpec::new(&src, "bmp");
    let a = e.runner.run(&bmp, &cancel, &no_events).unwrap().output.unwrap();
    let b = e.runner.run(&bmp, &cancel, &no_events).unwrap().output.unwrap();
    assert_ne!(a, b);
    assert!(b.to_string_lossy().ends_with("(1).bmp"));
    bmp.conflict = Conflict::Skip;
    let skipped = e.runner.run(&bmp, &cancel, &no_events).unwrap();
    assert!(skipped.output.is_none());
    std::fs::write(&a, b"ancien contenu").unwrap();
    bmp.conflict = Conflict::Overwrite;
    let c = e.runner.run(&bmp, &cancel, &no_events).unwrap().output.unwrap();
    assert_eq!(c, a);
    assert_eq!(&std::fs::read(&c).unwrap()[..2], b"BM");
    assert_eq!(jobs_left(&e), 0, "répertoires de travail nettoyés");
}

fn make_media(ffmpeg: &Path, args: &[&str], out: &Path) {
    let spec = ProcessSpec::new(ffmpeg).args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"]).args(args.iter().copied()).arg(out);
    let o = chud_process::run_capture(&spec, 0).unwrap();
    assert!(o.success(), "{}", o.stderr_tail);
}

#[test]
fn cancellation_kills_engine_and_cleans_up() {
    let e = env("cancel");
    let Some(ffmpeg) = e.ffmpeg.clone() else {
        skip("FFmpeg absent");
        return;
    };
    let src = e.dir.join("long.mkv");
    make_media(
        &ffmpeg,
        &["-f", "lavfi", "-i", "testsrc2=duration=30:size=1280x720:rate=30", "-c:v", "libx264", "-preset", "ultrafast"],
        &src,
    );
    let out_dir = e.dir.join("sortie");
    let mut spec = JobSpec::new(&src, "webm");
    spec.output_dir = Some(out_dir.clone());
    spec.options.parse_assignment("speed=slow").unwrap();
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        c2.cancel();
    });
    let started = std::time::Instant::now();
    let r = e.runner.run(&spec, &cancel, &no_events);
    killer.join().unwrap();
    assert!(matches!(r, Err(JobError::Cancelled)), "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(15), "l'annulation doit être rapide");
    assert!(std::fs::read_dir(&out_dir).map(|r| r.count()).unwrap_or(0) == 0, "aucune sortie partielle");
    assert_eq!(jobs_left(&e), 0, "répertoire de travail supprimé");
}

#[test]
fn corrupted_input_is_never_reported_as_success() {
    let e = env("corrupt");
    let Some(ffmpeg) = e.ffmpeg.clone() else {
        skip("FFmpeg absent");
        return;
    };
    let good = e.dir.join("bon.mp4");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "testsrc=duration=3:size=320x240", "-c:v", "libx264"], &good);
    let bytes = std::fs::read(&good).unwrap();
    let bad = e.dir.join("tronqué.mp4");
    std::fs::write(&bad, &bytes[..bytes.len() / 2]).unwrap();
    let r = e.runner.run(&JobSpec::new(&bad, "mkv"), &CancelToken::new(), &no_events);
    assert!(r.is_err(), "un fichier tronqué ne doit pas produire un « succès » : {r:?}");
    assert!(!e.dir.join("tronqué.mkv").exists());
    assert_eq!(jobs_left(&e), 0);
}

#[test]
fn queue_accepts_cancels_and_reports_each_job() {
    let e = env("queue");
    let files: Vec<PathBuf> = (1..=3).map(|i| e.dir.join(format!("img-{i}.png"))).collect();
    for f in &files {
        std::fs::write(f, PNG_1X1).unwrap();
    }
    // Le premier démarrage bloque l'unique fil : les tâches 2 et 3 restent en attente, ce qui
    // rend l'annulation d'une tâche en attente déterministe.
    let events: Arc<Mutex<Vec<QueueEvent>>> = Arc::default();
    let hold: Arc<(Mutex<(bool, bool)>, Condvar)> = Arc::default(); // (démarré, relâché)
    let (ev, h) = (events.clone(), hold.clone());
    let listener = Arc::new(move |event: QueueEvent| {
        if matches!(event, QueueEvent::Started { id: 1 }) {
            let (m, cv) = &*h;
            let mut g = m.lock().unwrap();
            g.0 = true;
            cv.notify_all();
            while !g.1 {
                g = cv.wait(g).unwrap();
            }
        }
        ev.lock().unwrap().push(event);
    });
    let queue = JobQueue::new(e.runner.clone(), 1, 4, listener);
    let ids: Vec<u64> = files.iter().map(|f| queue.submit(JobSpec::new(f, "bmp"))).collect();
    {
        let (m, cv) = &*hold;
        let g = cv.wait_timeout_while(m.lock().unwrap(), Duration::from_secs(20), |g| !g.0).unwrap().0;
        assert!(g.0, "la première tâche n'a pas démarré");
    }
    assert_eq!(queue.counts(), (2, 1));
    assert!(queue.cancel(ids[2]), "tâche en attente annulable");
    {
        let (m, cv) = &*hold;
        m.lock().unwrap().1 = true;
        cv.notify_all();
    }
    let finished = |evs: &[QueueEvent], id: u64| {
        evs.iter().any(|x| matches!(x, QueueEvent::Done { id: i, .. } | QueueEvent::Failed { id: i, .. } if *i == id))
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !ids.iter().all(|&id| finished(&events.lock().unwrap(), id)) {
        assert!(std::time::Instant::now() < deadline, "file bloquée");
        std::thread::sleep(Duration::from_millis(20));
    }
    let evs = events.lock().unwrap().clone();
    for (n, &id) in ids.iter().enumerate().take(2) {
        let pos = |pred: &dyn Fn(&QueueEvent) -> bool| evs.iter().position(pred).unwrap();
        let started = pos(&|x| matches!(x, QueueEvent::Started { id: i } if *i == id));
        let done = pos(&|x| matches!(x, QueueEvent::Done { id: i, .. } if *i == id));
        assert!(started < done);
        let QueueEvent::Done { outcome, .. } = &evs[done] else { unreachable!() };
        let out = outcome.output.clone().unwrap();
        assert_eq!(out, e.dir.join(format!("img-{}.bmp", n + 1)));
        assert_eq!(&std::fs::read(&out).unwrap()[..2], b"BM");
    }
    assert!(evs.iter().any(|x| matches!(x, QueueEvent::Failed { id, failure } if *id == ids[2] && failure.code == "cancelled")));
    assert!(!evs.iter().any(|x| matches!(x, QueueEvent::Started { id } if *id == ids[2])));
    assert!(!e.dir.join("img-3.bmp").exists());
    assert!(!queue.cancel(ids[0]), "une tâche terminée n'est plus annulable");
    assert!(queue.is_idle());
    drop(queue);
    assert_eq!(jobs_left(&e), 0);
}

/// Chaque préréglage livré fonctionne sur un vrai fichier de chaque sorte qu'il vise, et toutes
/// ses options existent réellement : une clé mal orthographiée serait sinon ignorée en silence.
#[test]
fn builtin_presets_work_on_real_files() {
    use chud_core::{EngineId, FormatCatalog};
    use chud_jobs::{PresetStore, kind_of_path};

    let e = env("presets");
    let Some(ffmpeg) = e.ffmpeg.clone() else {
        return skip("FFmpeg introuvable");
    };
    if e.runner.registry().engine(&EngineId::new("7zip")).is_none() {
        return skip("7-Zip introuvable");
    }
    let src = e.dir.join("sources");
    std::fs::create_dir_all(&src).unwrap();
    let video = src.join("film.mov");
    make_media(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=15:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ],
        &video,
    );
    let audio = src.join("son.wav");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=1"], &audio);
    let image = src.join("image.png");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "testsrc2=size=64x48", "-frames:v", "1"], &image);
    let data = src.join("liste.json");
    std::fs::write(&data, r#"[{"nom":"a","n":1},{"nom":"b","n":2}]"#).unwrap();
    let subs = src.join("dialogues.srt");
    std::fs::write(&subs, "1\n00:00:00,000 --> 00:00:01,000\nBonjour\n").unwrap();
    // Un second fichier par sorte, dans un autre format : « WAV » n'est pas proposé pour un WAV.
    let audio2 = src.join("son.mp3");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=1"], &audio2);
    let image2 = src.join("image.jpg");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "testsrc2=size=64x48", "-frames:v", "1"], &image2);
    let data2 = src.join("liste.csv");
    std::fs::write(&data2, "nom,n\na,1\nb,2\n").unwrap();
    let webvtt = src.join("dialogues.vtt");
    std::fs::write(&webvtt, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nBonjour\n").unwrap();
    let folder = src.join("dossier");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), "a").unwrap();

    let store = PresetStore::builtin();
    let zip_spec = store.get("zip").unwrap().job_spec(&folder);
    let archive = e.runner.run(&zip_spec, &CancelToken::new(), &no_events).unwrap().output.unwrap();
    let seven_spec = store.get("7z").unwrap().job_spec(&folder);
    let archive2 = e.runner.run(&seven_spec, &CancelToken::new(), &no_events).unwrap().output.unwrap();
    let samples = [&video, &audio, &audio2, &image, &image2, &data, &data2, &subs, &webvtt, &folder, &archive, &archive2];

    let catalog = FormatCatalog::builtin();
    let out = e.dir.join("sorties");
    std::fs::create_dir_all(&out).unwrap();
    for p in store.presets.iter() {
        let mut tried = 0;
        for kind in &p.applies_to {
            // Un échantillon de cette sorte pour lequel le préréglage serait proposé (« SRT » ne
            // l'est pas pour un fichier déjà en SRT, voir PresetStore::for_paths).
            let Some(sample) = samples.iter().find(|s| {
                let name = s.file_name().unwrap().to_string_lossy().into_owned();
                kind_of_path(s, catalog).as_deref() == Some(kind.as_str())
                    && !catalog.guess_from_name(&name).is_some_and(|f| p.is_pointless_for(&f.id))
            }) else {
                panic!("pas d'échantillon pour la sorte « {kind} » du préréglage {}", p.id);
            };
            let mut spec = p.job_spec(sample.as_path());
            spec.output_dir = Some(out.clone());
            let prepared = e.runner.prepare(&spec).unwrap_or_else(|err| panic!("{} sur {kind} : {err}", p.id));
            let known: Vec<String> = prepared
                .plan
                .steps
                .iter()
                .flat_map(|s| e.runner.registry().engine(&s.engine).map(|en| en.options(&s.from, &s.to)).unwrap_or_default())
                .map(|o| o.key)
                .collect();
            for key in p.options.keys() {
                assert!(known.contains(key), "{} : option « {key} » inconnue pour {kind} (connues : {known:?})", p.id);
            }
            let r = e.runner.run(&spec, &CancelToken::new(), &no_events).unwrap_or_else(|err| panic!("{} sur {kind} : {err}", p.id));
            assert!(r.output.is_some(), "{} sur {kind} : aucune sortie", p.id);
            tried += 1;
        }
        assert!(tried > 0, "{} jamais essayé", p.id);
    }
    assert_eq!(jobs_left(&e), 0, "répertoires de travail nettoyés");
}
