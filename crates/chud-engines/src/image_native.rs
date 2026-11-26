//! Moteur d'images natif (crate `image`, Rust pur).
//!
//! Points d'honnêteté :
//! - l'encodeur WebP de `image` ne fait que du **sans perte** : il est déclaré comme tel ;
//! - GIF (256 couleurs) et Radiance HDR (RGBE) sont déclarés **avec perte** ;
//! - les métadonnées EXIF/XMP ne sont pas réécrites : le planificateur l'annonce ;
//! - le profil ICC est recopié pour PNG, JPEG, WebP et TIFF ;
//! - l'orientation EXIF est appliquée aux pixels (sinon les photos ressortiraient tournées).

use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, FormatId, OptionSpec,
    Progress, SourceInfo, StepAssessment, StepOutput, StepQuery, StepTask, Verification,
};
use chud_core::{tr, trf};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

const READ: &[(&str, ImageFormat)] = &[
    ("png", ImageFormat::Png),
    ("apng", ImageFormat::Png),
    ("jpeg", ImageFormat::Jpeg),
    ("gif", ImageFormat::Gif),
    ("webp", ImageFormat::WebP),
    ("bmp", ImageFormat::Bmp),
    ("ico", ImageFormat::Ico),
    ("tiff", ImageFormat::Tiff),
    ("tga", ImageFormat::Tga),
    ("pnm", ImageFormat::Pnm),
    ("qoi", ImageFormat::Qoi),
    ("hdr", ImageFormat::Hdr),
    ("exr", ImageFormat::OpenExr),
    ("dds", ImageFormat::Dds),
    ("farbfeld", ImageFormat::Farbfeld),
];

const WRITE: &[&str] = &["png", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "tga", "pnm", "qoi", "hdr", "exr", "farbfeld"];

/// Taille maximale d'une icône.
const ICO_MAX: u32 = 256;
/// Au-delà (en pixels), la preuve « pixels identiques » est omise : deux copies 16 bits en mémoire.
const PIXEL_CHECK_MAX: u64 = 64_000_000;
/// Taille décodée maximale (octets) pour une analyse complète de la source.
const PROBE_DECODE_MAX: u64 = 256 * 1024 * 1024;

type Decoded = (DynamicImage, Option<Vec<u8>>, Vec<String>);

pub struct ImageNative {
    info: EngineInfo,
    caps: Vec<ConversionCap>,
}

impl Default for ImageNative {
    fn default() -> Self {
        Self::new()
    }
}

fn format_of(id: &str) -> Option<ImageFormat> {
    READ.iter().find(|(i, _)| *i == id).map(|(_, f)| *f)
}

fn declared_fidelity(to: &str) -> Fidelity {
    match to {
        "jpeg" | "gif" | "hdr" => Fidelity::Lossy,
        "ico" => Fidelity::Depends,
        _ => Fidelity::Lossless,
    }
}

fn preserved(to: &str) -> FeatureSet {
    // PNM : un fichier « .ppm » doit être un vrai PPM (P6), que tous les logiciels lisent ; la
    // transparence n'existe qu'en PAM (P7), que libvips et bien des visionneuses refusent.
    let mut keep = if to == "pnm" { vec!["high_bit_depth"] } else { vec!["alpha", "high_bit_depth"] };
    if matches!(to, "png" | "jpeg" | "webp" | "tiff") {
        keep.push("icc");
    }
    if matches!(to, "exr" | "hdr") {
        keep.push("hdr");
    }
    features(&keep)
}

impl ImageNative {
    pub fn new() -> Self {
        let mut caps = Vec::new();
        for (from, _) in READ {
            for to in WRITE {
                if from == to && !matches!(*to, "jpeg" | "png" | "webp") {
                    continue;
                }
                caps.push(ConversionCap::new(from, to, declared_fidelity(to), &preserved(to), 1));
            }
        }
        Self {
            info: EngineInfo {
                id: EngineId::new("image"),
                name: tr!("Images (natif)", "Images (native)").into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                license: "MIT OR Apache-2.0".into(),
                homepage: "https://github.com/image-rs/image".into(),
                origin: EngineOrigin::Builtin,
                path: None,
                description: tr!(
                    "Conversion d'images courantes en Rust pur, sans outil externe.",
                    "Common image conversions in pure Rust, without an external tool."
                )
                .into(),
            },
            caps,
        }
    }

    fn limits() -> Limits {
        let mut l = Limits::default();
        l.max_image_width = Some(100_000);
        l.max_image_height = Some(100_000);
        l.max_alloc = Some(2 * 1024 * 1024 * 1024);
        l
    }

    fn open(path: &Path, from: &str) -> Result<ImageReader<BufReader<File>>, EngineError> {
        let fmt = format_of(from).ok_or_else(|| EngineError::Unsupported(trf!("lecture {from}", "reading {from}")))?;
        let mut r = ImageReader::new(BufReader::new(File::open(path)?));
        r.set_format(fmt);
        r.limits(Self::limits());
        Ok(r)
    }

    /// Image décodée, profil ICC éventuel, remarques.
    fn decode(path: &Path, from: &str) -> Result<Decoded, EngineError> {
        let mut dec = Self::open(path, from)?.into_decoder().map_err(input_err)?;
        let icc = dec.icc_profile().ok().flatten();
        let orientation = dec.orientation().ok();
        let mut img = DynamicImage::from_decoder(dec).map_err(input_err)?;
        let mut notes = Vec::new();
        if let Some(o) = orientation
            && o != image::metadata::Orientation::NoTransforms
        {
            img.apply_orientation(o);
            notes.push(tr!("orientation EXIF appliquée aux pixels", "EXIF orientation applied to the pixels").into());
        }
        Ok((img, icc, notes))
    }

    fn target_size(&self, w: u32, h: u32, task_opts: &chud_core::OptionValues, to: &str) -> Option<(u32, u32)> {
        let mut max_w = task_opts.int_or("max_width", 0).max(0) as u32;
        let mut max_h = task_opts.int_or("max_height", 0).max(0) as u32;
        if to == "ico" {
            max_w = if max_w == 0 { ICO_MAX } else { max_w.min(ICO_MAX) };
            max_h = if max_h == 0 { ICO_MAX } else { max_h.min(ICO_MAX) };
        }
        let (mw, mh) = (if max_w == 0 { u32::MAX } else { max_w }, if max_h == 0 { u32::MAX } else { max_h });
        if w <= mw && h <= mh {
            return None;
        }
        let scale = (mw as f64 / w as f64).min(mh as f64 / h as f64);
        Some((((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1)))
    }
}

impl ImageNative {
    /// Échec du décodeur : une variante non prise en charge (DDS aux côtés non multiples de 4,
    /// TIFF en virgule flottante…) n'est pas un fichier abîmé — le moteur s'efface pour ce
    /// fichier ; une erreur de décodage ou de lecture, elle, signale un fichier abîmé (`None`).
    fn declined(&self, e: image::ImageError) -> Option<SourceInfo> {
        use image::ImageError as E;
        matches!(e, E::Unsupported(_) | E::Parameter(_) | E::Limits(_)).then(|| SourceInfo {
            features: None,
            summary: vec![(
                tr!("Moteur natif", "Native engine").into(),
                trf!("ne lit pas cette variante ({e})", "cannot read this variant ({e})"),
            )],
            unsupported_by: vec![self.info.id.clone()],
            ..Default::default()
        })
    }
}

fn input_err(e: image::ImageError) -> EngineError {
    match e {
        image::ImageError::Limits(l) => EngineError::Refused(trf!(
            "image trop grande pour les limites de sécurité ({l})",
            "image too large for the safety limits ({l})"
        )),
        image::ImageError::Unsupported(u) => EngineError::Unsupported(u.to_string()),
        other => EngineError::InvalidInput(other.to_string()),
    }
}

fn output_err(e: image::ImageError) -> EngineError {
    EngineError::Other(trf!("encodage impossible : {e}", "encoding impossible: {e}"))
}

fn has_alpha(c: ColorType) -> bool {
    c.has_alpha()
}

fn high_depth(c: ColorType) -> bool {
    c.bits_per_pixel() / u16::from(c.channel_count()) > 8
}

/// Aplatit la transparence sur une couleur de fond (pour JPEG).
fn flatten(img: &DynamicImage, bg: [u8; 3]) -> DynamicImage {
    if !img.color().has_alpha() {
        return img.clone();
    }
    let rgba: RgbaImage = img.to_rgba8();
    let mut out = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, Rgba([r, g, b, a])) in rgba.enumerate_pixels().map(|(x, y, p)| (x, y, *p)) {
        let a = a as u32;
        let mix = |c: u8, k: u8| ((c as u32 * a + k as u32 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([mix(r, bg[0]), mix(g, bg[1]), mix(b, bg[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

impl Engine for ImageNative {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        let mut o = Vec::new();
        match to.as_str() {
            "jpeg" => {
                o.push(
                    OptionSpec::int("quality", tr!("Qualité", "Quality"), 1, 100, 90)
                        .help(tr!("90 : très bonne qualité ; 75 : fichiers plus légers", "90: very good quality; 75: smaller files")),
                );
                o.push(
                    OptionSpec::text("background", tr!("Couleur de fond (transparence)", "Background color (transparency)"), 7, "#ffffff")
                        .advanced(),
                );
            }
            "png" => o.push(
                OptionSpec::choice(
                    "png_compression",
                    tr!("Compression PNG", "PNG compression"),
                    &[("fast", tr!("Rapide", "Fast")), ("default", "Standard"), ("best", tr!("Maximale", "Maximum"))],
                    "default",
                )
                .advanced(),
            ),
            _ => {}
        }
        o.push(OptionSpec::int("max_width", tr!("Largeur maximale (0 = inchangée)", "Maximum width (0 = unchanged)"), 0, 100_000, 0));
        o.push(OptionSpec::int("max_height", tr!("Hauteur maximale (0 = inchangée)", "Maximum height (0 = unchanged)"), 0, 100_000, 0));
        o.push(
            OptionSpec::choice(
                "resize_filter",
                tr!("Filtre de redimensionnement", "Resize filter"),
                &[
                    ("lanczos3", tr!("Lanczos (net)", "Lanczos (sharp)")),
                    ("catmullrom", "Catmull-Rom"),
                    ("triangle", tr!("Bilinéaire", "Bilinear")),
                    ("nearest", tr!("Plus proche voisin (pixel art)", "Nearest neighbor (pixel art)")),
                ],
                "lanczos3",
            )
            .advanced(),
        );
        o
    }

    fn can_probe(&self, format: &FormatId) -> bool {
        format_of(format.as_str()).is_some()
    }

    fn probe(&self, path: &Path, format: &FormatId) -> Option<SourceInfo> {
        let mut dec = match Self::open(path, format.as_str()).ok()?.into_decoder() {
            Ok(d) => d,
            Err(e) => return self.declined(e),
        };
        let (w, h) = dec.dimensions();
        let color = dec.color_type();
        let icc = dec.icc_profile().ok().flatten().is_some();
        let exif = dec.exif_metadata().ok().flatten().is_some();
        // Une image tronquée ou abîmée a souvent un en-tête intact : on la décode entièrement
        // (sauf si elle est très grande) pour ne pas proposer des conversions vouées à l'échec.
        if dec.total_bytes() <= PROBE_DECODE_MAX
            && let Err(e) = image::DynamicImage::from_decoder(dec)
        {
            return self.declined(e);
        }
        let animated = match format.as_str() {
            "gif" => File::open(path).ok().and_then(|f| {
                use image::AnimationDecoder;
                let d = image::codecs::gif::GifDecoder::new(BufReader::new(f)).ok()?;
                Some(d.into_frames().take(2).count() > 1)
            }),
            "png" | "apng" => {
                File::open(path).ok().and_then(|f| image::codecs::png::PngDecoder::new(BufReader::new(f)).ok()?.is_apng().ok())
            }
            "webp" => {
                File::open(path).ok().and_then(|f| Some(image::codecs::webp::WebPDecoder::new(BufReader::new(f)).ok()?.has_animation()))
            }
            _ => Some(false),
        }
        .unwrap_or(false);
        let mut feats = Vec::new();
        if has_alpha(color) {
            feats.push("alpha");
        }
        if high_depth(color) {
            feats.push("high_bit_depth");
        }
        if icc {
            feats.push("icc");
        }
        if exif {
            feats.push("exif");
        }
        if animated {
            feats.push("animation");
        }
        if matches!(format.as_str(), "hdr" | "exr") {
            feats.push("hdr");
        }
        Some(SourceInfo {
            features: Some(features(&feats)),
            width: Some(w),
            height: Some(h),
            summary: vec![
                ("Dimensions".into(), format!("{w} × {h} px")),
                (tr!("Couleurs", "Colors").into(), format!("{color:?}")),
                (tr!("Profil ICC", "ICC profile").into(), if icc { tr!("oui", "yes") } else { tr!("non", "no") }.into()),
                ("EXIF".into(), if exif { tr!("oui", "yes") } else { tr!("non", "no") }.into()),
                (
                    tr!("Animée", "Animated").into(),
                    if animated {
                        tr!(
                            "oui (seule la première image est convertie par ce moteur)",
                            "yes (only the first frame is converted by this engine)"
                        )
                    } else {
                        tr!("non", "no")
                    }
                    .into(),
                ),
            ],
            ..Default::default()
        })
    }

    fn assess(&self, q: &StepQuery<'_>) -> StepAssessment {
        let to = q.to.as_str();
        let mut fidelity = declared_fidelity(to);
        let mut notes = Vec::new();
        if let Some(src) = q.source {
            if let (Some(w), Some(h)) = (src.width, src.height) {
                match self.target_size(w, h, q.options, to) {
                    Some((nw, nh)) => {
                        fidelity = Fidelity::Lossy;
                        notes.push(format!("redimensionnement {w}×{h} → {nw}×{nh}"));
                    }
                    None if to == "ico" => fidelity = Fidelity::Lossless,
                    None => {}
                }
            }
            let feats = src.features.as_ref();
            if feats.is_some_and(|f| f.contains(&chud_core::Feature::new("high_bit_depth")))
                && !matches!(to, "png" | "tiff" | "farbfeld" | "exr" | "hdr")
            {
                fidelity = Fidelity::Lossy;
                notes.push(
                    tr!("réduction de la profondeur de couleur à 8 bits par canal", "color depth reduced to 8 bits per channel").into(),
                );
            }
            if feats.is_some_and(|f| f.contains(&chud_core::Feature::new("animation"))) {
                notes.push(
                    tr!(
                        "image animée : seule la première image est convertie par ce moteur",
                        "animated image: only the first frame is converted by this engine"
                    )
                    .into(),
                );
            }
        } else if q.options.int_or("max_width", 0) > 0 || q.options.int_or("max_height", 0) > 0 {
            fidelity = fidelity.max(Fidelity::Depends);
        }
        if to == "gif" {
            notes.push(tr!("réduction à 256 couleurs", "reduced to 256 colors").into());
        }
        StepAssessment { fidelity, notes, unavailable: None, preserves: None }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let to = task.to.as_str();
        ctx.report(Progress { fraction: Some(0.05), message: Some(tr!("décodage", "decoding").into()), ..Default::default() });
        let (mut img, icc, mut notes) = Self::decode(&task.input, task.from.as_str())?;
        ctx.check_cancelled()?;

        if let Some((nw, nh)) = self.target_size(img.width(), img.height(), &task.options, to) {
            let filter = match task.options.str_or("resize_filter", "lanczos3") {
                "nearest" => FilterType::Nearest,
                "triangle" => FilterType::Triangle,
                "catmullrom" => FilterType::CatmullRom,
                _ => FilterType::Lanczos3,
            };
            ctx.report(Progress { fraction: Some(0.4), message: Some(tr!("redimensionnement", "resizing").into()), ..Default::default() });
            notes.push(trf!("redimensionné de {}×{} à {nw}×{nh}", "resized from {}×{} to {nw}×{nh}", img.width(), img.height()));
            img = img.resize_exact(nw, nh, filter);
        }
        ctx.check_cancelled()?;
        ctx.report(Progress { fraction: Some(0.6), message: Some(tr!("encodage", "encoding").into()), ..Default::default() });

        let file = File::create(&task.output)?;
        let mut w = BufWriter::new(file);
        let icc_for = |enc: &mut dyn FnMut(Vec<u8>) -> bool| {
            if let Some(p) = icc.clone() {
                enc(p);
            }
        };
        match to {
            "jpeg" => {
                let bg = parse_hex_color(task.options.str_or("background", "#ffffff")).ok_or_else(|| EngineError::InvalidOption {
                    key: "background".into(),
                    message: "couleur attendue au format #rrggbb".into(),
                })?;
                if img.color().has_alpha() {
                    notes.push(
                        tr!("transparence aplatie sur la couleur de fond", "transparency flattened onto the background color").into(),
                    );
                }
                let rgb = match flatten(&img, bg) {
                    DynamicImage::ImageLuma8(l) => DynamicImage::ImageLuma8(l),
                    other => DynamicImage::ImageRgb8(other.to_rgb8()),
                };
                let q = task.options.int_or("quality", 90).clamp(1, 100) as u8;
                let mut enc = JpegEncoder::new_with_quality(&mut w, q);
                icc_for(&mut |p| enc.set_icc_profile(p).is_ok());
                rgb.write_with_encoder(enc).map_err(output_err)?;
            }
            "png" => {
                let comp = match task.options.str_or("png_compression", "default") {
                    "fast" => CompressionType::Fast,
                    "best" => CompressionType::Best,
                    _ => CompressionType::Default,
                };
                let img = match img {
                    DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => DynamicImage::ImageRgba16(img.to_rgba16()),
                    other => other,
                };
                let mut enc = PngEncoder::new_with_quality(&mut w, comp, PngFilter::Adaptive);
                icc_for(&mut |p| enc.set_icc_profile(p).is_ok());
                img.write_with_encoder(enc).map_err(output_err)?;
            }
            "webp" => {
                let img =
                    if img.color().has_alpha() { DynamicImage::ImageRgba8(img.to_rgba8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
                let mut enc = WebPEncoder::new_lossless(&mut w);
                icc_for(&mut |p| enc.set_icc_profile(p).is_ok());
                img.write_with_encoder(enc).map_err(output_err)?;
            }
            "tiff" => {
                let img = match img {
                    DynamicImage::ImageLuma8(_) | DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => img,
                    DynamicImage::ImageLuma16(_) | DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => img,
                    DynamicImage::ImageLumaA8(_) => DynamicImage::ImageRgba8(img.to_rgba8()),
                    other if other.color().has_alpha() => DynamicImage::ImageRgba16(other.to_rgba16()),
                    other => DynamicImage::ImageRgb16(other.to_rgb16()),
                };
                // TIFF exige un flux « seekable » : on encode en mémoire.
                let mut buf = std::io::Cursor::new(Vec::new());
                let mut enc = TiffEncoder::new(&mut buf);
                icc_for(&mut |p| enc.set_icc_profile(p).is_ok());
                img.write_with_encoder(enc).map_err(output_err)?;
                w.write_all(&buf.into_inner())?;
            }
            "pnm" => {
                // Sans sous-type imposé, `image` écrit du PAM (P7) — même sous le nom « .ppm ».
                use image::codecs::pnm::{PnmEncoder, PnmSubtype, SampleEncoding};
                let enc = PnmEncoder::new(&mut w).with_subtype(PnmSubtype::Pixmap(SampleEncoding::Binary));
                DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(enc).map_err(output_err)?;
            }
            other => {
                let fmt = format_of(other).ok_or_else(|| EngineError::Unsupported(trf!("écriture {other}", "writing {other}")))?;
                let img = match other {
                    // ICO : le décodeur exige des PNG internes en RGBA, même sans transparence.
                    "ico" => DynamicImage::ImageRgba8(img.to_rgba8()),
                    "bmp" | "tga" | "qoi" | "gif" => {
                        if img.color().has_alpha() {
                            DynamicImage::ImageRgba8(img.to_rgba8())
                        } else {
                            DynamicImage::ImageRgb8(img.to_rgb8())
                        }
                    }
                    "farbfeld" => DynamicImage::ImageRgba16(img.to_rgba16()),
                    "hdr" => DynamicImage::ImageRgb32F(img.to_rgb32f()),
                    "exr" => {
                        if img.color().has_alpha() {
                            DynamicImage::ImageRgba32F(img.to_rgba32f())
                        } else {
                            DynamicImage::ImageRgb32F(img.to_rgb32f())
                        }
                    }
                    _ => img,
                };
                // Encodage en mémoire : certains encodeurs (EXR, TIFF) exigent `Seek`.
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, fmt).map_err(output_err)?;
                w.write_all(&buf.into_inner())?;
            }
        }
        w.flush()?;
        if icc.is_some() && !matches!(to, "png" | "jpeg" | "webp" | "tiff") {
            notes.push(tr!("profil ICC non conservé par ce format", "ICC profile not kept by this format").into());
        }
        ctx.report(Progress { fraction: Some(1.0), ..Default::default() });
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        let to = task.to.as_str();
        let Some(_) = format_of(to) else { return Verification::SignatureOnly };
        let img = match Self::open(output, to).and_then(|r| r.decode().map_err(input_err)) {
            Ok(img) if img.width() > 0 && img.height() > 0 => img,
            Ok(_) => return Verification::Failed(tr!("image vide", "empty image").into()),
            Err(e) => {
                return Verification::Failed(trf!("le fichier produit ne se décode pas : {e}", "the output file does not decode: {e}"));
            }
        };
        let (w, h) = (img.width(), img.height());
        let mut detail = trf!("image décodée : {w} × {h} px", "image decoded: {w} × {h} px");
        // Dimensions attendues, puis « sans perte » prouvé pixel par pixel quand il est annoncé.
        let Ok((src, _, _)) = Self::decode(&task.input, task.from.as_str()) else { return Verification::Decoded(detail) };
        let expected = self.target_size(src.width(), src.height(), &task.options, to).unwrap_or((src.width(), src.height()));
        if (w, h) != expected {
            return Verification::Failed(trf!(
                "dimensions {w} × {h} au lieu de {} × {}",
                "dimensions {w} × {h} instead of {} × {}",
                expected.0,
                expected.1
            ));
        }
        let q = StepQuery { from: &task.from, to: &task.to, source: task.source.as_ref(), options: &task.options };
        if self.assess(&q).fidelity == Fidelity::Lossless && (w, h) == (src.width(), src.height()) {
            if u64::from(w) * u64::from(h) > PIXEL_CHECK_MAX {
                detail
                    .push_str(tr!(" ; comparaison des pixels omise (image très grande)", "; pixel comparison skipped (very large image)"));
            } else if if preserved(to).contains(&chud_core::Feature::new("alpha")) {
                src.to_rgba16().as_raw() == img.to_rgba16().as_raw()
            } else {
                // Transparence non gardée (perte annoncée à part) : les couleurs, elles, doivent l'être.
                src.to_rgb16().as_raw() == img.to_rgb16().as_raw()
            } {
                detail.push_str(tr!(" ; pixels identiques à la source", "; pixels identical to the source"));
            } else {
                return Verification::Failed(
                    tr!(
                        "conversion annoncée sans perte, mais les pixels diffèrent de la source",
                        "conversion announced as lossless, but the pixels differ from the source"
                    )
                    .into(),
                );
            }
        }
        Verification::Decoded(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chud_core::OptionValues;

    fn ctx(dir: &Path) -> ExecContext {
        ExecContext::new(dir.to_path_buf())
    }

    #[test]
    fn truncated_image_is_not_probed_as_valid() {
        let d = tmpdir("trunc");
        let full = d.join("full.png");
        sample(&full, true);
        let bytes = std::fs::read(&full).unwrap();
        let cut = d.join("cut.png");
        std::fs::write(&cut, &bytes[..bytes.len() * 2 / 3]).unwrap();
        let e = ImageNative::new();
        assert!(e.probe(&full, &"png".into()).is_some());
        assert!(e.probe(&cut, &"png".into()).is_none(), "en-tête intact mais données manquantes");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Dossier de test retiré à la fin du test, même en cas d'échec.
    struct TmpDir(std::path::PathBuf);

    impl std::ops::Deref for TmpDir {
        type Target = std::path::PathBuf;
        fn deref(&self) -> &std::path::PathBuf {
            &self.0
        }
    }

    impl AsRef<Path> for TmpDir {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmpdir(name: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!("chud-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    fn sample(path: &Path, alpha: bool) -> RgbaImage {
        let mut img = RgbaImage::new(37, 23);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x * 7) as u8, (y * 11) as u8, ((x + y) * 3) as u8, if alpha { (x * 6) as u8 } else { 255 }]);
        }
        img.save(path).unwrap();
        img
    }

    fn convert(e: &ImageNative, input: &Path, from: &str, to: &str, out: &Path, opts: OptionValues) -> StepOutput {
        let specs = e.options(&from.into(), &to.into());
        let task = StepTask {
            input: input.into(),
            output: out.into(),
            from: from.into(),
            to: to.into(),
            options: opts.resolve(&specs).unwrap(),
            source: None,

            name: None,
        };
        let out = e.execute(&task, &ctx(out.parent().unwrap())).unwrap();
        let v = e.verify(&task.output, &task, &ctx(out_dir(&task)));
        assert!(v.is_ok(), "{v:?}");
        out
    }

    fn out_dir(t: &StepTask) -> &Path {
        t.output.parent().unwrap()
    }

    #[test]
    fn lossless_roundtrip_png_bmp_png_is_pixel_exact() {
        let d = tmpdir("rt");
        let e = ImageNative::new();
        let src = sample(&d.join("a.png"), true);
        convert(&e, &d.join("a.png"), "png", "bmp", &d.join("a.bmp"), OptionValues::new());
        convert(&e, &d.join("a.bmp"), "bmp", "png", &d.join("b.png"), OptionValues::new());
        let back = image::open(d.join("b.png")).unwrap().to_rgba8();
        assert_eq!(src, back, "PNG → BMP → PNG doit être identique au pixel près");
        convert(&e, &d.join("a.png"), "png", "webp", &d.join("c.webp"), OptionValues::new());
        let webp = image::open(d.join("c.webp")).unwrap().to_rgba8();
        assert_eq!(src, webp, "le WebP produit par ce moteur est sans perte");
    }

    #[test]
    fn jpeg_flattens_alpha_and_resizes() {
        let d = tmpdir("jpg");
        let e = ImageNative::new();
        sample(&d.join("a.png"), true);
        let opts = OptionValues::new().with("max_width", chud_core::OptionValue::Int(10));
        let out = convert(&e, &d.join("a.png"), "png", "jpeg", &d.join("a.jpg"), opts);
        let j = image::open(d.join("a.jpg")).unwrap();
        assert_eq!(j.width(), 10);
        assert!(!j.color().has_alpha());
        assert!(out.notes.iter().any(|n| n.contains("transparence")));
    }

    /// DDS compressé en DXT1 : en-tête de 128 octets puis `blocks` blocs de 8 octets.
    fn dds(w: u32, h: u32, blocks: usize) -> Vec<u8> {
        let mut hdr = [0u32; 31];
        (hdr[0], hdr[1], hdr[2], hdr[3]) = (124, 0x1 | 0x2 | 0x4 | 0x1000, h, w);
        (hdr[18], hdr[19], hdr[20], hdr[26]) = (32, 0x4, u32::from_le_bytes(*b"DXT1"), 0x1000);
        let mut v = b"DDS ".to_vec();
        hdr.iter().for_each(|x| v.extend_from_slice(&x.to_le_bytes()));
        v.extend(std::iter::repeat_n(0u8, blocks * 8));
        v
    }

    #[test]
    fn unsupported_variants_are_not_called_damaged() {
        let d = tmpdir("dds");
        let e = ImageNative::new();
        let dds_id: FormatId = "dds".into();
        // 4×4 complet : lu normalement.
        std::fs::write(d.join("ok.dds"), dds(4, 4, 1)).unwrap();
        let ok = e.probe(&d.join("ok.dds"), &dds_id).unwrap();
        assert!(ok.unsupported_by.is_empty() && ok.width == Some(4));
        // 6×6 (côtés non multiples de 4, valide, écrit par ImageMagick) : le moteur s'efface.
        std::fs::write(d.join("odd.dds"), dds(6, 6, 4)).unwrap();
        let odd = e.probe(&d.join("odd.dds"), &dds_id).expect("pas « abîmé »");
        assert_eq!(odd.unsupported_by, vec![EngineId::new("image")]);
        assert!(odd.features.is_none());
        // Tronqué : vraiment abîmé.
        std::fs::write(d.join("cut.dds"), dds(8, 8, 1)).unwrap();
        assert!(e.probe(&d.join("cut.dds"), &dds_id).is_none());
    }

    #[test]
    fn pnm_is_a_plain_ppm_that_everyone_reads() {
        let d = tmpdir("pnm");
        let e = ImageNative::new();
        sample(&d.join("a.png"), true);
        convert(&e, &d.join("a.png"), "png", "pnm", &d.join("a.ppm"), OptionValues::new());
        let bytes = std::fs::read(d.join("a.ppm")).unwrap();
        assert_eq!(&bytes[..2], b"P6", "pas de PAM (P7) sous le nom .ppm");
        let cap = e.conversions().iter().find(|c| c.from.as_str() == "png" && c.to.as_str() == "pnm").unwrap();
        assert!(!cap.preserves.contains(&chud_core::Feature::new("alpha")), "la perte de transparence est annoncée");
    }

    #[test]
    fn ico_is_capped_at_256() {
        let d = tmpdir("ico");
        let e = ImageNative::new();
        RgbaImage::from_pixel(600, 300, Rgba([1, 2, 3, 255])).save(d.join("big.png")).unwrap();
        convert(&e, &d.join("big.png"), "png", "ico", &d.join("big.ico"), OptionValues::new());
        let ico = image::open(d.join("big.ico")).unwrap();
        assert_eq!((ico.width(), ico.height()), (256, 128));
    }

    #[test]
    fn assess_is_honest() {
        let e = ImageNative::new();
        let src = SourceInfo { width: Some(600), height: Some(300), features: Some(features(&["high_bit_depth"])), ..Default::default() };
        let opts = OptionValues::new();
        let (png, bmp, ico): (FormatId, FormatId, FormatId) = ("png".into(), "bmp".into(), "ico".into());
        let a = e.assess(&StepQuery { from: &png, to: &bmp, source: Some(&src), options: &opts });
        assert_eq!(a.fidelity, Fidelity::Lossy, "16 bits → 8 bits n'est pas sans perte");
        let a = e.assess(&StepQuery { from: &png, to: &ico, source: Some(&src), options: &opts });
        assert_eq!(a.fidelity, Fidelity::Lossy, "600 px → 256 px");
    }

    #[test]
    fn ico_from_an_opaque_image_reads_back() {
        // Régression : l'ICO écrit depuis une image RGB contenait un PNG RGB que le décodeur ICO refuse.
        let d = tmpdir("ico-opaque");
        let e = ImageNative::new();
        sample(&d.join("opaque.png"), false);
        convert(&e, &d.join("opaque.png"), "png", "ico", &d.join("opaque.ico"), OptionValues::new());
        let back = ImageNative::open(&d.join("opaque.ico"), "ico").unwrap().decode().unwrap();
        assert_eq!((back.width(), back.height()), (37, 23));
    }

    #[test]
    fn sixteen_bit_to_pnm_is_not_called_lossless() {
        let e = ImageNative::new();
        let src = SourceInfo { features: Some(features(&["high_bit_depth"])), width: Some(10), height: Some(10), ..Default::default() };
        let opts = OptionValues::new();
        let a = e.assess(&StepQuery { from: &"png".into(), to: &"pnm".into(), source: Some(&src), options: &opts });
        assert_eq!(a.fidelity, Fidelity::Lossy, "le PNM écrit par ce moteur est en 8 bits");
    }

    #[test]
    fn lossless_claim_is_checked_pixel_by_pixel() {
        let d = tmpdir("pixels");
        let e = ImageNative::new();
        sample(&d.join("a.png"), true);
        let task = StepTask {
            input: d.join("a.png"),
            output: d.join("a.bmp"),
            from: "png".into(),
            to: "bmp".into(),
            options: OptionValues::new(),
            source: None,

            name: None,
        };
        e.execute(&task, &ctx(&d)).unwrap();
        assert!(matches!(e.verify(&task.output, &task, &ctx(&d)), Verification::Decoded(ref s) if s.contains("identi")));
        // Un résultat altéré d'un seul pixel n'est plus « sans perte » : la vérification échoue.
        let mut img = image::open(&task.output).unwrap().to_rgba8();
        img.put_pixel(3, 4, Rgba([1, 2, 3, 255]));
        img.save_with_format(&task.output, ImageFormat::Bmp).unwrap();
        assert!(matches!(e.verify(&task.output, &task, &ctx(&d)), Verification::Failed(_)));
        // Mauvaises dimensions : échec également.
        RgbaImage::new(5, 5).save_with_format(&task.output, ImageFormat::Bmp).unwrap();
        assert!(matches!(e.verify(&task.output, &task, &ctx(&d)), Verification::Failed(ref s) if s.contains("37")));
    }

    #[test]
    fn decompression_bomb_is_refused() {
        // En-tête PNG annonçant 100 000 × 100 000 pixels : refusé avant allocation.
        let d = tmpdir("bomb");
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        img.save(d.join("tiny.png")).unwrap();
        let mut bytes = std::fs::read(d.join("tiny.png")).unwrap();
        bytes[16..20].copy_from_slice(&90_000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&90_000u32.to_be_bytes());
        std::fs::write(d.join("bomb.png"), &bytes).unwrap();
        let e = ImageNative::new();
        let task = StepTask {
            input: d.join("bomb.png"),
            output: d.join("bomb.bmp"),
            from: "png".into(),
            to: "bmp".into(),
            options: OptionValues::new(),
            source: None,

            name: None,
        };
        let err = e.execute(&task, &ctx(&d)).unwrap_err();
        assert!(matches!(err, EngineError::Refused(_) | EngineError::InvalidInput(_)), "{err:?}");
    }
}
