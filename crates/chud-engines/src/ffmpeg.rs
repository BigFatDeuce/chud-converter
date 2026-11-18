//! Adaptateur FFmpeg / ffprobe : audio, vidéo, animations, sous-titres et quelques images.
//!
//! Règles de sécurité appliquées à chaque appel :
//! - programme désigné par chemin absolu, arguments séparés, aucun shell ;
//! - chemins préfixés par `file:` (un nom de fichier ne peut jamais être pris pour une URL ou un protocole) ;
//! - `-protocol_whitelist file,pipe` : aucune ressource réseau ou de liste de lecture ;
//! - démultiplexeur **imposé** (`-f`) d'après notre propre détection : une liste HLS ou `concat`
//!   déguisée en `.mp4` n'est jamais interprétée ;
//! - filtres construits uniquement à partir de nombres validés, jamais de texte utilisateur.
//!
//! Règles d'honnêteté :
//! - chaque flux est soit **copié** (sans perte), soit **ré-encodé** (avec perte sauf codec sans perte) ;
//!   la décision est la même pour l'évaluation (`assess`) et l'exécution (`execute`) ;
//! - les conversions d'échantillons (flottant → entier, 24 → 16 bits, rééchantillonnage, mixage)
//!   sont signalées ;
//! - le résultat est relu par ffprobe (flux attendus, durée) puis décodé intégralement quand c'est
//!   raisonnable.

use crate::text;
use crate::util::{parse_time, run_tool, tail_lines};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, Feature, Fidelity, FormatCatalog, FormatId,
    OptionSpec, OptionValues, Progress, SourceInfo, StepAssessment, StepOutput, StepQuery, StepTask, StreamInfo, StreamKind, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------------------------------------
// Tables de formats et de codecs
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Video,
    Audio,
    Subtitle,
    /// Animations (GIF, APNG, WebP animé).
    Anim,
    /// Images fixes (extraction d'image, AVIF, WebP avec perte).
    Image,
}

struct Fmt {
    id: &'static str,
    kind: Kind,
    /// Démultiplexeurs candidats, le premier disponible est imposé avec `-f`.
    demux: &'static [&'static str],
    /// Multiplexeur ("" = lecture seule).
    mux: &'static str,
    mux_args: &'static [&'static str],
    /// Familles vidéo encodables (la première disponible est la valeur par défaut).
    video: &'static [&'static str],
    /// Codecs vidéo supplémentaires acceptés en copie.
    vcopy: &'static [&'static str],
    audio: &'static [&'static str],
    acopy: &'static [&'static str],
    /// Codecs de sous-titres acceptés (le premier codec texte encodable sert de cible).
    subs: &'static [&'static str],
    /// Accepte presque tous les codecs en copie (Matroska).
    any: bool,
}

const N: &[&str] = &[];
const FASTSTART: &[&str] = &["-movflags", "+faststart"];

const fn fmt(id: &'static str, kind: Kind, demux: &'static [&'static str], mux: &'static str) -> Fmt {
    Fmt { id, kind, demux, mux, mux_args: N, video: N, vcopy: N, audio: N, acopy: N, subs: N, any: false }
}

const MKV_AUDIO: &[&str] = &["aac", "opus", "flac", "vorbis", "mp3", "ac3", "eac3", "alac", "wavpack", "tta", "pcm_le", "mp2"];

static FORMATS: &[Fmt] = &[
    // --- Vidéo ---
    Fmt {
        mux_args: FASTSTART,
        video: &["h264", "h265", "av1", "vp9", "mpeg4"],
        vcopy: &["mpeg2video", "mjpeg"],
        audio: &["aac", "mp3", "ac3", "eac3", "opus", "flac", "alac"],
        acopy: &["mp2"],
        subs: &["mov_text"],
        ..fmt("mp4", Kind::Video, &["mov"], "mp4")
    },
    Fmt {
        video: &["h264", "h265", "av1", "vp9", "vp8", "mpeg4", "mpeg2", "theora", "prores"],
        audio: MKV_AUDIO,
        subs: &["subrip", "ass", "ssa", "webvtt", "text", "hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle"],
        any: true,
        ..fmt("mkv", Kind::Video, &["matroska"], "matroska")
    },
    Fmt { video: &["vp9", "av1", "vp8"], audio: &["opus", "vorbis"], subs: &["webvtt"], ..fmt("webm", Kind::Video, &["matroska"], "webm") },
    Fmt {
        video: &["mpeg4", "h264", "mpeg2"],
        vcopy: &["mjpeg", "msmpeg4v3", "msmpeg4v2", "rawvideo", "huffyuv", "ffv1", "dvvideo"],
        audio: &["mp3", "ac3", "mp2", "pcm_le"],
        ..fmt("avi", Kind::Video, &["avi"], "avi")
    },
    Fmt {
        mux_args: FASTSTART,
        video: &["h264", "h265", "prores", "mpeg4", "av1"],
        vcopy: &["mjpeg", "dnxhd", "rawvideo", "qtrle", "png"],
        audio: &["aac", "alac", "pcm_le", "mp3", "ac3", "eac3"],
        acopy: &["pcm_s16be", "pcm_s24be", "pcm_s32be", "pcm_f32be"],
        subs: &["mov_text"],
        ..fmt("mov", Kind::Video, &["mov"], "mov")
    },
    Fmt {
        video: &["wmv2"],
        vcopy: &["wmv1", "wmv3", "vc1", "msmpeg4v3"],
        audio: &["wma"],
        acopy: &["wmav1", "wmapro", "mp3"],
        ..fmt("wmv", Kind::Video, &["asf"], "asf")
    },
    Fmt { video: &["h264", "flv1"], audio: &["aac", "mp3"], ..fmt("flv", Kind::Video, &["flv"], "flv") },
    Fmt { video: &["mpeg2", "mpeg1"], audio: &["mp2", "ac3", "mp3"], ..fmt("mpeg", Kind::Video, &["mpeg"], "mpeg") },
    Fmt {
        video: &["h264", "h265", "mpeg2", "av1"],
        audio: &["aac", "ac3", "eac3", "mp2", "mp3", "opus"],
        subs: &["dvb_subtitle"],
        ..fmt("ts", Kind::Video, &["mpegts"], "mpegts")
    },
    Fmt {
        mux_args: &["-mpegts_m2ts_mode", "1"],
        video: &["h264", "h265", "mpeg2"],
        vcopy: &["vc1"],
        audio: &["ac3", "eac3", "aac"],
        acopy: &["pcm_bluray", "dts", "truehd"],
        subs: &["hdmv_pgs_subtitle"],
        ..fmt("m2ts", Kind::Video, &["mpegts"], "mpegts")
    },
    Fmt {
        mux_args: FASTSTART,
        video: &["h264", "mpeg4"],
        vcopy: &["h263"],
        audio: &["aac", "amr"],
        acopy: &["amr_wb"],
        subs: &["mov_text"],
        ..fmt("3gp", Kind::Video, &["mov"], "3gp")
    },
    Fmt {
        video: &["mpeg2"],
        audio: &["ac3", "mp2"],
        acopy: &["pcm_dvd", "dts"],
        subs: &["dvd_subtitle"],
        ..fmt("vob", Kind::Video, &["mpeg"], "vob")
    },
    Fmt { video: &["theora", "vp8"], audio: &["vorbis", "opus", "flac"], ..fmt("ogv", Kind::Video, &["ogg"], "ogv") },
    fmt("mxf", Kind::Video, &["mxf"], ""),
    fmt("rm", Kind::Video, &["rm"], ""),
    fmt("dv", Kind::Video, &["dv"], ""),
    // --- Audio ---
    Fmt { mux_args: &["-id3v2_version", "3"], audio: &["mp3"], ..fmt("mp3", Kind::Audio, &["mp3"], "mp3") },
    Fmt { mux_args: &["-rf64", "auto"], audio: &["pcm_le"], ..fmt("wav", Kind::Audio, &["wav"], "wav") },
    Fmt { audio: &["flac"], ..fmt("flac", Kind::Audio, &["flac"], "flac") },
    Fmt { audio: &["aac"], ..fmt("aac", Kind::Audio, &["aac"], "adts") },
    Fmt { mux_args: FASTSTART, audio: &["aac", "alac"], ..fmt("m4a", Kind::Audio, &["mov"], "ipod") },
    Fmt { mux_args: FASTSTART, audio: &["alac"], ..fmt("alac", Kind::Audio, &["mov"], "ipod") },
    Fmt { audio: &["vorbis", "opus", "flac"], ..fmt("ogg", Kind::Audio, &["ogg"], "ogg") },
    Fmt { audio: &["opus"], ..fmt("opus", Kind::Audio, &["ogg"], "opus") },
    Fmt { audio: &["wma"], acopy: &["wmav1", "wmapro", "wmalossless"], ..fmt("wma", Kind::Audio, &["asf"], "asf") },
    Fmt { audio: &["pcm_be"], ..fmt("aiff", Kind::Audio, &["aiff"], "aiff") },
    fmt("ape", Kind::Audio, &["ape"], ""),
    Fmt { audio: &["wavpack"], ..fmt("wv", Kind::Audio, &["wv"], "wv") },
    Fmt { audio: &["tta"], ..fmt("tta", Kind::Audio, &["tta"], "tta") },
    Fmt { audio: &["ac3"], ..fmt("ac3", Kind::Audio, &["ac3"], "ac3") },
    Fmt { audio: &["eac3"], ..fmt("eac3", Kind::Audio, &["eac3"], "eac3") },
    fmt("dts", Kind::Audio, &["dts"], ""),
    Fmt { audio: MKV_AUDIO, any: true, ..fmt("mka", Kind::Audio, &["matroska"], "matroska") },
    Fmt { audio: &["amr"], acopy: &["amr_wb"], ..fmt("amr", Kind::Audio, &["amr"], "amr") },
    Fmt {
        audio: &["alac", "pcm_le", "aac", "opus"],
        acopy: &["pcm_s16be", "pcm_s24be", "pcm_s32be", "pcm_f32be", "mp3", "ac3", "flac"],
        ..fmt("caf", Kind::Audio, &["caf"], "caf")
    },
    Fmt { audio: &["pcm_be"], acopy: &["pcm_mulaw", "pcm_alaw"], ..fmt("au", Kind::Audio, &["au"], "au") },
    // DSF ou DFF : le démultiplexeur est choisi d'après la signature (voir `demuxer_for`).
    fmt("dsf", Kind::Audio, &["dsf", "iff"], ""),
    fmt("mod", Kind::Audio, &["libopenmpt", "libmodplug"], ""),
    // --- Sous-titres ---
    Fmt { subs: &["subrip"], ..fmt("srt", Kind::Subtitle, &["srt"], "srt") },
    Fmt { subs: &["webvtt"], ..fmt("vtt", Kind::Subtitle, &["webvtt"], "webvtt") },
    Fmt { subs: &["ass", "ssa"], ..fmt("ass", Kind::Subtitle, &["ass"], "ass") },
    Fmt { subs: &["ttml"], ..fmt("ttml", Kind::Subtitle, N, "ttml") },
    Fmt { subs: &["microdvd"], ..fmt("microdvd", Kind::Subtitle, &["microdvd"], "microdvd") },
    Fmt { subs: &["text"], ..fmt("lrc", Kind::Subtitle, &["lrc"], "lrc") },
    Fmt { subs: &["hdmv_pgs_subtitle"], ..fmt("sup", Kind::Subtitle, &["sup"], "sup") },
    // --- Animations ---
    fmt("gif", Kind::Anim, &["gif"], "gif"),
    fmt("apng", Kind::Anim, &["apng"], "apng"),
    fmt("webp", Kind::Anim, &["webp_anim", "webp_pipe"], "webp"),
    // --- Images fixes ---
    fmt("png", Kind::Image, &["png_pipe"], "image2"),
    fmt("jpeg", Kind::Image, &["jpeg_pipe"], "image2"),
    fmt("bmp", Kind::Image, &["bmp_pipe"], "image2"),
    fmt("tiff", Kind::Image, &["tiff_pipe"], "image2"),
    fmt("avif", Kind::Image, &["mov"], "avif"),
];

fn fmt_by_id(id: &str) -> Option<&'static Fmt> {
    FORMATS.iter().find(|f| f.id == id)
}

struct VFam {
    name: &'static str,
    codec: &'static str,
    encoders: &'static [&'static str],
    label: &'static str,
}

static VFAMS: &[VFam] = &[
    VFam { name: "h264", codec: "h264", encoders: &["libx264"], label: "H.264 (AVC)" },
    VFam { name: "h265", codec: "hevc", encoders: &["libx265"], label: "H.265 (HEVC)" },
    VFam { name: "av1", codec: "av1", encoders: &["libsvtav1", "libaom-av1"], label: "AV1" },
    VFam { name: "vp9", codec: "vp9", encoders: &["libvpx-vp9"], label: "VP9" },
    VFam { name: "vp8", codec: "vp8", encoders: &["libvpx"], label: "VP8" },
    VFam { name: "prores", codec: "prores", encoders: &["prores_ks"], label: "Apple ProRes" },
    VFam { name: "mpeg4", codec: "mpeg4", encoders: &["mpeg4"], label: "MPEG-4 Part 2" },
    VFam { name: "mpeg2", codec: "mpeg2video", encoders: &["mpeg2video"], label: "MPEG-2" },
    VFam { name: "mpeg1", codec: "mpeg1video", encoders: &["mpeg1video"], label: "MPEG-1" },
    VFam { name: "theora", codec: "theora", encoders: &["libtheora"], label: "Theora" },
    VFam { name: "wmv2", codec: "wmv2", encoders: &["wmv2"], label: "Windows Media Video 8" },
    VFam { name: "flv1", codec: "flv1", encoders: &["flv"], label: "Sorenson Spark" },
];

fn vfam(name: &str) -> Option<&'static VFam> {
    VFAMS.iter().find(|f| f.name == name)
}

struct AFam {
    name: &'static str,
    codec: &'static str,
    encoders: &'static [&'static str],
    lossless: bool,
    max_channels: u32,
    label: &'static str,
}

static AFAMS: &[AFam] = &[
    AFam { name: "aac", codec: "aac", encoders: &["aac"], lossless: false, max_channels: 8, label: "AAC" },
    AFam { name: "mp3", codec: "mp3", encoders: &["libmp3lame"], lossless: false, max_channels: 2, label: "MP3" },
    AFam { name: "opus", codec: "opus", encoders: &["libopus"], lossless: false, max_channels: 8, label: "Opus" },
    AFam { name: "vorbis", codec: "vorbis", encoders: &["libvorbis"], lossless: false, max_channels: 8, label: "Vorbis" },
    AFam { name: "flac", codec: "flac", encoders: &["flac"], lossless: true, max_channels: 8, label: "FLAC" },
    AFam { name: "alac", codec: "alac", encoders: &["alac"], lossless: true, max_channels: 8, label: "ALAC" },
    AFam { name: "ac3", codec: "ac3", encoders: &["ac3"], lossless: false, max_channels: 6, label: "Dolby Digital (AC-3)" },
    AFam { name: "eac3", codec: "eac3", encoders: &["eac3"], lossless: false, max_channels: 6, label: "Dolby Digital Plus" },
    AFam { name: "mp2", codec: "mp2", encoders: &["mp2"], lossless: false, max_channels: 2, label: "MP2" },
    AFam { name: "wma", codec: "wmav2", encoders: &["wmav2"], lossless: false, max_channels: 2, label: "WMA" },
    AFam { name: "wavpack", codec: "wavpack", encoders: &["wavpack"], lossless: true, max_channels: 8, label: "WavPack" },
    AFam { name: "tta", codec: "tta", encoders: &["tta"], lossless: true, max_channels: 8, label: "TTA" },
    AFam { name: "amr", codec: "amr_nb", encoders: &["libopencore_amrnb"], lossless: false, max_channels: 1, label: "AMR-NB" },
    AFam { name: "pcm_le", codec: "", encoders: &["pcm_s16le"], lossless: true, max_channels: 64, label: "PCM" },
    AFam { name: "pcm_be", codec: "", encoders: &["pcm_s16be"], lossless: true, max_channels: 64, label: "PCM" },
];

fn afam(name: &str) -> Option<&'static AFam> {
    AFAMS.iter().find(|f| f.name == name)
}

/// Codecs audio sans perte (le décodage restitue exactement le signal d'origine).
const LOSSLESS_AUDIO: &[&str] = &["flac", "alac", "wavpack", "tta", "ape", "mlp", "truehd", "mp4als", "tak", "shorten"];

/// Codecs de sous-titres texte que FFmpeg sait décoder.
const TEXT_SUBS: &[&str] = &[
    "subrip",
    "ass",
    "ssa",
    "webvtt",
    "mov_text",
    "text",
    "microdvd",
    "mpl2",
    "jacosub",
    "realtext",
    "sami",
    "subviewer",
    "subviewer1",
    "vplayer",
    "pjs",
    "stl",
    "eia_608",
];

/// Couvertures recopiées telles quelles dans ces formats audio.
const COVER_TARGETS: &[&str] = &["mp3", "flac", "m4a", "alac"];

/// Balises techniques ignorées quand on cherche de « vraies » métadonnées.
const TECH_TAGS: &[&str] =
    &["encoder", "major_brand", "minor_version", "compatible_brands", "handler_name", "vendor_id", "creation_time", "duration"];

fn family_accepts_audio(fam: &AFam, codec: &str) -> bool {
    match fam.name {
        "pcm_le" => codec.starts_with("pcm_") && (codec.ends_with("le") || codec == "pcm_u8"),
        "pcm_be" => codec.starts_with("pcm_") && (codec.ends_with("be") || codec == "pcm_s8"),
        _ => fam.codec == codec,
    }
}

fn sub_encoder(codec: &str) -> &str {
    if codec == "subrip" { "srt" } else { codec }
}

/// Lignes d'erreur FFmpeg lisibles : préfixes « [mov,mp4 @ 0x…] » retirés, doublons supprimés.
fn ffmpeg_lines(stderr: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in stderr.lines() {
        let mut l = line.trim();
        while l.starts_with('[') {
            match l.find("] ") {
                Some(i) if l[..i].contains(" @ ") => l = l[i + 2..].trim_start(),
                _ => break,
            }
        }
        if !l.is_empty() && !out.iter().any(|x| x == l) {
            out.push(l.to_owned());
        }
    }
    out
}

/// Messages d'erreur d'FFmpeg connus pour être sans conséquence **lorsque le décodeur ne signale
/// aucune erreur**. Toute autre erreur fait échouer la vérification.
const BENIGN_DECODE_MESSAGES: &[(&str, &str, &str)] = &[(
    // FFmpeg 8.0 + libopus 1.6 : l'analyseur de paquets Opus du démultiplexeur Matroska rejette un
    // paquet que les décodeurs (natif et libopus) décodent pourtant entièrement.
    "Error parsing Opus packet header",
    "message connu de l'analyseur Opus de FFmpeg 8.0 avec libopus 1.6, sans effet sur le décodage",
    "known message from the FFmpeg 8.0 Opus parser with libopus 1.6, no effect on decoding",
)];

/// Bilan d'un décodage de vérification, lu ligne à ligne dans le journal d'FFmpeg.
#[derive(Default, Debug)]
struct DecodeLog {
    errors: Vec<String>,
    decode_errors: Option<u64>,
}

impl DecodeLog {
    fn feed(&mut self, line: &str) {
        let l = line.trim();
        if ["[error]", "[fatal]", "[panic]"].iter().any(|t| l.contains(t)) {
            let msg = ffmpeg_lines(&l.replacen("[error] ", "", 1).replacen("[fatal] ", "", 1).replacen("[panic] ", "", 1)).join(" ");
            if !msg.is_empty() && !self.errors.contains(&msg) && self.errors.len() < 20 {
                self.errors.push(msg);
            }
        }
        // « Input stream #0:1 (audio): 153 packets read (…); 153 frames decoded; 0 decode errors (…) »
        if let Some(i) = l.find(" decode errors") {
            let n: String = l[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
            if let Ok(n) = n.parse::<u64>() {
                *self.decode_errors.get_or_insert(0) += n;
            }
        }
    }

    /// `Ok(None)` : aucune erreur ; `Ok(Some(note))` : seulement des messages connus sans effet,
    /// avec zéro erreur de décodage confirmée ; `Err` : échec.
    fn verdict(&self, success: bool) -> Result<Option<String>, String> {
        let tail = |v: &[String]| v[v.len().saturating_sub(3)..].join(" | ");
        if !success {
            return Err(if self.errors.is_empty() {
                tr!("FFmpeg s'est arrêté en erreur", "FFmpeg stopped with an error").into()
            } else {
                tail(&self.errors)
            });
        }
        if self.errors.is_empty() {
            return Ok(None);
        }
        let notes: Vec<&str> = self
            .errors
            .iter()
            .filter_map(|e| BENIGN_DECODE_MESSAGES.iter().find(|(m, _, _)| e.contains(m)).map(|(_, fr, en)| tr!(*fr, *en)))
            .collect();
        if notes.len() == self.errors.len() && self.decode_errors == Some(0) {
            let mut notes = notes;
            notes.dedup();
            return Ok(Some(notes.join(tr!(" ; ", "; "))));
        }
        Err(tail(&self.errors))
    }
}

/// Échec d'FFmpeg : un fichier source illisible est signalé comme tel, avec sa cause probable.
fn failure(code: Option<i32>, stderr: &str) -> EngineError {
    const CAUSES: &[(&str, &str, &str)] = &[
        (
            "moov atom not found",
            "index MP4/MOV absent : fichier tronqué ou enregistrement interrompu",
            "MP4/MOV index missing: truncated file or interrupted recording",
        ),
        ("EBML header parsing failed", "en-tête Matroska illisible", "unreadable Matroska header"),
        ("Invalid data found when processing input", "données illisibles pour ce format", "unreadable data for this format"),
    ];
    let lines = ffmpeg_lines(stderr);
    for (marker, fr, en) in CAUSES {
        if let Some(l) = lines.iter().find(|l| l.contains(marker)) {
            let cause = tr!(*fr, *en);
            return EngineError::InvalidInput(trf!("{cause} (FFmpeg : {l})", "{cause} (FFmpeg: {l})"));
        }
    }
    EngineError::Process { code, detail: lines[lines.len().saturating_sub(3)..].join(" | ") }
}

fn is_lossy_target_audio(to: &Fmt) -> bool {
    to.audio.iter().filter_map(|f| afam(f)).all(|f| !f.lossless)
}

// ---------------------------------------------------------------------------------------------
// Capacités de l'installation FFmpeg
// ---------------------------------------------------------------------------------------------

/// Ce que l'exécutable FFmpeg installé sait réellement faire.
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub encoders: HashSet<String>,
    pub decoders: HashSet<String>,
    pub muxers: HashSet<String>,
    pub demuxers: HashSet<String>,
}

impl Capabilities {
    /// Interroge FFmpeg (`-encoders`, `-decoders`, `-muxers`, `-demuxers`).
    pub fn probe(ffmpeg: &Path) -> Result<Self, String> {
        let list = |what: &str| -> Result<String, String> {
            let spec = ProcessSpec::new(ffmpeg).args(["-hide_banner", what]).timeout(Some(Duration::from_secs(20)));
            let out = chud_process::run_capture(&spec, 4 * 1024 * 1024).map_err(|e| e.to_string())?;
            if !out.success() {
                return Err(trf!("ffmpeg {what} a échoué : {}", "ffmpeg {what} failed: {}", tail_lines(&out.stderr_tail, 2)));
            }
            Ok(out.stdout_text())
        };
        Ok(Self {
            encoders: parse_codec_list(&list("-encoders")?),
            decoders: parse_codec_list(&list("-decoders")?),
            muxers: parse_format_list(&list("-muxers")?),
            demuxers: parse_format_list(&list("-demuxers")?),
        })
    }

    fn has_encoder(&self, e: &str) -> bool {
        self.encoders.contains(e)
    }
}

fn after_separator(text: &str) -> impl Iterator<Item = &str> {
    let mut seen = false;
    text.lines().filter(move |l| {
        if seen {
            return true;
        }
        let t = l.trim();
        if !t.is_empty() && t.chars().all(|c| c == '-') {
            seen = true;
        }
        false
    })
}

/// Lignes « ` V....D libx264  description` ». Les encodeurs expérimentaux sont ignorés.
fn parse_codec_list(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for line in after_separator(text) {
        let (Some(flags), Some(rest)) = (line.get(1..7), line.get(7..)) else { continue };
        if flags.as_bytes().get(3) == Some(&b'X') {
            continue;
        }
        if let Some(name) = rest.split_whitespace().next() {
            set.insert(name.to_owned());
        }
    }
    set
}

/// Lignes « ` DEd name  description` ». Les noms composés (`mov,mp4,m4a…`) sont éclatés.
fn parse_format_list(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for line in after_separator(text) {
        let (Some(flags), Some(rest)) = (line.get(1..4), line.get(4..)) else { continue };
        if flags.contains('d') {
            continue; // périphériques de capture : jamais utilisés
        }
        if let Some(name) = rest.split_whitespace().next() {
            for n in name.split(',') {
                set.insert(n.to_owned());
            }
        }
    }
    set
}

// ---------------------------------------------------------------------------------------------
// Moteur
// ---------------------------------------------------------------------------------------------

pub struct Ffmpeg {
    info: EngineInfo,
    ffmpeg: PathBuf,
    ffprobe: Option<PathBuf>,
    caps: Vec<ConversionCap>,
    avail: Capabilities,
}

/// Recette calculée pour une étape : arguments et rapport honnête.
#[derive(Default, Debug)]
struct Recipe {
    pre_input: Vec<String>,
    output: Vec<String>,
    fidelity: Option<Fidelity>,
    notes: Vec<String>,
    preserves: Vec<&'static str>,
    expect: Expect,
    duration: Option<f64>,
    /// Durée non vérifiable précisément (copie avec découpe, image unique…).
    loose_duration: bool,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Expect {
    video: Option<usize>,
    audio: Option<usize>,
    subtitle: Option<usize>,
}

impl Recipe {
    fn worsen(&mut self, f: Fidelity) {
        self.fidelity = Some(self.fidelity.map_or(f, |cur| cur.max(f)));
    }

    fn keep(&mut self, f: &'static str) {
        if !self.preserves.contains(&f) {
            self.preserves.push(f);
        }
    }

    fn out<S: Into<String>>(&mut self, args: impl IntoIterator<Item = S>) {
        self.output.extend(args.into_iter().map(Into::into));
    }
}

/// Nature des échantillons d'une piste audio source.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Samples {
    Int(u32),
    Float(u32),
    /// Sortie d'un décodeur avec perte (MP3, AAC…).
    Decoded,
    /// Flux DSD (1 bit) : toute conversion en PCM passe par un filtrage.
    Dsd,
    Unknown,
}

fn samples_of(s: &StreamInfo) -> Samples {
    let codec = s.codec.as_str();
    if codec.starts_with("dsd_") {
        return Samples::Dsd;
    }
    let lossless = codec.starts_with("pcm_") || LOSSLESS_AUDIO.contains(&codec);
    if !lossless {
        return Samples::Decoded;
    }
    if codec.starts_with("pcm_f") {
        return Samples::Float(if codec.contains("64") { 64 } else { 32 });
    }
    match s.sample_format.as_deref().unwrap_or("") {
        "flt" | "fltp" => Samples::Float(32),
        "dbl" | "dblp" => Samples::Float(64),
        "u8" | "u8p" => Samples::Int(8),
        "s16" | "s16p" => Samples::Int(16),
        "s32" | "s32p" => Samples::Int(s.bit_depth.filter(|b| (1..=32).contains(b)).unwrap_or(32)),
        "s64" | "s64p" => Samples::Int(64),
        _ => match s.bit_depth {
            Some(b) if b > 0 => Samples::Int(b),
            _ => Samples::Unknown,
        },
    }
}

fn high_bit_pix(pix: &str) -> bool {
    ["10", "12", "14", "16"].iter().any(|d| pix.contains(d))
}

fn alpha_pix(pix: &str) -> bool {
    pix.starts_with("yuva")
        || pix.starts_with("gbrap")
        || pix.starts_with("ya")
        || pix.contains("rgba")
        || pix.contains("bgra")
        || pix.contains("argb")
        || pix.contains("abgr")
}

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn has(info: Option<&SourceInfo>, f: &str) -> bool {
    info.and_then(|i| i.features.as_ref()).is_some_and(|s| s.contains(&Feature::new(f)))
}

fn catalog_has(id: &str, f: &str) -> bool {
    FormatCatalog::builtin().get(id).is_some_and(|d| d.features.contains(&Feature::new(f)))
}

impl Ffmpeg {
    /// Construit le moteur à partir d'un exécutable FFmpeg (et de ffprobe s'il est présent).
    pub fn new(ffmpeg: PathBuf, ffprobe: Option<PathBuf>, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let avail = Capabilities::probe(&ffmpeg)?;
        Ok(Self::with_capabilities(ffmpeg, ffprobe, version, origin, avail))
    }

    /// Variante sans sonde (tests, capacités déjà connues).
    pub fn with_capabilities(
        ffmpeg: PathBuf,
        ffprobe: Option<PathBuf>,
        version: Option<String>,
        origin: EngineOrigin,
        avail: Capabilities,
    ) -> Self {
        let mut me = Self {
            info: EngineInfo {
                id: EngineId::new("ffmpeg"),
                name: "FFmpeg".into(),
                version,
                license: tr!("LGPL-2.1-or-later ou GPL (selon la compilation)", "LGPL-2.1-or-later or GPL (depending on the build)").into(),
                homepage: "https://ffmpeg.org".into(),
                origin,
                path: Some(ffmpeg.clone()),
                description: tr!("Audio, vidéo, animations et sous-titres.", "Audio, video, animations and subtitles.").into(),
            },
            ffmpeg,
            ffprobe,
            caps: Vec::new(),
            avail,
        };
        me.caps = me.build_caps();
        me
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.avail
    }

    fn demuxer(&self, f: &Fmt) -> Option<&'static str> {
        f.demux.iter().copied().find(|d| self.avail.demuxers.contains(*d))
    }

    /// Démultiplexeur imposé pour un fichier précis.
    fn demuxer_for(&self, f: &Fmt, path: &Path) -> Option<&'static str> {
        if f.id == "dsf" {
            let mut magic = [0u8; 4];
            use std::io::Read;
            std::fs::File::open(path).ok()?.read_exact(&mut magic).ok()?;
            let d = match &magic {
                b"DSD " => "dsf",
                b"FRM8" => "iff",
                _ => return None,
            };
            return self.avail.demuxers.contains(d).then_some(d);
        }
        self.demuxer(f)
    }

    fn readable(&self, f: &Fmt) -> bool {
        self.demuxer(f).is_some()
    }

    fn writable(&self, f: &Fmt) -> bool {
        !f.mux.is_empty() && self.avail.muxers.contains(f.mux)
    }

    fn vfam_encoder(&self, fam: &VFam) -> Option<&'static str> {
        fam.encoders.iter().copied().find(|e| self.avail.has_encoder(e))
    }

    fn afam_encoder(&self, fam: &AFam) -> Option<&'static str> {
        fam.encoders.iter().copied().find(|e| self.avail.has_encoder(e))
    }

    fn video_families(&self, to: &Fmt) -> Vec<&'static VFam> {
        to.video.iter().filter_map(|n| vfam(n)).filter(|f| self.vfam_encoder(f).is_some()).collect()
    }

    fn audio_families(&self, to: &Fmt) -> Vec<&'static AFam> {
        to.audio.iter().filter_map(|n| afam(n)).filter(|f| self.afam_encoder(f).is_some()).collect()
    }

    fn sub_target(&self, to: &Fmt) -> Option<&'static str> {
        to.subs.iter().copied().find(|c| TEXT_SUBS.contains(c) && self.avail.has_encoder(sub_encoder(c)))
    }

    fn anim_encoder(&self, to: &str, animated: bool) -> Option<&'static str> {
        let cands: &[&str] = match to {
            "gif" => &["gif"],
            "apng" => &["apng"],
            "webp" if animated => &["libwebp_anim", "libwebp"],
            "webp" => &["libwebp"],
            "png" => &["png"],
            "jpeg" => &["mjpeg"],
            "bmp" => &["bmp"],
            "tiff" => &["tiff"],
            "avif" => &["libaom-av1", "libsvtav1"],
            _ => &[],
        };
        cands.iter().copied().find(|e| self.avail.has_encoder(e))
    }

    fn av1_decoder(&self) -> bool {
        ["libdav1d", "libaom-av1", "av1"].iter().any(|d| self.avail.decoders.contains(*d))
    }

    // -----------------------------------------------------------------------------------------
    // Graphe déclaré
    // -----------------------------------------------------------------------------------------

    fn build_caps(&self) -> Vec<ConversionCap> {
        let mut caps = Vec::new();
        for from in FORMATS.iter().filter(|f| self.readable(f)) {
            for to in FORMATS.iter().filter(|f| self.writable(f)) {
                if let Some((fid, keep, cost)) = self.edge(from, to) {
                    caps.push(ConversionCap::new(from.id, to.id, fid, &features(&keep), cost));
                }
            }
        }
        caps
    }

    fn edge(&self, from: &Fmt, to: &Fmt) -> Option<(Fidelity, Vec<&'static str>, u32)> {
        const MEDIA: &[&str] = &[
            "video",
            "audio",
            "multi_audio",
            "multi_video",
            "subtitles",
            "image_subtitles",
            "chapters",
            "tags",
            "hdr",
            "high_bit_depth",
            "surround",
            "attachments",
            "alpha",
        ];
        use Kind::*;
        match (from.kind, to.kind) {
            (Video, Video) => (!self.video_families(to).is_empty()).then(|| (Fidelity::Depends, MEDIA.to_vec(), 2)),
            (Video | Audio, Audio) => {
                if self.audio_families(to).is_empty() {
                    return None;
                }
                let mut keep = vec!["audio", "multi_audio", "tags", "chapters", "surround", "high_bit_depth"];
                if COVER_TARGETS.contains(&to.id) {
                    keep.push("cover_art");
                }
                Some((self.declared_audio(from, to), keep, 2))
            }
            (Video, Subtitle) => {
                let ok = self.sub_target(to).is_some() || to.id == "sup";
                ok.then(|| (Fidelity::Depends, vec!["subtitles", "timed_text", "image_subtitles"], 2))
            }
            (Subtitle, Subtitle) => {
                if from.id == to.id || from.id == "sup" || to.id == "sup" || self.sub_target(to).is_none() {
                    return None;
                }
                let fid = if from.id == "microdvd" { Fidelity::Depends } else { Fidelity::Lossless };
                Some((fid, vec!["timed_text", "subtitles"], 2))
            }
            (Video | Anim, Anim) => {
                if from.id == to.id {
                    return None;
                }
                let animated = true;
                self.anim_encoder(to.id, animated)?;
                let fid = match (from.kind, to.id) {
                    (_, "gif") => Fidelity::Lossy,
                    (Video, "webp") => Fidelity::Lossy,
                    _ => Fidelity::Lossless,
                };
                let mut keep = vec!["animation", "video"];
                if from.kind == Anim {
                    keep.push("alpha");
                }
                Some((fid, keep, 2))
            }
            (Anim, Video) => (!self.video_families(to).is_empty()).then(|| (Fidelity::Lossy, vec!["animation", "video"], 2)),
            (Video, Image) => {
                if !matches!(to.id, "png" | "jpeg" | "bmp" | "tiff") {
                    return None;
                }
                self.anim_encoder(to.id, false)?;
                let fid = if to.id == "jpeg" { Fidelity::Lossy } else { Fidelity::Lossless };
                Some((fid, vec![], 2))
            }
            (Image, Image) => match (from.id, to.id) {
                ("png" | "jpeg" | "bmp" | "tiff", "avif") => {
                    self.anim_encoder("avif", false)?;
                    Some((Fidelity::Lossy, vec![], 3))
                }
                ("avif", "png" | "tiff" | "jpeg") if self.av1_decoder() => {
                    self.anim_encoder(to.id, false)?;
                    let fid = if to.id == "jpeg" { Fidelity::Lossy } else { Fidelity::Lossless };
                    Some((fid, vec!["high_bit_depth"], 3))
                }
                _ => None,
            },
            (Image, Anim) => {
                // WebP avec perte depuis une image fixe (le moteur natif ne fait que du sans perte).
                if to.id == "webp" && matches!(from.id, "png" | "jpeg" | "bmp" | "tiff") {
                    self.anim_encoder("webp", false)?;
                    return Some((Fidelity::Lossy, vec!["alpha"], 3));
                }
                None
            }
            _ => None,
        }
    }

    fn declared_audio(&self, from: &Fmt, to: &Fmt) -> Fidelity {
        if to.any {
            return Fidelity::Lossless;
        }
        let Some(fam) = self.default_audio_family(to, None) else { return Fidelity::Lossy };
        if !fam.lossless {
            let copy_possible = from.kind == Kind::Video
                || from.any
                || from.id == to.id
                || from.audio.iter().filter_map(|f| afam(f)).any(|ff| to.audio.iter().filter_map(|t| afam(t)).any(|tf| tf.name == ff.name));
            return if copy_possible { Fidelity::Depends } else { Fidelity::Lossy };
        }
        match fam.name {
            "pcm_le" | "pcm_be" | "wavpack" => {
                if from.id == "dsf" {
                    Fidelity::Lossy
                } else {
                    Fidelity::Lossless
                }
            }
            _ if matches!(from.id, "flac" | "alac" | "ape" | "tta") => Fidelity::Lossless,
            _ if from.id == "dsf" => Fidelity::Lossy,
            _ => Fidelity::Depends,
        }
    }

    fn default_audio_family(&self, to: &Fmt, src: Option<&StreamInfo>) -> Option<&'static AFam> {
        let fams = self.audio_families(to);
        if to.any {
            // Matroska : on garde le sans-perte si la source l'est.
            let lossless_src = src.is_some_and(|s| !matches!(samples_of(s), Samples::Decoded));
            if lossless_src && let Some(f) = fams.iter().find(|f| f.name == "flac") {
                return Some(f);
            }
        }
        fams.first().copied()
    }

    // -----------------------------------------------------------------------------------------
    // Recettes
    // -----------------------------------------------------------------------------------------

    fn recipe(&self, from: &Fmt, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        use Kind::*;
        let mut r = match (from.kind, to.kind) {
            (Video | Audio | Anim, Video | Audio) => self.av_recipe(from, to, src, o)?,
            (Video, Subtitle) => self.sub_extract_recipe(to, src, o)?,
            (Subtitle, Subtitle) => self.sub_convert_recipe(from, to)?,
            (Video | Anim, Anim) => self.anim_recipe(from, to, src, o)?,
            (Video, Image) => self.frame_recipe(to, src, o)?,
            (Image, Image | Anim) => self.still_recipe(from, to, src, o)?,
            _ => return Err(trf!("conversion {} → {} non gérée par FFmpeg", "{} → {} conversion not handled by FFmpeg", from.id, to.id)),
        };
        if o.bool_or("strip_metadata", false) {
            r.out(["-map_metadata", "-1", "-map_chapters", "-1"]);
            r.notes.push(tr!("métadonnées et chapitres supprimés à la demande", "metadata and chapters removed as requested").into());
            r.preserves.retain(|f| !matches!(*f, "tags" | "chapters"));
        } else if matches!(to.kind, Video | Audio) {
            r.keep("tags");
            r.keep("chapters");
        }
        // Une même remarque peut venir de plusieurs pistes : chacune n'est affichée qu'une fois.
        let mut seen = std::collections::HashSet::new();
        r.notes.retain(|n| seen.insert(n.clone()));
        Ok(r)
    }

    fn trim(&self, o: &OptionValues, src: Option<&SourceInfo>, r: &mut Recipe) -> Result<(), String> {
        let parse = |key: &str| -> Result<Option<f64>, String> {
            let s = o.str_or(key, "").trim();
            if s.is_empty() {
                return Ok(None);
            }
            parse_time(s).map(Some).ok_or_else(|| {
                trf!(
                    "« {s} » n'est pas une durée valide (ex. 90, 1:30, 01:02:03.5)",
                    "“{s}” is not a valid duration (e.g. 90, 1:30, 01:02:03.5)"
                )
            })
        };
        let start = parse("start")?;
        let end = parse("end")?;
        if let (Some(s), Some(e)) = (start, end)
            && e <= s
        {
            return Err(tr!("la fin doit être postérieure au début", "the end must come after the start").into());
        }
        let total = src.and_then(|i| i.duration);
        if let Some(s) = start {
            if total.is_some_and(|t| s >= t) {
                return Err(tr!("le début demandé dépasse la durée du fichier", "the requested start is past the end of the file").into());
            }
            r.pre_input.extend(["-ss".into(), num(s)]);
        }
        if let Some(e) = end {
            r.out(["-t".to_string(), num(e - start.unwrap_or(0.0))]);
        }
        r.duration = match (total, start, end) {
            (Some(t), s, e) => Some(e.unwrap_or(t).min(t) - s.unwrap_or(0.0)),
            (None, s, Some(e)) => Some(e - s.unwrap_or(0.0)),
            _ => None,
        };
        if start.is_some() || end.is_some() {
            r.notes.push(trf!(
                "extrait conservé : {} → {}",
                "excerpt kept: {} → {}",
                start.map_or(tr!("début", "start").into(), num),
                end.map_or(tr!("fin", "end").into(), num)
            ));
        }
        Ok(())
    }

    fn av_recipe(&self, from: &Fmt, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        let mut r = Recipe::default();
        self.trim(o, src, &mut r)?;
        let trimming = !r.pre_input.is_empty() || o.str_or("end", "").trim() != "";
        let mut ov = 0usize;
        let mut oa = 0usize;
        let mut os = 0usize;

        let Some(info) = src.filter(|i| !i.streams.is_empty()) else {
            // Sans sonde : correspondances optionnelles, tout est ré-encodé (aucune copie à l'aveugle).
            if self.ffprobe.is_none() {
                r.notes.push(
                    tr!(
                        "analyse détaillée indisponible (ffprobe absent) : tous les flux sont ré-encodés",
                        "detailed analysis unavailable (ffprobe missing): all streams are re-encoded"
                    )
                    .into(),
                );
            }
            if to.kind == Kind::Video {
                r.out(["-map", "0:v:0?"]);
                self.video_stream(None, to, o, ov, &mut r, src)?;
            }
            r.out(["-map", "0:a:0?"]);
            self.audio_stream(None, to, o, oa, &mut r)?;
            r.out(["-sn", "-dn"]);
            r.out(to.mux_args.iter().copied());
            return Ok(r);
        };

        let vids: Vec<&StreamInfo> = info.streams_of(StreamKind::Video).collect();
        let auds: Vec<&StreamInfo> = info.streams_of(StreamKind::Audio).collect();
        let subs: Vec<&StreamInfo> = info.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();

        if to.kind == Kind::Video {
            if vids.is_empty() {
                return Err(tr!("ce fichier ne contient aucune piste vidéo", "this file has no video track").into());
            }
            let take = if to.any { vids.len() } else { 1 };
            if vids.len() > take {
                r.notes.push(trf!(
                    "{} pistes vidéo : seule la première est conservée",
                    "{} video tracks: only the first one is kept",
                    vids.len()
                ));
            }
            for v in vids.iter().take(take) {
                r.out(["-map".to_string(), format!("0:{}", v.index)]);
                self.video_stream(Some(v), to, o, ov, &mut r, src)?;
                ov += 1;
            }
            if take > 1 {
                r.keep("multi_video");
            }
            r.expect.video = Some(take);
        } else {
            r.expect.video = Some(0);
        }

        if to.kind == Kind::Audio && auds.is_empty() {
            return Err(tr!("ce fichier ne contient aucune piste audio", "this file has no audio track").into());
        }
        let multi = to.any || (to.kind == Kind::Video && catalog_has(to.id, "multi_audio"));
        let take = if multi { auds.len() } else { auds.len().min(1) };
        if auds.len() > take {
            r.notes.push(trf!(
                "{} pistes audio : seule la piste principale est conservée",
                "{} audio tracks: only the main track is kept",
                auds.len()
            ));
        }
        let mut lead = auds.clone();
        // Piste principale : celle marquée « par défaut » n'est pas exposée ; on suit l'ordre du fichier.
        lead.truncate(take);
        for a in &lead {
            r.out(["-map".to_string(), format!("0:{}", a.index)]);
            self.audio_stream(Some(a), to, o, oa, &mut r)?;
            oa += 1;
        }
        if take > 1 {
            r.keep("multi_audio");
        }
        r.expect.audio = Some(take);

        // Sous-titres (cibles vidéo uniquement).
        let mut kept_subs = 0;
        if to.kind == Kind::Video && o.str_or("subtitles", "keep") == "keep" {
            for s in &subs {
                let copyable = to.subs.contains(&s.codec.as_str());
                if copyable {
                    r.out(["-map".to_string(), format!("0:{}", s.index), format!("-c:s:{os}"), "copy".into()]);
                    if TEXT_SUBS.contains(&s.codec.as_str()) {
                        r.keep("subtitles");
                    } else {
                        r.keep("image_subtitles");
                    }
                } else if TEXT_SUBS.contains(&s.codec.as_str()) {
                    match self.sub_target(to) {
                        Some(target) => {
                            r.out(["-map".to_string(), format!("0:{}", s.index), format!("-c:s:{os}"), sub_encoder(target).to_string()]);
                            r.keep("subtitles");
                            if s.codec == "ass" || s.codec == "ssa" {
                                // Texte et temps conservés ; polices, couleurs, positions et effets ne
                                // le sont pas : la fidélité dépend de ce que le fichier utilise.
                                r.worsen(Fidelity::Depends);
                                r.notes.push(trf!("sous-titres ASS convertis en {target} : texte et temps conservés, styles avancés (polices, couleurs, positions, effets) perdus s'il y en a", "ASS subtitles converted to {target}: text and timing kept, advanced styles (fonts, colors, positions, effects) lost if any"
                                ));
                            }
                        }
                        None => {
                            r.notes.push(trf!(
                                "sous-titres {} non pris en charge par {} : ignorés",
                                "{} subtitles not supported by {}: skipped",
                                s.codec,
                                to.id
                            ));
                            continue;
                        }
                    }
                } else {
                    r.notes.push(trf!(
                        "sous-titres image ({}) impossibles à stocker dans {} : ignorés (une reconnaissance de texte serait nécessaire)",
                        "image subtitles ({}) cannot be stored in {}: skipped (text recognition would be needed)",
                        s.codec,
                        to.id
                    ));
                    continue;
                }
                os += 1;
                kept_subs += 1;
            }
        } else if to.kind == Kind::Video && !subs.is_empty() {
            r.notes.push(tr!("sous-titres supprimés à la demande", "subtitles removed as requested").into());
        }
        r.expect.subtitle = Some(kept_subs);

        // Pochette (formats audio qui la gèrent).
        if to.kind == Kind::Audio
            && COVER_TARGETS.contains(&to.id)
            && let Some(pic) = info.streams.iter().find(|s| s.attached_pic && matches!(s.codec.as_str(), "mjpeg" | "png"))
        {
            r.out([
                "-map".to_string(),
                format!("0:{}", pic.index),
                "-c:v".into(),
                "copy".into(),
                "-disposition:v:0".into(),
                "attached_pic".into(),
            ]);
            r.keep("cover_art");
        }
        // Pièces jointes Matroska (polices des sous-titres ASS…).
        if to.id == "mkv" && from.any && info.streams.iter().any(|s| s.kind == StreamKind::Attachment) {
            r.out(["-map", "0:t?", "-c:t", "copy"]);
            r.keep("attachments");
        }

        if trimming && r.output.iter().any(|a| a == "copy") {
            r.notes.push(
                tr!(
                    "découpe en copie directe : alignée sur l'image clé la plus proche, la durée peut légèrement différer",
                    "cutting with stream copy: aligned on the nearest keyframe, the duration may differ slightly"
                )
                .into(),
            );
            r.loose_duration = true;
        }
        r.out(["-max_muxing_queue_size", "4096"]);
        r.out(to.mux_args.iter().copied());
        if r.fidelity.is_none() {
            r.fidelity = Some(Fidelity::Lossless);
        }
        Ok(r)
    }

    fn video_stream(
        &self,
        s: Option<&StreamInfo>,
        to: &Fmt,
        o: &OptionValues,
        ov: usize,
        r: &mut Recipe,
        src: Option<&SourceInfo>,
    ) -> Result<(), String> {
        let wanted = o.str_or("video_codec", "auto");
        let max_h = o.int_or("max_height", 0).max(0) as u32;
        let fps = o.float_or("fps", 0.0).max(0.0);
        let pix = s.and_then(|s| s.sample_format.clone()).unwrap_or_default();
        let src_hdr = has(src, "hdr");
        let src_high = high_bit_pix(&pix);

        if let Some(s) = s {
            let needs_scale = max_h > 0 && s.height.is_none_or(|h| h > max_h);
            let needs_fps = fps > 0.0 && s.frame_rate.is_none_or(|f| (f - fps).abs() > 0.01);
            let codec_ok = to.vcopy.contains(&s.codec.as_str())
                || to.video.iter().filter_map(|n| vfam(n)).any(|f| f.codec == s.codec)
                || (to.any && !matches!(s.codec.as_str(), "gif" | "apng" | "webp"));
            let codec_matches_choice = wanted == "auto" || wanted == "copy" || vfam(wanted).is_some_and(|f| f.codec == s.codec);
            // « Toujours réencoder » : pour vraiment alléger un fichier déjà dans le bon codec.
            let reencode = o.bool_or("reencode", false);
            if reencode && wanted == "copy" {
                return Err(tr!(
                    "« Copie obligatoire » et « Toujours réencoder » sont incompatibles",
                    "“Copy only” and “Always re-encode” cannot be combined"
                )
                .into());
            }
            if codec_ok && codec_matches_choice && !needs_scale && !needs_fps && !reencode {
                r.out([format!("-c:v:{ov}"), "copy".into()]);
                if s.codec == "hevc" && matches!(to.id, "mp4" | "mov") {
                    r.out([format!("-tag:v:{ov}"), "hvc1".into()]);
                }
                r.worsen(Fidelity::Lossless);
                r.keep("video");
                if src_hdr {
                    r.keep("hdr");
                }
                if src_high {
                    r.keep("high_bit_depth");
                }
                if alpha_pix(&pix) {
                    r.keep("alpha");
                }
                r.notes.push(trf!("vidéo {} copiée sans ré-encodage", "{} video copied without re-encoding", s.codec));
                return Ok(());
            }
            if wanted == "copy" {
                return Err(trf!(
                    "la vidéo {} ne peut pas être copiée telle quelle dans {}",
                    "{} video cannot be copied as is into {}",
                    s.codec,
                    to.id
                ));
            }
        } else if wanted == "copy" {
            return Err(tr!("copie vidéo impossible sans analyse du fichier", "video copy impossible without analyzing the file").into());
        }

        // Ré-encodage.
        let fams = self.video_families(to);
        let fam =
            if wanted == "auto" || wanted == "copy" { fams.first().copied() } else { fams.iter().copied().find(|f| f.name == wanted) }
                .ok_or_else(|| {
                    trf!("aucun encodeur vidéo disponible pour {} ({wanted})", "no video encoder available for {} ({wanted})", to.id)
                })?;
        let enc = self.vfam_encoder(fam).expect("famille filtrée");
        let quality = o.str_or("video_quality", "balanced");
        let speed = o.str_or("speed", "medium");
        let pick = |h: i64, b: i64, s: i64| match quality {
            "high" => h,
            "small" => s,
            _ => b,
        };
        let spd = |f: &'static str, m: &'static str, s: &'static str| match speed {
            "fast" => f,
            "slow" => s,
            _ => m,
        };
        let ten_bit = src_high && matches!(enc, "libx265" | "libsvtav1" | "libaom-av1" | "libvpx-vp9");
        let mut pix_out = if ten_bit { "yuv420p10le" } else { "yuv420p" };
        let mut a: Vec<String> = vec![format!("-c:v:{ov}"), enc.into()];
        match enc {
            "libx264" => {
                a.extend(["-preset".into(), spd("veryfast", "medium", "slow").into(), "-crf".into(), pick(18, 23, 28).to_string()])
            }
            "libx265" => {
                a.extend(["-preset".into(), spd("veryfast", "medium", "slow").into(), "-crf".into(), pick(20, 26, 30).to_string()]);
                a.extend(["-x265-params".into(), "log-level=error".into()]);
                if matches!(to.id, "mp4" | "mov") {
                    a.extend([format!("-tag:v:{ov}"), "hvc1".into()]);
                }
            }
            "libsvtav1" => a.extend(["-preset".into(), spd("10", "7", "4").into(), "-crf".into(), pick(26, 32, 40).to_string()]),
            "libaom-av1" => a.extend([
                "-crf".into(),
                pick(26, 32, 40).to_string(),
                "-b:v".into(),
                "0".into(),
                "-cpu-used".into(),
                spd("8", "5", "3").into(),
                "-row-mt".into(),
                "1".into(),
            ]),
            "libvpx-vp9" => a.extend([
                "-crf".into(),
                pick(24, 32, 40).to_string(),
                "-b:v".into(),
                "0".into(),
                "-deadline".into(),
                "good".into(),
                "-cpu-used".into(),
                spd("4", "2", "1").into(),
                "-row-mt".into(),
                "1".into(),
            ]),
            "libvpx" => a.extend(["-crf".into(), pick(6, 10, 20).to_string(), "-b:v".into(), "20M".into()]),
            "prores_ks" => {
                a.extend(["-profile:v".into(), pick(3, 2, 0).to_string()]);
                pix_out = "yuv422p10le";
            }
            "libtheora" => a.extend(["-q:v".into(), pick(9, 7, 5).to_string()]),
            _ => a.extend(["-q:v".into(), pick(2, 4, 7).to_string()]),
        }
        a.extend([format!("-pix_fmt:v:{ov}"), pix_out.into()]);

        let mut chain: Vec<String> = Vec::new();
        if fps > 0.0 {
            chain.push(format!("fps={}", num(fps)));
            r.notes.push(trf!("cadence convertie à {} images/s", "frame rate converted to {} fps", num(fps)));
        }
        let (w, h) = (s.and_then(|s| s.width), s.and_then(|s| s.height));
        let even_needed = pix_out.starts_with("yuv420");
        if max_h > 0 && h.is_none_or(|h| h > max_h) {
            let target = (max_h & !1).max(2);
            chain.push(match h {
                Some(_) => format!("scale=-2:{target}:flags=lanczos"),
                None => format!("scale=w=-2:h='min(ih,{target})':flags=lanczos"),
            });
            if let Some(h) = h {
                r.notes.push(trf!("hauteur réduite de {h} à {target} pixels", "height reduced from {h} to {target} pixels"));
            }
        } else if even_needed && (w.is_none() || h.is_none() || w.is_some_and(|w| w % 2 == 1) || h.is_some_and(|h| h % 2 == 1)) {
            chain.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into());
            if w.is_some_and(|w| w % 2 == 1) || h.is_some_and(|h| h % 2 == 1) {
                r.notes.push(
                    tr!(
                        "dimensions arrondies à un nombre pair (exigence du codec)",
                        "dimensions rounded to an even number (codec requirement)"
                    )
                    .into(),
                );
            }
        }
        if !chain.is_empty() {
            a.extend([format!("-filter:v:{ov}"), chain.join(",")]);
        }
        r.out(a);
        r.worsen(Fidelity::Lossy);
        r.keep("video");
        if ten_bit || pix_out.contains("10") {
            r.keep("high_bit_depth");
        } else if src_high {
            r.notes.push(tr!("profondeur de couleur réduite à 8 bits", "color depth reduced to 8 bits").into());
        }
        if src_hdr {
            r.notes.push(
                tr!(
                    "métadonnées HDR non garanties après ré-encodage : l'image peut paraître délavée",
                    "HDR metadata not guaranteed after re-encoding: the picture may look washed out"
                )
                .into(),
            );
        }
        r.notes.push(trf!(
            "vidéo ré-encodée en {} ({enc}){}",
            "video re-encoded to {} ({enc}){}",
            fam.label,
            s.map(|s| format!(", source {}", s.codec)).unwrap_or_default()
        ));
        Ok(())
    }

    fn audio_stream(&self, s: Option<&StreamInfo>, to: &Fmt, o: &OptionValues, oa: usize, r: &mut Recipe) -> Result<(), String> {
        let wanted = o.str_or("audio_codec", "auto");
        let bitrate = o.int_or("audio_bitrate", 0).max(0);
        let rate = o.int_or("sample_rate", 0).max(0) as u32;
        let channels = o.int_or("channels", 0).max(0) as u32;
        let normalize = o.bool_or("normalize", false);
        let depth = o.str_or("bit_depth", "auto");

        if let Some(s) = s {
            let changes_rate = rate > 0 && s.sample_rate != Some(rate);
            let changes_ch = channels > 0 && s.channels != Some(channels);
            let codec_ok = to.acopy.contains(&s.codec.as_str())
                || to.audio.iter().filter_map(|n| afam(n)).any(|f| family_accepts_audio(f, &s.codec))
                || (to.any && !s.codec.is_empty());
            let choice_ok = wanted == "auto" || wanted == "copy" || afam(wanted).is_some_and(|f| family_accepts_audio(f, &s.codec));
            let pcm_forced = depth != "auto" && s.codec.starts_with("pcm_");
            if codec_ok && choice_ok && !changes_rate && !changes_ch && !normalize && bitrate == 0 && !pcm_forced {
                r.out([format!("-c:a:{oa}"), "copy".into()]);
                r.worsen(Fidelity::Lossless);
                r.keep("audio");
                r.keep("surround");
                r.keep("high_bit_depth");
                r.notes.push(trf!("audio {} copié sans ré-encodage", "{} audio copied without re-encoding", s.codec));
                return Ok(());
            }
            if wanted == "copy" {
                return Err(trf!(
                    "l'audio {} ne peut pas être copié tel quel dans {}",
                    "{} audio cannot be copied as is into {}",
                    s.codec,
                    to.id
                ));
            }
        } else if wanted == "copy" {
            return Err(tr!("copie audio impossible sans analyse du fichier", "audio copy impossible without analyzing the file").into());
        }

        let fam = if wanted == "auto" || wanted == "copy" {
            self.default_audio_family(to, s)
        } else {
            self.audio_families(to).into_iter().find(|f| f.name == wanted)
        }
        .ok_or_else(|| trf!("aucun encodeur audio disponible pour {} ({wanted})", "no audio encoder available for {} ({wanted})", to.id))?;
        let nature = s.map(samples_of).unwrap_or(Samples::Unknown);
        let mut a: Vec<String> = Vec::new();
        let mut fid = if fam.lossless { Fidelity::Lossless } else { Fidelity::Lossy };
        let mut keep_depth = fam.lossless;

        if fam.name.starts_with("pcm_") {
            let le = fam.name == "pcm_le";
            let (bits, float) = match depth {
                "16" => (16, false),
                "24" => (24, false),
                "32" => (32, false),
                "32f" => (32, true),
                _ => match nature {
                    Samples::Int(b) if b <= 8 => (8, false),
                    Samples::Int(b) if b <= 16 => (16, false),
                    Samples::Int(b) if b <= 24 => (24, false),
                    Samples::Int(_) => (32, false),
                    Samples::Float(64) => (64, true),
                    _ => (32, true),
                },
            };
            let codec = match (bits, float, le) {
                (8, _, true) => "pcm_u8".to_string(),
                (8, _, false) => "pcm_s8".to_string(),
                (b, true, le) => format!("pcm_f{b}{}", if le { "le" } else { "be" }),
                (b, false, le) => format!("pcm_s{b}{}", if le { "le" } else { "be" }),
            };
            if !self.avail.has_encoder(&codec) {
                return Err(trf!("encodeur {codec} indisponible", "encoder {codec} unavailable"));
            }
            match (nature, float) {
                (Samples::Int(b), false) if b > bits => {
                    fid = Fidelity::Lossy;
                    r.notes.push(trf!("profondeur réduite de {b} à {bits} bits", "bit depth reduced from {b} to {bits} bits"));
                }
                (Samples::Float(_), false) | (Samples::Decoded, false) => {
                    fid = Fidelity::Lossy;
                    r.notes.push(trf!(
                        "échantillons flottants arrondis en entiers {bits} bits",
                        "floating-point samples rounded to {bits}-bit integers"
                    ));
                }
                (Samples::Float(64), true) if bits == 32 => {
                    fid = Fidelity::Lossy;
                    r.notes.push(tr!("échantillons 64 bits réduits à 32 bits flottants", "64-bit samples reduced to 32-bit float").into());
                }
                (Samples::Int(b), true) if b > 24 => {
                    fid = Fidelity::Lossy;
                    r.notes.push(trf!(
                        "entiers {b} bits convertis en flottant 32 bits (précision 24 bits)",
                        "{b}-bit integers converted to 32-bit float (24-bit precision)"
                    ));
                }
                (Samples::Decoded, true) => {
                    r.notes.push(
                        tr!(
                            "signal décodé conservé en flottant 32 bits, sans arrondi",
                            "decoded signal kept as 32-bit float, without rounding"
                        )
                        .into(),
                    );
                }
                (Samples::Unknown, _) => fid = Fidelity::Depends,
                _ => {}
            }
            a.extend([format!("-c:a:{oa}"), codec]);
        } else {
            let enc = self.afam_encoder(fam).expect("famille filtrée");
            a.extend([format!("-c:a:{oa}"), enc.to_string()]);
            if fam.lossless {
                let int24 = matches!(fam.name, "flac" | "alac" | "tta");
                let planar = fam.name == "alac";
                let sfmt = |b16: bool| -> &'static str {
                    match (b16, planar) {
                        (true, true) => "s16p",
                        (true, false) => "s16",
                        (false, true) => "s32p",
                        (false, false) => "s32",
                    }
                };
                if int24 {
                    match nature {
                        Samples::Int(b) if b <= 16 => a.extend([format!("-sample_fmt:a:{oa}"), sfmt(true).into()]),
                        Samples::Int(b) if b <= 24 => a.extend([format!("-sample_fmt:a:{oa}"), sfmt(false).into()]),
                        Samples::Int(b) => {
                            a.extend([format!("-sample_fmt:a:{oa}"), sfmt(false).into()]);
                            fid = Fidelity::Lossy;
                            r.notes.push(trf!(
                                "entiers {b} bits réduits à 24 bits ({} est limité à 24 bits)",
                                "{b}-bit integers reduced to 24 bits ({} is limited to 24 bits)",
                                fam.label
                            ));
                        }
                        Samples::Float(_) => {
                            a.extend([format!("-sample_fmt:a:{oa}"), sfmt(false).into()]);
                            fid = Fidelity::Lossy;
                            r.notes.push(trf!(
                                "échantillons flottants convertis en entiers 24 bits ({} ne stocke pas de flottants)",
                                "floating-point samples converted to 24-bit integers ({} does not store floats)",
                                fam.label
                            ));
                        }
                        Samples::Decoded => {
                            a.extend([format!("-sample_fmt:a:{oa}"), sfmt(false).into()]);
                            fid = Fidelity::Depends;
                            r.notes.push(
                                tr!(
                                    "signal décodé stocké en entiers 24 bits ; d'éventuelles crêtes au-delà de 0 dBFS sont écrêtées",
                                    "decoded signal stored as 24-bit integers; any peaks above 0 dBFS are clipped"
                                )
                                .into(),
                            );
                        }
                        Samples::Dsd | Samples::Unknown => fid = Fidelity::Depends,
                    }
                } else if nature == Samples::Float(64) {
                    fid = Fidelity::Lossy;
                    r.notes.push(tr!("échantillons 64 bits réduits à 32 bits flottants", "64-bit samples reduced to 32-bit float").into());
                }
            } else {
                keep_depth = false;
                if bitrate > 0 {
                    a.extend([format!("-b:a:{oa}"), format!("{bitrate}k")]);
                } else {
                    match enc {
                        "libmp3lame" => a.extend([format!("-q:a:{oa}"), "2".into()]),
                        "libvorbis" => a.extend([format!("-q:a:{oa}"), "6".into()]),
                        "libopus" => a.extend([format!("-b:a:{oa}"), "160k".into()]),
                        "ac3" | "eac3" => a.extend([format!("-b:a:{oa}"), "448k".into()]),
                        "libopencore_amrnb" => a.extend([format!("-b:a:{oa}"), "12.2k".into()]),
                        _ => a.extend([format!("-b:a:{oa}"), "192k".into()]),
                    }
                }
                r.notes.push(trf!(
                    "audio ré-encodé en {}{}",
                    "audio re-encoded to {}{}",
                    fam.label,
                    s.map(|s| format!(" (source {})", s.codec)).unwrap_or_default()
                ));
            }
        }
        if nature == Samples::Dsd {
            fid = Fidelity::Lossy;
            r.notes.push(
                tr!("conversion DSD → PCM : filtrage passe-bas obligatoire", "DSD → PCM conversion: low-pass filtering required").into(),
            );
        }

        // Canaux.
        let src_ch = s.and_then(|s| s.channels);
        let mut out_ch = if channels > 0 { Some(channels) } else { None };
        if let Some(c) = out_ch.or(src_ch)
            && c > fam.max_channels
        {
            out_ch = Some(fam.max_channels);
        }
        if let Some(c) = out_ch
            && src_ch != Some(c)
        {
            a.extend([format!("-ac:a:{oa}"), c.to_string()]);
            if let Some(sc) = src_ch {
                fid = Fidelity::Lossy;
                r.notes.push(trf!("mixage de {sc} à {c} canaux", "downmix from {sc} to {c} channels"));
            }
        }
        let surround_kept = out_ch.or(src_ch).is_some_and(|c| c > 2) || out_ch.is_none();
        // Fréquence.
        let mut out_rate = if rate > 0 { Some(rate) } else { None };
        if fam.name == "amr" {
            out_rate = Some(8000);
        }
        if let Some(hz) = out_rate {
            if s.and_then(|s| s.sample_rate) != Some(hz) {
                a.extend([format!("-ar:a:{oa}"), hz.to_string()]);
                if let Some(sr) = s.and_then(|s| s.sample_rate) {
                    fid = Fidelity::Lossy;
                    r.notes.push(trf!("rééchantillonnage de {sr} à {hz} Hz", "resampling from {sr} to {hz} Hz"));
                }
            }
        } else if fam.name == "opus" && s.and_then(|s| s.sample_rate).is_some_and(|sr| sr != 48000) {
            r.notes
                .push(tr!("Opus fonctionne à 48 kHz : l'audio est rééchantillonné", "Opus works at 48 kHz: the audio is resampled").into());
        }
        if normalize {
            a.extend([format!("-filter:a:{oa}"), "loudnorm=I=-16:TP=-1.5:LRA=11".into()]);
            fid = Fidelity::Lossy;
            r.notes.push(tr!("volume normalisé (EBU R128, −16 LUFS)", "loudness normalized (EBU R128, −16 LUFS)").into());
        }
        r.out(a);
        r.worsen(fid);
        r.keep("audio");
        if surround_kept {
            r.keep("surround");
        }
        if keep_depth && fid != Fidelity::Lossy {
            r.keep("high_bit_depth");
        }
        Ok(())
    }

    fn sub_extract_recipe(&self, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        let mut r = Recipe::default();
        let wanted = o.int_or("subtitle_index", 0).max(0) as usize;
        let Some(info) = src.filter(|i| !i.streams.is_empty()) else {
            return Err(tr!(
                "analyse du fichier nécessaire pour choisir la piste de sous-titres",
                "the file must be analyzed to choose the subtitle track"
            )
            .into());
        };
        let subs: Vec<&StreamInfo> = info.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();
        let usable: Vec<&&StreamInfo> = if to.id == "sup" {
            subs.iter().filter(|s| s.codec == "hdmv_pgs_subtitle").collect()
        } else {
            subs.iter().filter(|s| TEXT_SUBS.contains(&s.codec.as_str())).collect()
        };
        if usable.is_empty() {
            return Err(if subs.is_empty() {
                tr!("ce fichier ne contient aucun sous-titre", "this file has no subtitles").into()
            } else if to.id == "sup" {
                tr!("aucun sous-titre PGS à extraire", "no PGS subtitles to extract").into()
            } else {
                tr!(
                    "les sous-titres de ce fichier sont des images : une reconnaissance de texte (OCR) serait nécessaire",
                    "this file's subtitles are images: text recognition (OCR) would be needed"
                )
                .into()
            });
        }
        let s = usable.get(wanted).ok_or_else(|| {
            trf!(
                "piste de sous-titres n°{wanted} introuvable ({} disponibles)",
                "subtitle track #{wanted} not found ({} available)",
                usable.len()
            )
        })?;
        r.out(["-map".to_string(), format!("0:{}", s.index)]);
        if to.subs.contains(&s.codec.as_str()) {
            r.out(["-c:s", "copy"]);
            r.notes.push(trf!("sous-titres {} extraits sans conversion", "{} subtitles extracted without conversion", s.codec));
        } else {
            let target = self.sub_target(to).ok_or(tr!("aucun encodeur de sous-titres disponible", "no subtitle encoder available"))?;
            r.out(["-c:s", sub_encoder(target)]);
            r.notes.push(trf!("sous-titres {} convertis en {}", "{} subtitles converted to {}", s.codec, to.id));
            if s.codec == "ass" || s.codec == "ssa" {
                r.notes.push(
                    tr!(
                        "les styles ASS (polices, couleurs, positions) ne sont pas tous transposables",
                        "not all ASS styles (fonts, colors, positions) can be carried over"
                    )
                    .into(),
                );
            }
        }
        if let Some(l) = &s.language {
            r.notes.push(trf!("langue : {l}", "language: {l}"));
        }
        r.keep("subtitles");
        r.keep("timed_text");
        r.keep("image_subtitles");
        r.expect = Expect { video: Some(0), audio: Some(0), subtitle: Some(1) };
        r.duration = None;
        r.loose_duration = true;
        r.fidelity = Some(Fidelity::Lossless);
        Ok(r)
    }

    fn sub_convert_recipe(&self, from: &Fmt, to: &Fmt) -> Result<Recipe, String> {
        let mut r = Recipe::default();
        let target = self.sub_target(to).ok_or(tr!("aucun encodeur de sous-titres disponible", "no subtitle encoder available"))?;
        r.out(["-map", "0:s:0", "-c:s", sub_encoder(target)]);
        r.keep("timed_text");
        r.keep("subtitles");
        r.fidelity = Some(Fidelity::Lossless);
        if from.id == "microdvd" {
            r.fidelity = Some(Fidelity::Depends);
            r.notes.push(
                tr!(
                    "MicroDVD compte en images : la cadence indiquée dans le fichier (ou 23,976 i/s) sert au calcul des temps",
                    "MicroDVD counts frames: the frame rate given in the file (or 23.976 fps) is used to compute times"
                )
                .into(),
            );
        }
        if from.id == "ass" && target != "ass" {
            r.fidelity = Some(Fidelity::Depends);
            r.notes.push(
                tr!(
                    "texte et temps conservés ; les styles ASS (polices, couleurs, positions, effets) sont perdus s'il y en a",
                    "text and timing kept; ASS styles (fonts, colors, positions, effects) are lost if any"
                )
                .into(),
            );
        }
        r.expect = Expect { video: Some(0), audio: Some(0), subtitle: Some(1) };
        r.loose_duration = true;
        Ok(r)
    }

    fn anim_recipe(&self, from: &Fmt, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        let mut r = Recipe::default();
        self.trim(o, src, &mut r)?;
        let from_video = from.kind == Kind::Video;
        let v = match src.filter(|i| !i.streams.is_empty()) {
            Some(info) => Some(
                info.streams_of(StreamKind::Video)
                    .next()
                    .ok_or(tr!("ce fichier ne contient aucune piste vidéo", "this file has no video track"))?,
            ),
            None => None,
        };
        let map = v.map_or("0:v:0".to_string(), |v| format!("0:{}", v.index));
        let src_fps = v.and_then(|v| v.frame_rate);
        let default_fps = if from_video && to.id == "gif" { 15.0 } else { 0.0 };
        let fps = o.float_or("fps", default_fps).max(0.0);
        let default_w = if from_video && to.id == "gif" { 480 } else { 0 };
        let max_w = o.int_or("max_width", default_w).max(0) as u32;
        let mut chain: Vec<String> = Vec::new();
        let mut fid = Fidelity::Lossless;
        if fps > 0.0 && src_fps.is_none_or(|f| f > fps + 0.01) {
            chain.push(format!("fps={}", num(fps)));
            r.notes.push(trf!("cadence limitée à {} images/s", "frame rate limited to {} fps", num(fps)));
            fid = Fidelity::Lossy;
        }
        if max_w > 0 && v.and_then(|v| v.width).is_none_or(|w| w > max_w) {
            chain.push(format!("scale=w='min(iw,{max_w})':h=-1:flags=lanczos"));
            r.notes.push(trf!("largeur limitée à {max_w} pixels", "width limited to {max_w} pixels"));
            fid = Fidelity::Lossy;
        }
        let animated = true;
        let enc = self.anim_encoder(to.id, animated).ok_or(tr!("encodeur indisponible", "encoder unavailable"))?;
        let lossless_default = !from_video;
        match to.id {
            "gif" => {
                let base = if chain.is_empty() { "null".to_string() } else { chain.join(",") };
                r.out([
                    "-filter_complex".to_string(),
                    format!("[{map}]{base},split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a[out]"),
                    "-map".into(),
                    "[out]".into(),
                    "-c:v".into(),
                    "gif".into(),
                    "-loop".into(),
                    if o.bool_or("loop", true) { "0" } else { "-1" }.into(),
                ]);
                fid = Fidelity::Lossy;
                r.notes.push(
                    tr!(
                        "palette réduite à 256 couleurs (optimisée pour l'animation)",
                        "palette reduced to 256 colors (optimized for the animation)"
                    )
                    .into(),
                );
            }
            _ => {
                r.out(["-map".to_string(), map]);
                if !chain.is_empty() {
                    r.out(["-filter:v".to_string(), chain.join(",")]);
                }
                r.out(["-c:v", enc]);
                if to.id == "apng" {
                    r.out(["-plays", if o.bool_or("loop", true) { "0" } else { "1" }]);
                    if from_video {
                        r.out(["-pix_fmt", "rgb24"]);
                    }
                } else {
                    // WebP
                    let lossless = o.bool_or("lossless", lossless_default);
                    r.out(["-loop", if o.bool_or("loop", true) { "0" } else { "1" }]);
                    if lossless {
                        r.out(["-lossless", "1"]);
                    } else {
                        let q = o.int_or("quality", 80).clamp(0, 100);
                        r.out(["-lossless".to_string(), "0".into(), "-quality".into(), q.to_string()]);
                        fid = Fidelity::Lossy;
                        r.notes.push(trf!("WebP avec perte, qualité {q}", "lossy WebP, quality {q}"));
                    }
                }
            }
        }
        if from_video {
            r.notes.push(tr!("le son n'est pas conservé dans une animation", "sound is not kept in an animation").into());
        }
        r.out(["-an", "-sn", "-dn"]);
        r.keep("animation");
        r.keep("video");
        if !from_video && to.id != "gif" {
            r.keep("alpha");
        }
        r.fidelity = Some(fid);
        r.expect = Expect { video: Some(1), audio: Some(0), subtitle: Some(0) };
        r.loose_duration = true;
        Ok(r)
    }

    fn frame_recipe(&self, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        let mut r = Recipe::default();
        let v = match src.filter(|i| !i.streams.is_empty()) {
            Some(info) => Some(
                info.streams_of(StreamKind::Video)
                    .next()
                    .ok_or(tr!("ce fichier ne contient aucune piste vidéo", "this file has no video track"))?,
            ),
            None => None,
        };
        let raw = o.str_or("time", "").trim();
        let total = src.and_then(|i| i.duration);
        let t = if raw.is_empty() {
            total.map(|d| (d * 0.1).min(10.0)).unwrap_or(0.0)
        } else {
            parse_time(raw).ok_or_else(|| {
                trf!(
                    "« {raw} » n'est pas un instant valide (ex. 12, 0:12, 00:00:12.5)",
                    "“{raw}” is not a valid time (e.g. 12, 0:12, 00:00:12.5)"
                )
            })?
        };
        if total.is_some_and(|d| t >= d) {
            return Err(tr!("l'instant demandé dépasse la durée de la vidéo", "the requested time is past the end of the video").into());
        }
        r.pre_input.extend(["-ss".into(), num(t)]);
        let enc = self.anim_encoder(to.id, false).ok_or(tr!("encodeur indisponible", "encoder unavailable"))?;
        r.out(["-map".to_string(), v.map_or("0:v:0".into(), |v| format!("0:{}", v.index))]);
        r.out(["-frames:v", "1", "-update", "1", "-c:v", enc]);
        let mut fid = Fidelity::Lossless;
        if to.id == "jpeg" {
            let q = o.int_or("quality", 90).clamp(1, 100);
            // Échelle mjpeg : 2 (meilleur) à 31.
            let qv = 2 + ((100 - q) * 29 / 99);
            r.out(["-q:v".to_string(), qv.to_string()]);
            fid = Fidelity::Lossy;
        } else if matches!(to.id, "png" | "bmp" | "tiff") {
            let pix = if v.and_then(|v| v.sample_format.as_deref()).is_some_and(alpha_pix) { "rgba" } else { "rgb24" };
            r.out(["-pix_fmt", pix]);
        }
        r.out(["-an", "-sn", "-dn"]);
        r.notes.push(trf!("une seule image extraite, à {} s", "a single frame extracted, at {} s", num(t)));
        r.fidelity = Some(fid);
        r.expect = Expect { video: Some(1), audio: Some(0), subtitle: Some(0) };
        r.loose_duration = true;
        Ok(r)
    }

    fn still_recipe(&self, from: &Fmt, to: &Fmt, src: Option<&SourceInfo>, o: &OptionValues) -> Result<Recipe, String> {
        let _ = src;
        let mut r = Recipe::default();
        let enc = self.anim_encoder(to.id, false).ok_or(tr!("encodeur indisponible", "encoder unavailable"))?;
        r.out(["-map", "0:v:0", "-frames:v", "1", "-c:v", enc]);
        let q = o.int_or("quality", if to.id == "avif" { 60 } else { 80 }).clamp(0, 100);
        let mut fid = Fidelity::Lossy;
        match to.id {
            "avif" => {
                // CRF 0 (meilleur) à 63.
                let crf = 63 - (q * 63 / 100);
                if enc == "libaom-av1" {
                    r.out([
                        "-still-picture".to_string(),
                        "1".into(),
                        "-crf".into(),
                        crf.to_string(),
                        "-b:v".into(),
                        "0".into(),
                        "-cpu-used".into(),
                        "6".into(),
                    ]);
                } else {
                    r.out(["-crf".to_string(), crf.to_string(), "-preset".into(), "8".into()]);
                }
                r.out(["-pix_fmt", "yuv420p"]);
                r.notes.push(trf!(
                    "AVIF qualité {q} ; la transparence n'est pas conservée par ce moteur",
                    "AVIF quality {q}; transparency is not kept by this engine"
                ));
            }
            "webp" => {
                if o.bool_or("lossless", false) {
                    r.out(["-lossless", "1"]);
                    fid = Fidelity::Lossless;
                } else {
                    r.out(["-lossless".to_string(), "0".into(), "-quality".into(), q.to_string()]);
                    r.notes.push(trf!("WebP avec perte, qualité {q}", "lossy WebP, quality {q}"));
                }
            }
            "jpeg" => {
                let qv = 2 + ((100 - q.max(1)) * 29 / 99);
                r.out(["-q:v".to_string(), qv.to_string()]);
            }
            _ => {
                fid = Fidelity::Lossless;
                if from.id == "avif" {
                    r.notes.push(tr!("image AVIF décodée (conversion YUV → RVB)", "AVIF image decoded (YUV → RGB conversion)").into());
                }
            }
        }
        if to.mux == "image2" {
            r.out(["-update", "1"]);
        }
        r.fidelity = Some(fid);
        r.expect = Expect { video: Some(1), audio: Some(0), subtitle: Some(0) };
        r.loose_duration = true;
        Ok(r)
    }

    // -----------------------------------------------------------------------------------------
    // Sonde (ffprobe)
    // -----------------------------------------------------------------------------------------

    fn ffprobe_json(&self, path: &Path, demux: Option<&str>, timeout: Duration) -> Option<Value> {
        let probe = self.ffprobe.as_ref()?;
        let mut spec = ProcessSpec::new(probe)
            .args(["-v", "error", "-hide_banner", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters"])
            .args(["-protocol_whitelist", "file,pipe"])
            .timeout(Some(timeout));
        if let Some(d) = demux {
            spec = spec.args(["-f", d]);
        }
        let mut target = std::ffi::OsString::from("file:");
        target.push(path.as_os_str());
        spec = spec.arg(target);
        let out = chud_process::run_capture(&spec, 32 * 1024 * 1024).ok()?;
        if !out.success() || out.stdout_truncated {
            return None;
        }
        serde_json::from_slice(&out.stdout).ok()
    }
}

fn parse_rate(s: &str) -> Option<f64> {
    let (a, b) = s.split_once('/')?;
    let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
    (b > 0.0 && a > 0.0).then(|| a / b)
}

fn jstr<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn jnum(v: &Value, k: &str) -> Option<f64> {
    match v.get(k)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Transforme la sortie JSON de ffprobe en [`SourceInfo`].
fn parse_probe(v: &Value, fkind: Kind) -> SourceInfo {
    let mut info = SourceInfo::default();
    let mut feats: Vec<&str> = Vec::new();
    let mut summary = Vec::new();
    let empty = Vec::new();
    let streams = v.get("streams").and_then(Value::as_array).unwrap_or(&empty);
    let mut hdr = false;
    let mut timecode = false;
    for s in streams {
        let ctype = jstr(s, "codec_type").unwrap_or("");
        let kind = match ctype {
            "video" => StreamKind::Video,
            "audio" => StreamKind::Audio,
            "subtitle" => StreamKind::Subtitle,
            "attachment" => StreamKind::Attachment,
            _ => StreamKind::Data,
        };
        let codec = jstr(s, "codec_name").unwrap_or("").to_owned();
        let attached = s.get("disposition").and_then(|d| d.get("attached_pic")).and_then(Value::as_i64) == Some(1);
        let tags = s.get("tags");
        let lang = tags.and_then(|t| jstr(t, "language")).filter(|l| *l != "und").map(str::to_owned);
        if tags.and_then(|t| jstr(t, "timecode")).is_some() || jstr(s, "codec_tag_string") == Some("tmcd") {
            timecode = true;
        }
        if matches!(jstr(s, "color_transfer"), Some("smpte2084" | "arib-std-b67")) {
            hdr = true;
        }
        let sample_format = match kind {
            StreamKind::Video => jstr(s, "pix_fmt"),
            StreamKind::Audio => jstr(s, "sample_fmt"),
            _ => None,
        }
        .map(str::to_owned);
        let st = StreamInfo {
            index: s.get("index").and_then(Value::as_u64).unwrap_or(0) as u32,
            kind,
            codec,
            attached_pic: attached,
            language: lang,
            width: jnum(s, "width").map(|w| w as u32),
            height: jnum(s, "height").map(|h| h as u32),
            channels: jnum(s, "channels").map(|c| c as u32),
            sample_rate: jnum(s, "sample_rate").map(|r| r as u32),
            bit_depth: jnum(s, "bits_per_raw_sample").map(|b| b as u32).filter(|b| *b > 0),
            sample_format,
            frame_rate: jstr(s, "avg_frame_rate").and_then(parse_rate).or_else(|| jstr(s, "r_frame_rate").and_then(parse_rate)),
            bitrate: jnum(s, "bit_rate").map(|b| b as u64),
        };
        info.streams.push(st);
    }
    let fmt = v.get("format");
    info.duration = fmt.and_then(|f| jnum(f, "duration")).filter(|d| d.is_finite() && *d > 0.0);

    let first_dims = info.streams_of(StreamKind::Video).next().map(|v| (v.width, v.height));
    if let Some((w, h)) = first_dims {
        info.width = w;
        info.height = h;
    }
    let vids: Vec<&StreamInfo> = info.streams_of(StreamKind::Video).collect();
    let auds: Vec<&StreamInfo> = info.streams_of(StreamKind::Audio).collect();
    let subs: Vec<&StreamInfo> = info.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();
    if !vids.is_empty() {
        feats.push("video");
    }
    if vids.len() > 1 {
        feats.push("multi_video");
    }
    if !auds.is_empty() {
        feats.push("audio");
    }
    if auds.len() > 1 {
        feats.push("multi_audio");
    }
    if auds.iter().any(|a| a.channels.is_some_and(|c| c > 2)) {
        feats.push("surround");
    }
    if subs.iter().any(|s| TEXT_SUBS.contains(&s.codec.as_str())) {
        feats.push("subtitles");
    }
    if subs.iter().any(|s| !TEXT_SUBS.contains(&s.codec.as_str())) {
        feats.push("image_subtitles");
    }
    if info.streams.iter().any(|s| s.attached_pic) {
        feats.push("cover_art");
    }
    if info.streams.iter().any(|s| s.kind == StreamKind::Attachment) {
        feats.push("attachments");
    }
    if hdr {
        feats.push("hdr");
    }
    if timecode {
        feats.push("timecode");
    }
    let high = vids.iter().any(|v| v.sample_format.as_deref().is_some_and(high_bit_pix))
        || auds.iter().any(|a| matches!(samples_of(a), Samples::Int(b) if b > 16) || matches!(samples_of(a), Samples::Float(_)));
    if high {
        feats.push("high_bit_depth");
    }
    if vids.iter().any(|v| v.sample_format.as_deref().is_some_and(alpha_pix)) {
        feats.push("alpha");
    }
    let chapters = v.get("chapters").and_then(Value::as_array).map_or(0, Vec::len);
    if chapters > 0 {
        feats.push("chapters");
    }
    let tags = fmt.and_then(|f| f.get("tags")).and_then(Value::as_object);
    if tags.is_some_and(|t| t.keys().any(|k| !TECH_TAGS.contains(&k.to_ascii_lowercase().as_str()))) {
        feats.push("tags");
    }

    // Résumé lisible.
    if let Some(name) = fmt.and_then(|f| jstr(f, "format_long_name")) {
        summary.push((tr!("Conteneur", "Container").to_string(), name.to_owned()));
    }
    if let Some(d) = info.duration {
        let s = d.round() as u64;
        summary.push((tr!("Durée", "Duration").into(), format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)));
    }
    for (i, s) in vids.iter().enumerate() {
        let mut t = s.codec.clone();
        if let (Some(w), Some(h)) = (s.width, s.height) {
            t.push_str(&format!(" {w}×{h}"));
        }
        if let Some(f) = s.frame_rate {
            t.push_str(&trf!(" {} i/s", " {} fps", num(f)));
        }
        if let Some(p) = &s.sample_format {
            t.push_str(&format!(" ({p})"));
        }
        summary.push((trf!("Vidéo {}", "Video {}", i + 1), t));
    }
    for (i, s) in auds.iter().enumerate() {
        let mut t = s.codec.clone();
        if let Some(r) = s.sample_rate {
            t.push_str(&format!(" {} kHz", num(r as f64 / 1000.0)));
        }
        if let Some(c) = s.channels {
            t.push_str(&if c > 1 { trf!(", {c} canaux", ", {c} channels") } else { trf!(", {c} canal", ", {c} channel") });
        }
        if let Some(l) = &s.language {
            t.push_str(&format!(" [{l}]"));
        }
        summary.push((format!("Audio {}", i + 1), t));
    }
    for (i, s) in subs.iter().enumerate() {
        let mut t = s.codec.clone();
        if let Some(l) = &s.language {
            t.push_str(&format!(" [{l}]"));
        }
        summary.push((trf!("Sous-titres {}", "Subtitles {}", i + 1), t));
    }
    if chapters > 0 {
        summary.push((tr!("Chapitres", "Chapters").into(), chapters.to_string()));
    }
    if hdr {
        summary.push(("HDR".into(), tr!("oui", "yes").into()));
    }
    info.summary = summary;
    // Pour les fichiers de sous-titres, la sonde ne sait pas établir les styles réellement utilisés :
    // on laisse le catalogue décrire les caractéristiques (non confirmées).
    info.features = if fkind == Kind::Subtitle { None } else { Some(features(&feats)) };
    info
}

fn counts(info: &SourceInfo) -> (usize, usize, usize) {
    (
        info.streams_of(StreamKind::Video).count(),
        info.streams_of(StreamKind::Audio).count(),
        info.streams.iter().filter(|s| s.kind == StreamKind::Subtitle).count(),
    )
}

impl Engine for Ffmpeg {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        let (Some(f), Some(t)) = (fmt_by_id(from.as_str()), fmt_by_id(to.as_str())) else { return Vec::new() };
        let mut o = Vec::new();
        let trim = |o: &mut Vec<OptionSpec>| {
            o.push(
                OptionSpec::text("start", tr!("Début (ex. 1:30)", "Start (e.g. 1:30)"), 16, "")
                    .help(tr!("Laisser vide pour partir du début", "Leave empty to start at the beginning")),
            );
            o.push(
                OptionSpec::text("end", tr!("Fin (ex. 2:45)", "End (e.g. 2:45)"), 16, "")
                    .help(tr!("Laisser vide pour aller jusqu'à la fin", "Leave empty to go to the end")),
            );
        };
        match (f.kind, t.kind) {
            (Kind::Video | Kind::Audio | Kind::Anim, Kind::Video | Kind::Audio) => {
                if t.kind == Kind::Video {
                    let mut choices: Vec<(&str, &str)> = vec![
                        ("auto", tr!("Automatique (copie si possible)", "Automatic (copy when possible)")),
                        ("copy", tr!("Copie obligatoire", "Copy only")),
                    ];
                    for fam in self.video_families(t) {
                        choices.push((fam.name, fam.label));
                    }
                    o.push(OptionSpec::choice("video_codec", tr!("Codec vidéo", "Video codec"), &choices, "auto"));
                    o.push(OptionSpec::choice(
                        "video_quality",
                        tr!("Qualité vidéo", "Video quality"),
                        &[
                            ("high", tr!("Haute", "High")),
                            ("balanced", tr!("Équilibrée", "Balanced")),
                            ("small", tr!("Fichier léger", "Small file")),
                        ],
                        "balanced",
                    ));
                    o.push(
                        OptionSpec::choice(
                            "speed",
                            tr!("Vitesse d'encodage", "Encoding speed"),
                            &[
                                ("fast", tr!("Rapide", "Fast")),
                                ("medium", tr!("Normale", "Normal")),
                                ("slow", tr!("Lente (meilleure compression)", "Slow (better compression)")),
                            ],
                            "medium",
                        )
                        .advanced(),
                    );
                    o.push(
                        OptionSpec::bool("reencode", tr!("Toujours réencoder la vidéo", "Always re-encode the video"), false)
                            .help(tr!("Même si elle pourrait être copiée telle quelle : nécessaire pour réduire la taille d'une vidéo déjà au bon format", "Even if it could be copied as is: needed to shrink a video already in the right format"))
                            .advanced(),
                    );
                    o.push(
                        OptionSpec::int(
                            "max_height",
                            tr!("Hauteur maximale (0 = inchangée)", "Maximum height (0 = unchanged)"),
                            0,
                            8640,
                            0,
                        )
                        .help("720, 1080, 2160…"),
                    );
                    o.push(
                        OptionSpec::float(
                            "fps",
                            tr!("Images par seconde (0 = inchangé)", "Frames per second (0 = unchanged)"),
                            0.0,
                            240.0,
                            0.0,
                        )
                        .advanced(),
                    );
                    o.push(OptionSpec::choice(
                        "subtitles",
                        tr!("Sous-titres", "Subtitles"),
                        &[("keep", tr!("Conserver", "Keep")), ("drop", tr!("Supprimer", "Remove"))],
                        "keep",
                    ));
                }
                let mut choices: Vec<(&str, &str)> = vec![
                    ("auto", tr!("Automatique (copie si possible)", "Automatic (copy when possible)")),
                    ("copy", tr!("Copie obligatoire", "Copy only")),
                ];
                for fam in self.audio_families(t) {
                    choices.push((fam.name, fam.label));
                }
                o.push(OptionSpec::choice("audio_codec", tr!("Codec audio", "Audio codec"), &choices, "auto").advanced());
                if !is_lossy_target_audio(t) && t.audio.iter().any(|a| a.starts_with("pcm")) {
                    o.push(OptionSpec::choice(
                        "bit_depth",
                        tr!("Résolution", "Bit depth"),
                        &[
                            ("auto", tr!("Fidèle à la source", "Same as source")),
                            ("16", "16 bits"),
                            ("24", "24 bits"),
                            ("32", tr!("32 bits entiers", "32-bit integer")),
                            ("32f", tr!("32 bits flottants", "32-bit float")),
                        ],
                        "auto",
                    ));
                }
                if self.audio_families(t).iter().any(|a| !a.lossless) {
                    o.push(OptionSpec::int(
                        "audio_bitrate",
                        tr!("Débit audio en kbit/s (0 = automatique)", "Audio bitrate in kbit/s (0 = automatic)"),
                        0,
                        1536,
                        0,
                    ));
                }
                o.push(
                    OptionSpec::int(
                        "sample_rate",
                        tr!("Fréquence d'échantillonnage (0 = inchangée)", "Sample rate (0 = unchanged)"),
                        0,
                        384_000,
                        0,
                    )
                    .advanced(),
                );
                o.push(
                    OptionSpec::int("channels", tr!("Nombre de canaux (0 = inchangé)", "Number of channels (0 = unchanged)"), 0, 8, 0)
                        .advanced(),
                );
                o.push(
                    OptionSpec::bool("normalize", tr!("Normaliser le volume (EBU R128)", "Normalize loudness (EBU R128)"), false)
                        .advanced(),
                );
                trim(&mut o);
                o.push(OptionSpec::bool("strip_metadata", tr!("Supprimer les métadonnées", "Remove metadata"), false).advanced());
            }
            (Kind::Video, Kind::Subtitle) => {
                o.push(OptionSpec::int(
                    "subtitle_index",
                    tr!("Piste de sous-titres (0 = première)", "Subtitle track (0 = first)"),
                    0,
                    99,
                    0,
                ));
            }
            (Kind::Video | Kind::Anim, Kind::Anim) => {
                let from_video = f.kind == Kind::Video;
                let gif = t.id == "gif";
                o.push(OptionSpec::float(
                    "fps",
                    tr!("Images par seconde (0 = inchangé)", "Frames per second (0 = unchanged)"),
                    0.0,
                    60.0,
                    if from_video && gif { 15.0 } else { 0.0 },
                ));
                o.push(OptionSpec::int(
                    "max_width",
                    tr!("Largeur maximale (0 = inchangée)", "Maximum width (0 = unchanged)"),
                    0,
                    8192,
                    if from_video && gif { 480 } else { 0 },
                ));
                o.push(OptionSpec::bool("loop", tr!("Lecture en boucle", "Loop playback"), true));
                if t.id == "webp" {
                    o.push(OptionSpec::bool("lossless", tr!("Sans perte", "Lossless"), !from_video));
                    o.push(OptionSpec::int("quality", tr!("Qualité (avec perte)", "Quality (lossy)"), 0, 100, 80));
                }
                if from_video {
                    trim(&mut o);
                }
            }
            (Kind::Video, Kind::Image) => {
                o.push(
                    OptionSpec::text("time", tr!("Instant de l'image (ex. 0:12)", "Frame time (e.g. 0:12)"), 16, "")
                        .help(tr!("Vide : à 10 % de la durée", "Empty: at 10% of the duration")),
                );
                if t.id == "jpeg" {
                    o.push(OptionSpec::int("quality", tr!("Qualité", "Quality"), 1, 100, 90));
                }
            }
            (Kind::Image, Kind::Image | Kind::Anim) => {
                if matches!(t.id, "avif" | "webp" | "jpeg") {
                    o.push(OptionSpec::int("quality", tr!("Qualité", "Quality"), 0, 100, if t.id == "avif" { 60 } else { 80 }));
                }
                if t.id == "webp" {
                    o.push(OptionSpec::bool("lossless", tr!("Sans perte", "Lossless"), false));
                }
            }
            _ => {}
        }
        o
    }

    fn can_probe(&self, format: &FormatId) -> bool {
        self.ffprobe.is_some()
            && fmt_by_id(format.as_str()).is_some_and(|f| matches!(f.kind, Kind::Video | Kind::Audio | Kind::Subtitle) && self.readable(f))
    }

    fn probe(&self, path: &Path, format: &FormatId) -> Option<SourceInfo> {
        let f = fmt_by_id(format.as_str())?;
        let demux = self.demuxer_for(f, path)?;
        let v = self.ffprobe_json(path, Some(demux), Duration::from_secs(60))?;
        Some(parse_probe(&v, f.kind))
    }

    fn assess(&self, q: &StepQuery<'_>) -> StepAssessment {
        let (Some(from), Some(to)) = (fmt_by_id(q.from.as_str()), fmt_by_id(q.to.as_str())) else {
            return StepAssessment {
                fidelity: Fidelity::Lossy,
                notes: vec![],
                unavailable: Some("format inconnu de l'adaptateur FFmpeg".into()),
                preserves: None,
            };
        };
        let declared = self.caps.iter().find(|c| &c.from == q.from && &c.to == q.to);
        let Some(declared) = declared else {
            return StepAssessment {
                fidelity: Fidelity::Lossy,
                notes: vec![],
                unavailable: Some(
                    tr!(
                        "conversion non disponible avec cette installation de FFmpeg",
                        "conversion not available with this FFmpeg installation"
                    )
                    .into(),
                ),
                preserves: None,
            };
        };
        match q.source {
            Some(src) => match self.recipe(from, to, Some(src), q.options) {
                Ok(r) => StepAssessment {
                    fidelity: r.fidelity.unwrap_or(declared.fidelity),
                    notes: r.notes,
                    unavailable: None,
                    preserves: Some(features(&r.preserves)),
                },
                Err(e) => StepAssessment { fidelity: Fidelity::Lossy, notes: vec![], unavailable: Some(e), preserves: None },
            },
            None => {
                // Sans sonde : la fidélité déclarée, aggravée par les options qui imposent un ré-encodage.
                let mut fid = declared.fidelity;
                let mut notes = Vec::new();
                let forced = ["max_height", "fps", "sample_rate", "channels", "audio_bitrate"]
                    .iter()
                    .any(|k| q.options.int_or(k, 0) > 0 || q.options.float_or(k, 0.0) > 0.0)
                    || q.options.bool_or("normalize", false)
                    || q.options.bool_or("reencode", false)
                    || !matches!(q.options.str_or("video_codec", "auto"), "auto" | "copy");
                if forced && matches!(to.kind, Kind::Video | Kind::Audio) {
                    fid = Fidelity::Lossy;
                    notes.push(tr!("les options choisies imposent un ré-encodage", "the chosen options require re-encoding").into());
                }
                if let Err(e) = self.recipe(from, to, None, q.options)
                    && matches!(to.kind, Kind::Video | Kind::Audio)
                {
                    return StepAssessment { fidelity: fid, notes, unavailable: Some(e), preserves: None };
                }
                StepAssessment { fidelity: fid, notes, unavailable: None, preserves: None }
            }
        }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let from = fmt_by_id(task.from.as_str()).ok_or_else(|| EngineError::Unsupported(task.from.to_string()))?;
        let to = fmt_by_id(task.to.as_str()).ok_or_else(|| EngineError::Unsupported(task.to.to_string()))?;
        let demux = self.demuxer_for(from, &task.input).ok_or_else(|| {
            EngineError::Unsupported(trf!(
                "lecture {} indisponible avec cette installation de FFmpeg",
                "reading {} is not available with this FFmpeg installation",
                from.id
            ))
        })?;
        if !self.writable(to) {
            return Err(EngineError::Unsupported(trf!(
                "écriture {} indisponible avec cette installation de FFmpeg",
                "writing {} is not available with this FFmpeg installation",
                to.id
            )));
        }
        let recipe = self.recipe(from, to, task.source.as_ref(), &task.options).map_err(EngineError::Unsupported)?;

        let mut spec = ProcessSpec::new(&self.ffmpeg)
            .args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"])
            .args(["-protocol_whitelist", "file,pipe"]);
        spec = spec.args(recipe.pre_input.iter());
        // Encodage des sous-titres texte : détecté par nos soins plutôt que supposé UTF-8.
        if from.kind == Kind::Subtitle
            && let Ok(bytes) = std::fs::read(&task.input)
        {
            let d = text::decode(&bytes);
            if d.encoding != "UTF-8" && !d.had_bom {
                spec = spec.args(["-sub_charenc", d.encoding]);
            }
        }
        let mut input = std::ffi::OsString::from("file:");
        input.push(task.input.as_os_str());
        spec = spec.args(["-f", demux]).arg("-i").arg(input);
        spec = spec.args(recipe.output.iter());
        let mut output = std::ffi::OsString::from("file:");
        output.push(task.output.as_os_str());
        spec = spec.args(["-f", to.mux]).arg(output);

        let total = recipe.duration.filter(|d| *d > 0.0);
        let mut speed: Option<String> = None;
        let mut on_stdout = |line: &str| {
            if let Some(v) = line.strip_prefix("speed=") {
                let v = v.trim();
                if v != "N/A" {
                    speed = Some(v.to_owned());
                }
            } else if let Some(v) = line.strip_prefix("out_time_us=") {
                if let (Ok(us), Some(t)) = (v.trim().parse::<f64>(), total) {
                    let frac = (us / 1e6 / t).clamp(0.0, 0.99);
                    ctx.report(Progress { fraction: Some(frac), speed: speed.clone(), message: None });
                }
            } else if line.trim() == "progress=end" {
                ctx.report(Progress { fraction: Some(1.0), speed: speed.clone(), message: None });
            }
        };
        let out = run_tool(spec, ctx, &mut on_stdout, &mut |_| {})?;
        if !out.success() {
            return Err(failure(out.code, &out.stderr_tail));
        }
        if std::fs::metadata(&task.output).map(|m| m.len()).unwrap_or(0) == 0 {
            return Err(EngineError::Process {
                code: out.code,
                detail: tr!("FFmpeg n'a produit aucun contenu", "FFmpeg produced no content").into(),
            });
        }
        Ok(StepOutput { notes: recipe.notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        let (Some(from), Some(to)) = (fmt_by_id(task.from.as_str()), fmt_by_id(task.to.as_str())) else {
            return Verification::SignatureOnly;
        };
        let Some(demux) = self.demuxer(to) else { return Verification::SignatureOnly };
        let Ok(recipe) = self.recipe(from, to, task.source.as_ref(), &task.options) else {
            return Verification::Failed(
                tr!("impossible de recalculer la recette de conversion", "cannot recompute the conversion recipe").into(),
            );
        };
        let mut level: Vec<String> = Vec::new();
        if self.ffprobe.is_some() {
            let Some(v) = self.ffprobe_json(output, Some(demux), Duration::from_secs(120)) else {
                return Verification::Failed(
                    tr!("ffprobe ne parvient pas à lire le fichier produit", "ffprobe cannot read the output file").into(),
                );
            };
            let info = parse_probe(&v, to.kind);
            let (nv, na, ns) = counts(&info);
            let check = |label: &str, want: Option<usize>, got: usize| -> Result<(), String> {
                match want {
                    Some(w) if w != got => Err(trf!("{label} : {w} attendue(s), {got} trouvée(s)", "{label}: {w} expected, {got} found")),
                    _ => Ok(()),
                }
            };
            for (label, want, got) in [
                (tr!("pistes vidéo", "video tracks"), recipe.expect.video, nv),
                (tr!("pistes audio", "audio tracks"), recipe.expect.audio, na),
                (tr!("sous-titres", "subtitles"), recipe.expect.subtitle, ns),
            ] {
                if let Err(e) = check(label, want, got) {
                    return Verification::Failed(e);
                }
            }
            if nv + na + ns == 0 {
                return Verification::Failed(tr!("le fichier produit ne contient aucun flux", "the output file has no streams").into());
            }
            if let (Some(want), Some(got), false) = (recipe.duration, info.duration, recipe.loose_duration) {
                let tol = (want * 0.02).max(1.5);
                if (want - got).abs() > tol {
                    return Verification::Failed(trf!(
                        "durée {got:.1} s au lieu de {want:.1} s",
                        "duration {got:.1} s instead of {want:.1} s"
                    ));
                }
            }
            level.push(tr!("flux et durée contrôlés par ffprobe", "streams and duration checked with ffprobe").into());
        }
        // Décodage intégral quand c'est raisonnable (audio, sous-titres, images, vidéos courtes).
        let video_out = recipe.expect.video.unwrap_or(1) > 0 && to.kind == Kind::Video;
        let short = recipe.duration.is_some_and(|d| d <= 600.0);
        if !video_out || short {
            let mut target = std::ffi::OsString::from("file:");
            target.push(output.as_os_str());
            // Sous-titres texte : décodés puis ré-encodés en ASS (le sortie « null » exige un encodeur) ;
            // sous-titres image : relus en copie.
            let text_subs = to.kind == Kind::Subtitle && to.id != "sup";
            // Journal « verbose » avec niveaux : messages d'erreur et bilan « N decode errors » par flux.
            let spec = ProcessSpec::new(&self.ffmpeg)
                .args(["-hide_banner", "-nostdin", "-loglevel", "repeat+level+verbose", "-xerror", "-protocol_whitelist", "file,pipe"])
                .args(["-f", demux, "-i"])
                .arg(target)
                .args(["-map", "0:v?", "-map", "0:a?", "-map", "0:s?", "-c:s", if text_subs { "ass" } else { "copy" }, "-f", "null", "-"]);
            let mut log = DecodeLog::default();
            match run_tool(spec, ctx, &mut |_| {}, &mut |l| log.feed(l)) {
                Ok(out) => match log.verdict(out.success()) {
                    Ok(None) => level.push(tr!("décodage intégral sans erreur", "fully decoded without errors").into()),
                    Ok(Some(note)) => level.push(trf!("décodage intégral sans erreur ({note})", "fully decoded without errors ({note})")),
                    Err(e) => return Verification::Failed(trf!("erreurs au décodage : {e}", "decoding errors: {e}")),
                },
                Err(EngineError::Cancelled) => return Verification::Failed(tr!("vérification annulée", "verification cancelled").into()),
                Err(e) => return Verification::Failed(trf!("vérification impossible : {e}", "verification impossible: {e}")),
            }
            return Verification::Decoded(level.join(tr!(" ; ", "; ")));
        }
        if level.is_empty() { Verification::SignatureOnly } else { Verification::Structural(level.join(tr!(" ; ", "; "))) }
    }

    fn weight(&self, from: &FormatId, to: &FormatId) -> u32 {
        match (fmt_by_id(from.as_str()).map(|f| f.kind), fmt_by_id(to.as_str()).map(|f| f.kind)) {
            (_, Some(Kind::Video)) => 4,
            (Some(Kind::Video), Some(Kind::Anim)) => 2,
            _ => 1,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests unitaires (sans FFmpeg) ; les tests réels sont dans `tests/`.
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn full_caps() -> Capabilities {
        let mut c = Capabilities::default();
        for e in [
            "libx264",
            "libx265",
            "libsvtav1",
            "libvpx-vp9",
            "mpeg4",
            "aac",
            "libmp3lame",
            "libopus",
            "libvorbis",
            "flac",
            "alac",
            "pcm_s16le",
            "pcm_s24le",
            "pcm_s32le",
            "pcm_f32le",
            "pcm_s16be",
            "pcm_s24be",
            "srt",
            "ass",
            "webvtt",
            "mov_text",
            "gif",
            "apng",
            "libwebp",
            "libwebp_anim",
            "png",
            "mjpeg",
        ] {
            c.encoders.insert(e.into());
        }
        for d in ["mov", "mp4", "matroska", "webm", "wav", "flac", "mp3", "srt", "ass", "gif", "avi", "ogg"] {
            c.demuxers.insert(d.into());
        }
        for m in [
            "mp4", "matroska", "webm", "wav", "flac", "mp3", "srt", "ass", "webvtt", "gif", "apng", "webp", "image2", "ipod", "avi", "ogg",
            "opus", "adts",
        ] {
            c.muxers.insert(m.into());
        }
        c
    }

    fn engine() -> Ffmpeg {
        Ffmpeg::with_capabilities(PathBuf::from("/ffmpeg"), Some(PathBuf::from("/ffprobe")), None, EngineOrigin::System, full_caps())
    }

    fn stream(index: u32, kind: StreamKind, codec: &str) -> StreamInfo {
        StreamInfo {
            index,
            kind,
            codec: codec.into(),
            attached_pic: false,
            language: None,
            width: None,
            height: None,
            channels: None,
            sample_rate: None,
            bit_depth: None,
            sample_format: None,
            frame_rate: None,
            bitrate: None,
        }
    }

    fn video_src() -> SourceInfo {
        let mut v = stream(0, StreamKind::Video, "h264");
        v.width = Some(1920);
        v.height = Some(1080);
        v.sample_format = Some("yuv420p".into());
        let mut a = stream(1, StreamKind::Audio, "aac");
        a.channels = Some(2);
        a.sample_rate = Some(48000);
        a.sample_format = Some("fltp".into());
        SourceInfo {
            features: Some(features(&["video", "audio"])),
            duration: Some(60.0),
            streams: vec![v, a, stream(2, StreamKind::Subtitle, "subrip")],
            ..Default::default()
        }
    }

    #[test]
    fn parses_lists() {
        let enc = "Encoders:\n V..... = Video\n ------\n V....D libx264              H.264\n A..X.D dca                  DTS\n S..... srt                  SubRip\n";
        let set = parse_codec_list(enc);
        assert!(set.contains("libx264") && set.contains("srt"));
        assert!(!set.contains("dca"), "les encodeurs expérimentaux sont ignorés");
        let fmts =
            "Formats:\n D.. = Demuxing\n ---\n D   mov,mp4,m4a,3gp  QuickTime\n D d dshow           DirectShow\n  E  mp4             MP4\n";
        let set = parse_format_list(fmts);
        assert!(set.contains("mov") && set.contains("mp4") && set.contains("m4a"));
        assert!(!set.contains("dshow"), "les périphériques sont ignorés");
    }

    #[test]
    fn remux_is_lossless() {
        let e = engine();
        let src = video_src();
        let r = e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("mkv").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossless), "{:?}", r);
        assert!(r.output.windows(2).any(|w| w[0] == "-c:v:0" && w[1] == "copy"));
        assert!(r.output.windows(2).any(|w| w[0] == "-c:s:0" && w[1] == "copy"));
        assert_eq!(r.expect, Expect { video: Some(1), audio: Some(1), subtitle: Some(1) });
    }

    #[test]
    fn decode_log_tolerates_only_known_harmless_messages() {
        let summary = "[in#0/matroska,webm @ 0x1] [verbose]   Input stream #0:0 (audio): 153 packets read (68757 bytes); 153 frames decoded; 0 decode errors (146008 samples); ";
        // Rien à signaler.
        let mut log = DecodeLog::default();
        log.feed(summary);
        assert_eq!(log.verdict(true), Ok(None));
        // Message connu + zéro erreur de décodage confirmée : accepté, avec une remarque.
        let mut log = DecodeLog::default();
        log.feed("[opus @ 0x57ab8b5fc700] [error] Error parsing Opus packet header.");
        log.feed(summary);
        assert!(matches!(log.verdict(true), Ok(Some(_))), "{log:?}");
        // Même message mais erreurs de décodage : refusé.
        let mut log = DecodeLog::default();
        log.feed("[opus @ 0x57ab8b5fc700] [error] Error parsing Opus packet header.");
        log.feed(&summary.replace("0 decode errors", "3 decode errors"));
        assert!(log.verdict(true).is_err());
        // Même message sans bilan (FFmpeg ancien) : refusé, faute de preuve.
        let mut log = DecodeLog::default();
        log.feed("[opus @ 0x1] [error] Error parsing Opus packet header.");
        assert!(log.verdict(true).is_err());
        // Toute autre erreur : refusée, même avec un bilan à zéro.
        let mut log = DecodeLog::default();
        log.feed("[h264 @ 0x2] [error] Invalid NAL unit size (1234 > 567).");
        log.feed(summary);
        assert_eq!(log.verdict(true), Err("Invalid NAL unit size (1234 > 567).".into()));
        // Processus en échec : toujours refusé.
        assert!(DecodeLog::default().verdict(false).is_err());
    }

    #[test]
    fn ffmpeg_failures_are_readable() {
        let stderr = "[in#0 @ 000001a3eaaf54c0] moov atom not found
[in#0 @ 000001a3eaa63980] Error opening input: Invalid data found when processing input
Error opening input files: Invalid data found when processing input
";
        let e = failure(Some(-1094995529), stderr);
        assert!(matches!(&e, EngineError::InvalidInput(m) if m.starts_with("index MP4/MOV absent") && !m.contains("@ 0")), "{e}");
        let e = failure(
            Some(1),
            "[libx264 @ 0x1] something odd
[libx264 @ 0x1] something odd
Conversion failed!
",
        );
        assert_eq!(e.to_string(), "le moteur a échoué (code de sortie 1) : something odd | Conversion failed!");
    }

    #[test]
    fn styled_subtitles_are_never_called_lossless() {
        let e = engine();
        let mut src = video_src();
        src.streams[2].codec = "ass".into();
        // MKV → MP4 : ASS doit devenir mov_text ; vidéo et audio copiés, mais les styles peuvent se perdre.
        let r = e.recipe(fmt_by_id("mkv").unwrap(), fmt_by_id("mp4").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Depends), "{:?}", r);
        assert!(r.output.iter().any(|a| a == "mov_text"));
        // MKV → MKV garde l'ASS tel quel.
        let r = e.recipe(fmt_by_id("mkv").unwrap(), fmt_by_id("mkv").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossless));
        // Fichier ASS → SRT.
        let r = e.recipe(fmt_by_id("ass").unwrap(), fmt_by_id("srt").unwrap(), None, &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Depends));
    }

    #[test]
    fn webm_requires_reencode_and_says_so() {
        let e = engine();
        let src = video_src();
        let r = e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("webm").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossy));
        assert!(r.output.iter().any(|a| a == "libvpx-vp9"));
        assert!(r.output.iter().any(|a| a == "libopus"));
        assert!(r.output.iter().any(|a| a == "webvtt"), "srt converti en WebVTT");
    }

    #[test]
    fn audio_extraction_copies_when_possible() {
        let e = engine();
        let src = video_src();
        let r = e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("m4a").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossless));
        assert_eq!(r.expect.video, Some(0));
        let r = e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("mp3").unwrap(), Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossy));
    }

    #[test]
    fn reencode_option_forces_encoding_even_when_copy_is_possible() {
        let e = engine();
        let src = video_src();
        let (mkv, mp4) = (fmt_by_id("mkv").unwrap(), fmt_by_id("mp4").unwrap());
        let copy = e.recipe(mkv, mp4, Some(&src), &OptionValues::new()).unwrap();
        assert!(copy.notes.iter().any(|n| n.contains("copiée")), "{:?}", copy.notes);
        let opts = OptionValues::new().with("reencode", chud_core::OptionValue::Bool(true));
        let r = e.recipe(mkv, mp4, Some(&src), &opts).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossy), "un réencodage n'est jamais annoncé sans perte");
        assert!(!r.notes.iter().any(|n| n.contains("vidéo") && n.contains("copiée")), "{:?}", r.notes);
        let both = opts.clone().with("video_codec", chud_core::OptionValue::Text("copy".into()));
        assert!(e.recipe(mkv, mp4, Some(&src), &both).is_err(), "options contradictoires refusées");
    }

    #[test]
    fn honest_sample_formats() {
        let e = engine();
        let mut a = stream(0, StreamKind::Audio, "pcm_f32le");
        a.sample_format = Some("flt".into());
        a.channels = Some(2);
        a.sample_rate = Some(48000);
        let src = SourceInfo { streams: vec![a], duration: Some(10.0), ..Default::default() };
        let wav = fmt_by_id("wav").unwrap();
        let flac = fmt_by_id("flac").unwrap();
        let r = e.recipe(wav, flac, Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossy), "flottant → FLAC n'est pas sans perte");
        let mut s24 = stream(0, StreamKind::Audio, "pcm_s24le");
        s24.sample_format = Some("s32".into());
        s24.bit_depth = Some(24);
        let src = SourceInfo { streams: vec![s24], duration: Some(10.0), ..Default::default() };
        let r = e.recipe(wav, flac, Some(&src), &OptionValues::new()).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossless));
        let opts = OptionValues::new().with("bit_depth", chud_core::OptionValue::Text("16".into()));
        let r = e.recipe(wav, wav, Some(&src), &opts).unwrap();
        assert_eq!(r.fidelity, Some(Fidelity::Lossy), "24 → 16 bits est une perte");
    }

    #[test]
    fn rejects_bad_times_and_missing_streams() {
        let e = engine();
        let src = video_src();
        let opts = OptionValues::new().with("start", chud_core::OptionValue::Text("1;del *".into()));
        assert!(e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("mkv").unwrap(), Some(&src), &opts).is_err());
        let mut audio_only = video_src();
        audio_only.streams.retain(|s| s.kind == StreamKind::Audio);
        assert!(e.recipe(fmt_by_id("mp4").unwrap(), fmt_by_id("mkv").unwrap(), Some(&audio_only), &OptionValues::new()).is_err());
    }

    #[test]
    fn declared_graph_is_reasonable() {
        let e = engine();
        let has = |a: &str, b: &str| e.conversions().iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(has("mp4", "mkv") && has("mkv", "mp4") && has("mp4", "mp3") && has("wav", "flac"));
        assert!(has("mp4", "gif") && has("gif", "mp4") && has("srt", "vtt") && has("mp4", "png"));
        assert!(!has("srt", "srt"));
        // Aucune arête vers un format dont le multiplexeur est absent.
        assert!(!has("mp4", "mov"));
    }
}
