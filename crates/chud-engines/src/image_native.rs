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
    let mut keep = vec!["alpha", "high_bit_depth"];
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
                name: "Images (natif)".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                license: "MIT OR Apache-2.0".into(),
                homepage: "https://github.com/image-rs/image".into(),
                origin: EngineOrigin::Builtin,
                path: None,
                description: "Conversion d'images courantes en Rust pur, sans outil externe.".into(),
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
        let fmt = format_of(from).ok_or_else(|| EngineError::Unsupported(format!("lecture {from}")))?;
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
            notes.push("orientation EXIF appliquée aux pixels".into());
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

fn input_err(e: image::ImageError) -> EngineError {
    match e {
        image::ImageError::Limits(l) => EngineError::Refused(format!("image trop grande pour les limites de sécurité ({l})")),
        image::ImageError::Unsupported(u) => EngineError::Unsupported(u.to_string()),
        other => EngineError::InvalidInput(other.to_string()),
    }
}

fn output_err(e: image::ImageError) -> EngineError {
    EngineError::Other(format!("encodage impossible : {e}"))
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
                o.push(OptionSpec::int("quality", "Qualité", 1, 100, 90).help("90 : très bonne qualité ; 75 : fichiers plus légers"));
                o.push(OptionSpec::text("background", "Couleur de fond (transparence)", 7, "#ffffff").advanced());
            }
            "png" => o.push(
                OptionSpec::choice(
                    "png_compression",
                    "Compression PNG",
                    &[("fast", "Rapide"), ("default", "Standard"), ("best", "Maximale")],
                    "default",
                )
                .advanced(),
            ),
            _ => {}
        }
        o.push(OptionSpec::int("max_width", "Largeur maximale (0 = inchangée)", 0, 100_000, 0));
        o.push(OptionSpec::int("max_height", "Hauteur maximale (0 = inchangée)", 0, 100_000, 0));
        o.push(
            OptionSpec::choice(
                "resize_filter",
                "Filtre de redimensionnement",
                &[
                    ("lanczos3", "Lanczos (net)"),
                    ("catmullrom", "Catmull-Rom"),
                    ("triangle", "Bilinéaire"),
                    ("nearest", "Plus proche voisin (pixel art)"),
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
        let mut dec = Self::open(path, format.as_str()).ok()?.into_decoder().ok()?;
        let (w, h) = dec.dimensions();
        let color = dec.color_type();
        let icc = dec.icc_profile().ok().flatten().is_some();
        let exif = dec.exif_metadata().ok().flatten().is_some();
        drop(dec);
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
                ("Couleurs".into(), format!("{color:?}")),
                ("Profil ICC".into(), if icc { "oui" } else { "non" }.into()),
                ("EXIF".into(), if exif { "oui" } else { "non" }.into()),
                ("Animée".into(), if animated { "oui (seule la première image est convertie par ce moteur)" } else { "non" }.into()),
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
                && !matches!(to, "png" | "tiff" | "pnm" | "farbfeld" | "exr" | "hdr")
            {
                fidelity = Fidelity::Lossy;
                notes.push("réduction de la profondeur de couleur à 8 bits par canal".into());
            }
            if feats.is_some_and(|f| f.contains(&chud_core::Feature::new("animation"))) {
                notes.push("image animée : seule la première image est convertie par ce moteur".into());
            }
        } else if q.options.int_or("max_width", 0) > 0 || q.options.int_or("max_height", 0) > 0 {
            fidelity = fidelity.max(Fidelity::Depends);
        }
        if to == "gif" {
            notes.push("réduction à 256 couleurs".into());
        }
        StepAssessment { fidelity, notes, unavailable: None, preserves: None }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let to = task.to.as_str();
        ctx.report(Progress { fraction: Some(0.05), message: Some("décodage".into()), ..Default::default() });
        let (mut img, icc, mut notes) = Self::decode(&task.input, task.from.as_str())?;
        ctx.check_cancelled()?;

        if let Some((nw, nh)) = self.target_size(img.width(), img.height(), &task.options, to) {
            let filter = match task.options.str_or("resize_filter", "lanczos3") {
                "nearest" => FilterType::Nearest,
                "triangle" => FilterType::Triangle,
                "catmullrom" => FilterType::CatmullRom,
                _ => FilterType::Lanczos3,
            };
            ctx.report(Progress { fraction: Some(0.4), message: Some("redimensionnement".into()), ..Default::default() });
            notes.push(format!("redimensionné de {}×{} à {nw}×{nh}", img.width(), img.height()));
            img = img.resize_exact(nw, nh, filter);
        }
        ctx.check_cancelled()?;
        ctx.report(Progress { fraction: Some(0.6), message: Some("encodage".into()), ..Default::default() });

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
                    notes.push("transparence aplatie sur la couleur de fond".into());
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
            other => {
                let fmt = format_of(other).ok_or_else(|| EngineError::Unsupported(format!("écriture {other}")))?;
                let img = match other {
                    "bmp" | "tga" | "qoi" | "gif" | "ico" | "pnm" => {
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
            notes.push("profil ICC non conservé par ce format".into());
        }
        ctx.report(Progress { fraction: Some(1.0), ..Default::default() });
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        let to = task.to.as_str();
        let Some(_) = format_of(to) else { return Verification::SignatureOnly };
        match Self::open(output, to).and_then(|r| r.decode().map_err(input_err)) {
            Ok(img) if img.width() > 0 && img.height() > 0 => {
                Verification::Decoded(format!("image décodée : {} × {} px", img.width(), img.height()))
            }
            Ok(_) => Verification::Failed("image vide".into()),
            Err(e) => Verification::Failed(format!("le fichier produit ne se décode pas : {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chud_core::OptionValues;

    fn ctx(dir: &Path) -> ExecContext {
        ExecContext::new(dir.to_path_buf())
    }

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("chud-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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
        };
        let out = e.execute(&task, &ctx(out.parent().unwrap())).unwrap();
        assert!(e.verify(&task.output, &task, &ctx(out_dir(&task))).is_ok());
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
        };
        let err = e.execute(&task, &ctx(&d)).unwrap_err();
        assert!(matches!(err, EngineError::Refused(_) | EngineError::InvalidInput(_)), "{err:?}");
    }
}
