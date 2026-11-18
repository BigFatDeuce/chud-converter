//! Tests réels de l'adaptateur FFmpeg. Ignorés (avec un message « SKIP ») si FFmpeg est absent.
//!
//! Les médias de test sont générés à la volée par FFmpeg (mire et sinusoïde), aucun fichier binaire
//! n'est stocké dans le dépôt.

use chud_core::{Engine, EngineOrigin, ExecContext, Fidelity, FormatId, OptionValue, OptionValues, StepQuery, StepTask, Verification};
use chud_engines::ffmpeg::Ffmpeg;
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};

fn which(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&exe)).find(|p| p.is_file())
}

/// Moteur absent : test ignoré, sauf si `CHUD_REQUIRE_ENGINES` est défini (CI), où c'est un échec.
fn skip(why: &str) {
    if std::env::var_os("CHUD_REQUIRE_ENGINES").is_some() {
        panic!("{why} alors que CHUD_REQUIRE_ENGINES est défini");
    }
    eprintln!("SKIP : {why}");
}

fn tools() -> Option<(PathBuf, PathBuf)> {
    let (Some(f), Some(p)) = (which("ffmpeg"), which("ffprobe")) else {
        skip("ffmpeg/ffprobe introuvables dans le PATH");
        return None;
    };
    Some((f, p))
}

fn make_media(ffmpeg: &Path, args: &[&str], out: &Path) {
    let spec = ProcessSpec::new(ffmpeg).args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"]).args(args.iter().copied()).arg(out);
    let o = chud_process::run_capture(&spec, 0).expect("lancement ffmpeg");
    assert!(o.success(), "génération échouée : {}", o.stderr_tail);
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chud-ffmpeg-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Run {
    fidelity: Fidelity,
    notes: Vec<String>,
    verification: Verification,
}

fn convert(e: &Ffmpeg, input: &Path, from: &str, to: &str, out: &Path, opts: OptionValues) -> Run {
    let (from, to) = (FormatId::from(from), FormatId::from(to));
    let source = e.probe(input, &from);
    let specs = e.options(&from, &to);
    let options = opts.resolve(&specs).expect("options valides");
    let a = e.assess(&StepQuery { from: &from, to: &to, source: source.as_ref(), options: &options });
    assert!(a.unavailable.is_none(), "indisponible : {:?}", a.unavailable);
    let task = StepTask { input: input.into(), output: out.into(), from, to, options, source };
    let ctx = ExecContext::new(out.parent().unwrap().into());
    e.execute(&task, &ctx).unwrap_or_else(|err| panic!("échec {} → {} : {err}", task.from, task.to));
    let verification = e.verify(out, &task, &ctx);
    Run { fidelity: a.fidelity, notes: a.notes, verification }
}

fn pcm_md5(ffmpeg: &Path, file: &Path) -> String {
    let spec = ProcessSpec::new(ffmpeg).args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"]).arg(file).args([
        "-map",
        "0:a:0",
        "-c:a",
        "pcm_s16le",
        "-f",
        "md5",
        "-",
    ]);
    let o = chud_process::run_capture(&spec, 4096).unwrap();
    assert!(o.success(), "{}", o.stderr_tail);
    o.stdout_text().trim().to_owned()
}

#[test]
fn real_ffmpeg_conversions() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe), None, EngineOrigin::System).expect("sonde des capacités");
    let dir = workdir("main");

    // Source vidéo au nom piégeux : tiret initial, accents, idéogrammes, espaces.
    let srt = dir.join("subs.srt");
    std::fs::write(&srt, "1\r\n00:00:00,500 --> 00:00:01,500\r\nBonjour à tous\r\n").unwrap();
    let src = dir.join("-é 文件 test.mkv");
    make_media(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-i",
            srt.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
        ],
        &src,
    );
    let before = std::fs::read(&src).unwrap();

    let info = e.probe(&src, &FormatId::from("mkv")).expect("sonde ffprobe");
    assert_eq!(info.streams.len(), 3, "{:?}", info.streams);
    assert!((info.duration.unwrap() - 3.0).abs() < 0.2);

    // Remux MKV → MP4 : vidéo et audio copiés, sous-titres SRT convertis en mov_text.
    let r = convert(&e, &src, "mkv", "mp4", &dir.join("remux.mp4"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossless, "{:?}", r.notes);
    assert!(matches!(r.verification, Verification::Decoded(_)), "{:?}", r.verification);

    // MKV → WebM : ré-encodage annoncé avec perte.
    let r = convert(&e, &src, "mkv", "webm", &dir.join("out.webm"), OptionValues::new().with("speed", OptionValue::Text("fast".into())));
    assert_eq!(r.fidelity, Fidelity::Lossy);
    assert!(r.verification.is_ok(), "{:?}", r.verification);

    // Extraction audio, GIF, image fixe, sous-titres.
    let r = convert(&e, &src, "mkv", "mp3", &dir.join("audio.mp3"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossy);
    assert!(matches!(r.verification, Verification::Decoded(_)));
    let r = convert(&e, &src, "mkv", "gif", &dir.join("anim.gif"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    let r = convert(&e, &src, "mkv", "png", &dir.join("frame.png"), OptionValues::new().with("time", OptionValue::Text("1".into())));
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    assert_eq!(&std::fs::read(dir.join("frame.png")).unwrap()[..4], b"\x89PNG");
    let r = convert(&e, &src, "mkv", "srt", &dir.join("extrait.srt"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossless);
    assert!(std::fs::read_to_string(dir.join("extrait.srt")).unwrap().contains("Bonjour à tous"));

    // Découpe validée.
    let r = convert(
        &e,
        &src,
        "mkv",
        "mp4",
        &dir.join("cut.mp4"),
        OptionValues::new().with("start", OptionValue::Text("1".into())).with("video_codec", OptionValue::Text("h264".into())),
    );
    assert!(r.verification.is_ok(), "{:?}", r.verification);

    // La source n'a pas été modifiée.
    assert_eq!(before, std::fs::read(&src).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_ffmpeg_audio_is_bit_exact() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe), None, EngineOrigin::System).unwrap();
    let dir = workdir("audio");
    let wav = dir.join("source.wav");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=frequency=1000:duration=2:sample_rate=44100", "-ac", "2", "-c:a", "pcm_s16le"], &wav);

    let r = convert(&e, &wav, "wav", "flac", &dir.join("a.flac"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossless);
    assert!(matches!(r.verification, Verification::Decoded(_)));
    assert_eq!(pcm_md5(&ffmpeg, &wav), pcm_md5(&ffmpeg, &dir.join("a.flac")), "FLAC doit être identique à l'échantillon près");

    let r = convert(&e, &dir.join("a.flac"), "flac", "alac", &dir.join("a.m4a"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossless);
    assert_eq!(pcm_md5(&ffmpeg, &wav), pcm_md5(&ffmpeg, &dir.join("a.m4a")));

    // 16 bits forcés depuis une source 16 bits : toujours sans perte ; rééchantillonnage : avec perte.
    let r = convert(&e, &wav, "wav", "wav", &dir.join("b.wav"), OptionValues::new().with("sample_rate", OptionValue::Int(22050)));
    assert_eq!(r.fidelity, Fidelity::Lossy);
    assert!(r.notes.iter().any(|n| n.contains("rééchantillonnage")), "{:?}", r.notes);

    // Sous-titres Windows-1252 : l'encodage est détecté et transmis à FFmpeg.
    let srt = dir.join("cp1252.srt");
    std::fs::write(&srt, b"1\r\n00:00:01,000 --> 00:00:02,000\r\nCaf\xe9 cr\xe8me \xe0 volont\xe9\r\n").unwrap();
    let r = convert(&e, &srt, "srt", "vtt", &dir.join("out.vtt"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    let vtt = std::fs::read_to_string(dir.join("out.vtt")).unwrap();
    assert!(vtt.contains("Café crème à volonté"), "{vtt}");
    let _ = std::fs::remove_dir_all(&dir);
}
