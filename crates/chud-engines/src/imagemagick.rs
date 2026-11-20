//! Adaptateur ImageMagick : images dans plus de 200 formats (PSD, XCF, HEIC, JPEG XL, RAW…).
//!
//! - Les formats lus et écrits sont demandés à l'exécutable (`-list format`), puis limités à
//!   ceux du catalogue. Les formats qui passeraient par un programme délégué (PDF, PostScript
//!   via Ghostscript) ne sont jamais confiés à ImageMagick.
//! - Le décodeur est toujours imposé (`psd:input.psd`) : le contenu ne peut pas faire choisir un
//!   autre décodeur (MSL, MVG, texte…). Fichiers sous des noms simples dans le dossier de travail.
//! - Ressources plafonnées (`-limit`) : une image piégée ne peut pas épuiser la mémoire.
//! - Vérification : le résultat est relu et décodé entièrement par ImageMagick.

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, Fidelity, FormatId, OptionSpec, StepOutput, StepTask,
    Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// (format du catalogue, décodeur ImageMagick, écriture autorisée)
const MAP: &[(&str, &str, bool)] = &[
    ("png", "PNG", true),
    // Lecture seulement : pour une image fixe, ImageMagick écrit un PNG ordinaire sous le nom
    // APNG (sans bloc d'animation), que FFmpeg refuse ensuite. FFmpeg et le moteur natif écrivent l'APNG.
    ("apng", "APNG", false),
    ("jpeg", "JPEG", true),
    ("webp", "WEBP", true),
    ("avif", "AVIF", true),
    ("heic", "HEIC", true),
    ("jxl", "JXL", true),
    ("jp2", "JP2", true),
    ("gif", "GIF", true),
    ("bmp", "BMP", true),
    ("tiff", "TIFF", true),
    ("ico", "ICO", true),
    ("cur", "CUR", false),
    ("icns", "ICNS", false),
    ("svg", "SVG", false),
    ("psd", "PSD", true),
    ("xcf", "XCF", false),
    ("exr", "EXR", true),
    ("hdr", "HDR", true),
    ("dds", "DDS", true),
    ("tga", "TGA", true),
    ("pcx", "PCX", true),
    ("pnm", "PNM", true),
    ("qoi", "QOI", true),
    ("farbfeld", "FARBFELD", true),
    ("dng", "DNG", false),
    ("cr2", "CR2", false),
    ("cr3", "CR3", false),
    ("nef", "NEF", false),
    ("arw", "ARW", false),
    ("orf", "ORF", false),
    ("rw2", "RW2", false),
    ("raf", "RAF", false),
    ("pef", "PEF", false),
    ("emf", "EMF", false),
    ("wmf", "WMF", false),
];

pub(crate) const RAW: &[&str] = &["dng", "cr2", "cr3", "nef", "arw", "orf", "rw2", "raf", "pef"];
const LOSSY_TARGETS: &[&str] = &["jpeg", "gif", "webp", "avif", "heic", "jxl", "jp2"];
const MULTI: &[&str] = &["gif", "apng", "webp", "tiff", "ico"];
/// Côté maximal d'une image dans une icône : ImageMagick refuse d'écrire au-delà.
const ICO_MAX: u32 = 256;

/// Dimensions après réduction pour une icône, si la source dépasse 256 px.
fn ico_shrink(w: u32, h: u32) -> Option<(u32, u32)> {
    if w <= ICO_MAX && h <= ICO_MAX {
        return None;
    }
    let scale = (ICO_MAX as f64 / w as f64).min(ICO_MAX as f64 / h as f64);
    Some((((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1)))
}

/// Fidélité d'une conversion d'image matricielle (partagée avec libvips).
pub(crate) fn raster_fidelity(from: &str, to: &str) -> Fidelity {
    let vector = matches!(from, "svg" | "emf" | "wmf");
    // Haute dynamique vers 8 bits : valeurs écrêtées.
    let hdr_clipped = matches!(from, "exr" | "hdr") && !matches!(to, "exr" | "hdr" | "tiff");
    if LOSSY_TARGETS.contains(&to) || RAW.contains(&from) || vector || hdr_clipped { Fidelity::Lossy } else { Fidelity::Depends }
}

pub(crate) fn raster_keep(from: &str, to: &str) -> chud_core::FeatureSet {
    let mut f = vec!["alpha", "icc", "exif", "xmp", "iptc", "high_bit_depth", "cmyk"];
    if MULTI.contains(&from) && MULTI.contains(&to) {
        f.extend(["multipage", "animation"]);
    }
    features(&f)
}

/// Lignes de `magick -list format` : « PNG* PNG rw- Portable… » ou « APNG rw+ Animated… ».
fn parse_formats(text: &str) -> HashMap<String, (bool, bool)> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = toks.first() else { continue };
        let name = name.trim_end_matches('*');
        let is_mode = |t: &&&str| {
            let m = t.as_bytes();
            m.len() == 3 && matches!(m[0], b'r' | b'-') && matches!(m[1], b'w' | b'-') && matches!(m[2], b'+' | b'-')
        };
        let Some(mode) = toks.iter().skip(1).take(2).find(is_mode) else { continue };
        let m = mode.as_bytes();
        out.insert(name.to_owned(), (m[0] == b'r', m[1] == b'w'));
    }
    out
}

pub struct ImageMagick {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
}

fn coder(id: &str) -> Option<&'static str> {
    MAP.iter().find(|m| m.0 == id).map(|m| m.1)
}

impl ImageMagick {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let out = tool::query(&exe, &["-list", "format"])?;
        if !out.success() {
            return Err(tr!("magick -list format a échoué", "magick -list format failed").into());
        }
        let formats = parse_formats(&out.stdout_text());
        if formats.is_empty() {
            return Err(tr!("liste des formats ImageMagick illisible", "unreadable ImageMagick format list").into());
        }
        Ok(Self::with_formats(exe, version, origin, &formats))
    }

    fn with_formats(exe: PathBuf, version: Option<String>, origin: EngineOrigin, formats: &HashMap<String, (bool, bool)>) -> Self {
        let can = |c: &str| formats.get(c).copied().unwrap_or((false, false));
        let readable: Vec<&str> = MAP.iter().filter(|m| can(m.1).0).map(|m| m.0).collect();
        let writable: Vec<&str> = MAP.iter().filter(|m| m.2 && can(m.1).1).map(|m| m.0).collect();
        let mut caps = Vec::new();
        for from in &readable {
            for to in &writable {
                if from != to {
                    caps.push(ConversionCap::new(from, to, raster_fidelity(from, to), &raster_keep(from, to), 3));
                }
            }
        }
        Self {
            info: tool::engine_info(
                "imagemagick",
                "ImageMagick",
                version,
                origin,
                &exe,
                tr!("ImageMagick (compatible Apache-2.0)", "ImageMagick (Apache-2.0 compatible)"),
                "https://imagemagick.org",
                tr!(
                    "Images : PSD, XCF, HEIC, JPEG XL, RAW et plus de 100 autres formats.",
                    "Images: PSD, XCF, HEIC, JPEG XL, RAW and more than 100 other formats."
                ),
            ),
            exe,
            caps,
        }
    }

    fn limits(spec: ProcessSpec) -> ProcessSpec {
        spec.args([
            "-limit", "memory", "1GiB", "-limit", "map", "2GiB", "-limit", "disk", "8GiB", "-limit", "width", "64KP", "-limit", "height",
            "64KP",
        ])
    }
}

impl Engine for ImageMagick {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        if LOSSY_TARGETS.contains(&to.as_str()) && to.as_str() != "gif" {
            vec![OptionSpec::int("quality", tr!("Qualité", "Quality"), 1, 100, 90)]
        } else {
            Vec::new()
        }
    }

    fn assess(&self, q: &chud_core::StepQuery<'_>) -> chud_core::StepAssessment {
        let Some(cap) = self.caps.iter().find(|c| &c.from == q.from && &c.to == q.to) else {
            return chud_core::StepAssessment {
                fidelity: Fidelity::Lossy,
                notes: Vec::new(),
                unavailable: Some(tr!("conversion non déclarée par le moteur", "conversion not declared by the engine").into()),
                preserves: None,
            };
        };
        let mut a = chud_core::StepAssessment { fidelity: cap.fidelity, notes: Vec::new(), unavailable: None, preserves: None };
        let size = q.source.and_then(|s| s.width.zip(s.height));
        if q.to.as_str() == "ico"
            && let Some(((w, h), (nw, nh))) = size.and_then(|(w, h)| Some(((w, h), ico_shrink(w, h)?)))
        {
            a.fidelity = Fidelity::Lossy;
            a.notes.push(trf!(
                "redimensionnement {w}×{h} → {nw}×{nh} (taille maximale d'une icône)",
                "resized {w}×{h} → {nw}×{nh} (largest icon size)"
            ));
        }
        a
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let (from, to) = (task.from.as_str(), task.to.as_str());
        let (Some(cin), Some(cout)) = (coder(from), coder(to)) else {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        };
        if !self.caps.iter().any(|c| c.from.as_str() == from && c.to.as_str() == to) {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        }
        let stage = Stage::new(ctx, "imagemagick")?;
        let in_name = format!("input.{}", tool::ext(from));
        stage.input(&task.input, &in_name)?;
        let out_name = format!("out.{}", tool::ext(to));
        let mut notes = Vec::new();
        // Cible à image unique : la première image seulement (sinon plusieurs fichiers numérotés).
        let source = if MULTI.contains(&to) { format!("{cin}:{in_name}") } else { format!("{cin}:{in_name}[0]") };
        if !MULTI.contains(&to) && MULTI.contains(&from) {
            notes.push(tr!("première image seulement", "first frame only").to_owned());
        }
        let mut spec = Self::limits(ProcessSpec::new(&self.exe)).arg(source);
        if to == "ico" {
            // « > » : seules les images plus grandes que 256 px sont réduites, proportions gardées.
            spec = spec.args(["-resize", "256x256>"]);
            if let Some((nw, nh)) = task.source.as_ref().and_then(|s| s.width.zip(s.height)).and_then(|(w, h)| ico_shrink(w, h)) {
                notes.push(trf!("réduite à {nw}×{nh} (taille maximale d'une icône)", "reduced to {nw}×{nh} (largest icon size)"));
            }
        }
        if LOSSY_TARGETS.contains(&to) && to != "gif" {
            spec = spec.arg("-quality").arg(task.options.int_or("quality", 90).clamp(1, 100).to_string());
        }
        spec = spec.arg(format!("{cout}:{out_name}")).cwd(&stage.dir).timeout(ctx.timeout.or(Some(Duration::from_secs(900))));
        tool::run_ok(spec, ctx, &[], &mut |_| {})?;
        tool::deliver(&stage.path(&out_name), &task.output, "ImageMagick")?;
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        let Some(c) = coder(task.to.as_str()) else { return Verification::SignatureOnly };
        let name = format!("check.{}", tool::ext(task.to.as_str()));
        let stage = match Stage::new(ctx, "imagemagick-check").and_then(|st| st.input(output, &name).map(|_| st)) {
            Ok(st) => st,
            Err(e) => return Verification::Failed(e.to_string()),
        };
        let spec = Self::limits(ProcessSpec::new(&self.exe).arg("identify"))
            .args(["-format", "%w %h\n"])
            .arg(format!("{c}:{name}[0]"))
            .cwd(&stage.dir);
        let mut first = String::new();
        match crate::util::run_tool(
            spec,
            ctx,
            &mut |l| {
                if first.is_empty() {
                    first = l.trim().to_owned();
                }
            },
            &mut |_| {},
        ) {
            Ok(o) if o.success() && !first.is_empty() => {
                let wh = first.replace(' ', "×");
                Verification::Decoded(trf!("relu et décodé par ImageMagick ({wh})", "re-read and decoded by ImageMagick ({wh})"))
            }
            Ok(o) => Verification::Failed(trf!(
                "ImageMagick ne relit pas le résultat (code {:?})",
                "ImageMagick cannot re-read the result (code {:?})",
                o.code
            )),
            Err(e) => Verification::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_list_and_capabilities() {
        let text = "   Format  Module    Mode  Description\n---\n     APNG  rw+   Animated PNG\n      PNG* PNG       rw-   Portable\n     HEIC  r--   High Efficiency\n      PDF  rw+   Portable Document\n      XCF* r--   GIMP\n";
        let f = parse_formats(text);
        assert_eq!(f["PNG"], (true, true));
        assert_eq!(f["HEIC"], (true, false));
        let im = ImageMagick::with_formats(PathBuf::from("/x"), None, EngineOrigin::System, &f);
        let has = |a: &str, b: &str| im.caps.iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(
            has("heic", "png")
                && has("xcf", "png")
                && has("apng", "png")
                && !has("png", "apng")
                && !has("png", "heic")
                && !has("png", "pdf")
        );
        assert_eq!(raster_fidelity("png", "jpeg"), Fidelity::Lossy);
        assert_eq!(raster_fidelity("cr2", "tiff"), Fidelity::Lossy);
        assert_eq!(raster_fidelity("psd", "png"), Fidelity::Depends);
    }

    #[test]
    fn icons_are_capped_at_256() {
        assert_eq!(ico_shrink(256, 100), None);
        assert_eq!(ico_shrink(1024, 512), Some((256, 128)));
        assert_eq!(ico_shrink(300, 3000), Some((26, 256)));
        let f = parse_formats(
            "      PNG* PNG       rw-   Portable
      ICO* ICO       rw+   Icon
",
        );
        let im = ImageMagick::with_formats(PathBuf::from("/x"), None, EngineOrigin::System, &f);
        let src = chud_core::SourceInfo { width: Some(1024), height: Some(512), ..Default::default() };
        let opts = chud_core::OptionValues::new();
        let (png, ico): (FormatId, FormatId) = ("png".into(), "ico".into());
        let a = im.assess(&chud_core::StepQuery { from: &png, to: &ico, source: Some(&src), options: &opts });
        assert_eq!(a.fidelity, Fidelity::Lossy);
        assert!(a.notes.iter().any(|n| n.contains("256×128")), "{:?}", a.notes);
    }
}
