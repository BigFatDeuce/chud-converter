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
    let task = StepTask { input: input.into(), output: out.into(), from, to, options, source, name: None };
    let ctx = ExecContext::new(out.parent().unwrap().into());
    e.execute(&task, &ctx).unwrap_or_else(|err| panic!("échec {} → {} : {err}", task.from, task.to));
    let verification = e.verify(out, &task, &ctx);
    Run { fidelity: a.fidelity, notes: a.notes, verification }
}

/// Conversion possible avec ce FFmpeg ? Certains encodeurs sont optionnels (l'AMR n'est pas dans
/// le FFmpeg d'Ubuntu) : le logiciel le dit, et le test le signale au lieu d'échouer.
fn offered(e: &Ffmpeg, from: &str, to: &str) -> bool {
    let (f, t) = (FormatId::from(from), FormatId::from(to));
    let options = OptionValues::new().resolve(&e.options(&f, &t)).unwrap();
    let a = e.assess(&StepQuery { from: &f, to: &t, source: None, options: &options });
    if let Some(why) = a.unavailable {
        eprintln!("SKIP {from} → {to} : {why}");
        return false;
    }
    true
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

fn audio_codec(ffprobe: &Path, file: &Path) -> String {
    let spec = ProcessSpec::new(ffprobe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name", "-of", "default=nw=1:nk=1"])
        .arg(file);
    let o = chud_process::run_capture(&spec, 4096).unwrap();
    assert!(o.success(), "{}", o.stderr_tail);
    o.stdout_text().trim().to_owned()
}

/// Régressions signalées sur un vrai FLAC de 3 minutes :
/// - « Ogg Vorbis » contenait le FLAC copié tel quel, annoncé « sans perte » ;
/// - AAC (ADTS) et AMR déclarés en échec : ffprobe estime leur durée d'après le débit
///   (198,7 s et 192,9 s pour 186,9 s) ; c'est la durée décodée qui doit compter.
#[test]
fn real_ffmpeg_raw_streams_and_ogg_vorbis() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe.clone()), None, EngineOrigin::System).unwrap();
    let dir = workdir("raw");
    let flac = dir.join("source.flac");
    make_media(
        &ffmpeg,
        &["-f", "lavfi", "-i", "sine=frequency=440:duration=90:sample_rate=44100", "-ac", "2", "-c:a", "flac", "-sample_fmt", "s16"],
        &flac,
    );

    let ogg = dir.join("a.ogg");
    let r = convert(&e, &flac, "flac", "ogg", &ogg, OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossy, "Vorbis est un format avec perte");
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    assert_eq!(audio_codec(&ffprobe, &ogg), "vorbis");

    for (to, file) in [("aac", "a.aac"), ("amr", "a.amr"), ("eac3", "a.eac3"), ("ac3", "a.ac3"), ("mp3", "a.mp3")] {
        if to == "amr" && !offered(&e, "flac", to) {
            continue;
        }
        let r = convert(&e, &flac, "flac", to, &dir.join(file), OptionValues::new());
        match &r.verification {
            Verification::Decoded(d) => assert!(d.contains("90.0") || d.contains("89.9") || d.contains("90.1"), "{to} : {d}"),
            other => panic!("{to} : {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn probe_entry(ffprobe: &Path, file: &Path, entries: &str) -> String {
    let spec = ProcessSpec::new(ffprobe).args(["-v", "error", "-show_entries", entries, "-of", "default=nw=1:nk=1"]).arg(file);
    let o = chud_process::run_capture(&spec, 4096).unwrap();
    assert!(o.success(), "{}", o.stderr_tail);
    o.stdout_text().trim().to_owned()
}

/// Régressions de l'audit des formats audio : chaque cas échouait avant correction.
#[test]
fn real_ffmpeg_audio_regressions() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe.clone()), None, EngineOrigin::System).unwrap();
    let dir = workdir("audioreg");

    // Balises Ogg : rangées par flux, elles étaient perdues vers MP3.
    let ogg = dir.join("tags.ogg");
    make_media(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-c:a",
            "libvorbis",
            "-metadata:s:a:0",
            "title=Chanson été",
            "-metadata:s:a:0",
            "artist=Groupe",
        ],
        &ogg,
    );
    let mp3 = dir.join("tags.mp3");
    let r = convert(&e, &ogg, "ogg", "mp3", &mp3, OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    assert_eq!(probe_entry(&ffprobe, &mp3, "format_tags=title"), "Chanson été");

    // Opus se décode toujours à 48 kHz : une source à 8 kHz n'est pas un écart de format.
    let low = dir.join("8k.wav");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=2:sample_rate=8000", "-ac", "1"], &low);
    let r = convert(&e, &low, "wav", "opus", &dir.join("8k.opus"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    // AMR : 8 kHz mono imposé, la vérification attend ce format-là.
    if offered(&e, "wav", "amr") {
        let r = convert(&e, &dir.join("8k.wav"), "wav", "amr", &dir.join("8k.amr"), OptionValues::new());
        assert!(r.verification.is_ok(), "{:?}", r.verification);
    }

    // CAF : PCM exact, y compris depuis du 8 bits non signé (refusé tel quel par le multiplexeur).
    let u8 = dir.join("u8.wav");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=2", "-c:a", "pcm_u8"], &u8);
    for (src, name) in [(&u8, "u8.caf"), (&low, "8k.caf")] {
        let r = convert(&e, src, "wav", "caf", &dir.join(name), OptionValues::new());
        assert_eq!(r.fidelity, Fidelity::Lossless, "{name}");
        assert!(r.verification.is_ok(), "{name} : {:?}", r.verification);
    }

    // Débit MP2 invalide aux fréquences basses (MPEG-2 LSF) : une vidéo 3GP à 22,05 kHz vers MPEG.
    let gp = dir.join("lsf.3gp");
    make_media(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=176x144:rate=15",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2:sample_rate=22050",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-ac",
            "1",
        ],
        &gp,
    );
    let r = convert(&e, &gp, "3gp", "mpeg", &dir.join("lsf.mpg"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);

    // Ré-encodage sans perte prouvé : WavPack depuis du FLAC, échantillons identiques.
    let flac = dir.join("src.flac");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=3:sample_rate=44100", "-ac", "2", "-c:a", "flac"], &flac);
    let r = convert(&e, &flac, "flac", "wv", &dir.join("a.wv"), OptionValues::new());
    assert_eq!(r.fidelity, Fidelity::Lossless);
    match &r.verification {
        Verification::Decoded(d) => assert!(d.contains("identi"), "{d}"),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Source tronquée : l'en-tête annonce plus que ce qui est lisible. Ce n'est pas une erreur de
/// conversion si tout ce qui est lisible a été converti, et le résultat doit le dire.
#[test]
fn real_ffmpeg_truncated_source_is_diagnosed() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe), None, EngineOrigin::System).unwrap();
    let dir = workdir("trunc");
    let full = dir.join("full.flac");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "sine=duration=10:sample_rate=44100", "-c:a", "flac"], &full);
    let data = std::fs::read(&full).unwrap();
    let cut = dir.join("cut.flac");
    std::fs::write(&cut, &data[..data.len() / 2]).unwrap();
    let r = convert(&e, &cut, "flac", "mp3", &dir.join("cut.mp3"), OptionValues::new());
    match &r.verification {
        Verification::Decoded(d) => assert!(d.contains("incompl"), "{d}"),
        Verification::Failed(f) => assert!(f.contains("source"), "le message doit désigner la source : {f}"),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Régressions vidéo et image de l'audit.
#[test]
fn real_ffmpeg_video_regressions() {
    let Some((ffmpeg, ffprobe)) = tools() else { return };
    let e = Ffmpeg::new(ffmpeg.clone(), Some(ffprobe.clone()), None, EngineOrigin::System).unwrap();
    let dir = workdir("videoreg");
    let mkv = dir.join("h264.mkv");
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
            "sine=duration=3",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "mp3",
        ],
        &mkv,
    );
    // H.264 copié de MKV vers AVI : il faut le flux « Annex B ».
    let avi = dir.join("copy.avi");
    let r = convert(&e, &mkv, "mkv", "avi", &avi, OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    assert_eq!(probe_entry(&ffprobe, &avi, "stream=codec_name").lines().next(), Some("h264"));

    // Cadence imposée : retrouvée dans le résultat ; l'audio reste synchrone.
    let r = convert(&e, &mkv, "mkv", "mp4", &dir.join("15fps.mp4"), OptionValues::new().with("fps", OptionValue::Float(15.0)));
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    let rate = probe_entry(&ffprobe, &dir.join("15fps.mp4"), "stream=avg_frame_rate");
    assert!(rate.lines().any(|l| l == "15/1"), "{rate}");

    // Image extraite d'un MPEG-TS : la recherche avant l'entrée ne donnait aucune image.
    let ts = dir.join("a.ts");
    let r = convert(&e, &mkv, "mkv", "ts", &ts, OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    let r = convert(&e, &ts, "ts", "png", &dir.join("frame.png"), OptionValues::new().with("time", OptionValue::Text("1.5".into())));
    assert!(r.verification.is_ok(), "{:?}", r.verification);

    // Dimensions impaires : arrondies au pair pour H.264, et la vérification l'accepte.
    let odd = dir.join("odd.mkv");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "testsrc=duration=2:size=321x241:rate=25", "-c:v", "ffv1"], &odd);
    let r = convert(&e, &odd, "mkv", "mp4", &dir.join("odd.mp4"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);

    // WebP fixe vers vidéo : le démultiplexeur d'animation le refusait.
    let webp = dir.join("still.webp");
    make_media(&ffmpeg, &["-f", "lavfi", "-i", "testsrc=size=128x96", "-frames:v", "1", "-c:v", "libwebp"], &webp);
    let r = convert(&e, &webp, "webp", "mp4", &dir.join("still.mp4"), OptionValues::new());
    assert!(r.verification.is_ok(), "{:?}", r.verification);
    let _ = std::fs::remove_dir_all(&dir);
}
