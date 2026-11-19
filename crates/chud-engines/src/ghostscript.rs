//! Adaptateur Ghostscript : rendu des PDF et PostScript en images, PostScript ⇄ PDF.
//!
//! - Toujours `-dSAFER` (le document ne peut ni lire ni écrire de fichiers ni lancer de
//!   commande), `-dBATCH -dNOPAUSE` (aucune attente de saisie).
//! - Lancé dans le dossier de travail avec des noms simples : un `%` dans un nom de fichier ne
//!   peut pas être pris pour un motif de numérotation de pages.
//! - Vérification : les images produites sont décodées entièrement ; les PDF et PostScript
//!   produits sont rendus à nouveau par Ghostscript (sans affichage).

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, FormatId, OptionSpec, StepOutput,
    StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::time::Duration;

const VECTOR_IN: &[&str] = &["pdf", "ps", "eps", "ai"];
const RASTER_OUT: &[&str] = &["png", "jpeg", "tiff"];

pub struct Ghostscript {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
}

fn vector_keep() -> FeatureSet {
    features(&["multipage", "vector", "text_layer", "images", "fonts_embedded", "cmyk"])
}

impl Ghostscript {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        let mut caps = Vec::new();
        for from in VECTOR_IN {
            for to in RASTER_OUT {
                // Rendu en pixels : le texte sélectionnable et le vectoriel disparaissent.
                let keep = if *to == "tiff" { features(&["multipage", "cmyk"]) } else { FeatureSet::new() };
                caps.push(ConversionCap::new(from, to, Fidelity::Lossy, &keep, 3));
            }
        }
        for from in ["ps", "eps", "ai"] {
            caps.push(ConversionCap::new(from, "pdf", Fidelity::Depends, &vector_keep(), 2));
        }
        caps.push(ConversionCap::new("pdf", "ps", Fidelity::Depends, &vector_keep(), 2));
        Ok(Self {
            info: tool::engine_info(
                "ghostscript",
                "Ghostscript",
                version,
                origin,
                &exe,
                "AGPL-3.0-or-later",
                "https://www.ghostscript.com",
                tr!(
                    "PDF et PostScript : rendu en images, PostScript vers PDF.",
                    "PDF and PostScript: rendering to images, PostScript to PDF."
                ),
            ),
            exe,
            caps,
        })
    }

    fn base(&self) -> ProcessSpec {
        ProcessSpec::new(&self.exe).args(["-dSAFER", "-dBATCH", "-dNOPAUSE"])
    }

    /// Rendu complet sans affichage (`nullpage`) : prouve que le document se lit en entier.
    fn render_check(&self, file: &Path, ctx: &ExecContext) -> Verification {
        let Some(dir) = file.parent() else { return Verification::SignatureOnly };
        let Some(name) = file.file_name() else { return Verification::SignatureOnly };
        let spec = self.base().args(["-q", "-sDEVICE=nullpage"]).arg(name).cwd(dir);
        let mut err = String::new();
        match crate::util::run_tool(spec, ctx, &mut |_| {}, &mut |l| err = l.to_owned()) {
            Ok(o) if o.success() && !err.to_lowercase().contains("error") => {
                Verification::Decoded(tr!("entièrement rendu par Ghostscript", "fully rendered by Ghostscript").into())
            }
            Ok(o) => Verification::Failed(trf!("rendu impossible (code {:?}) : {err}", "rendering failed (code {:?}): {err}", o.code)),
            Err(e) => Verification::Failed(e.to_string()),
        }
    }
}

impl Engine for Ghostscript {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        let mut o = Vec::new();
        if RASTER_OUT.contains(&to.as_str()) {
            o.push(OptionSpec::int("dpi", tr!("Résolution (points par pouce)", "Resolution (dots per inch)"), 36, 1200, 150));
            if to.as_str() != "tiff" {
                o.push(OptionSpec::int("page", tr!("Page à convertir", "Page to convert"), 1, 100_000, 1).help(tr!(
                    "PNG et JPEG ne contiennent qu'une image ; le TIFF garde toutes les pages",
                    "PNG and JPEG hold a single image; TIFF keeps every page"
                )));
            }
            if to.as_str() == "jpeg" {
                o.push(OptionSpec::int("quality", tr!("Qualité JPEG", "JPEG quality"), 1, 100, 90));
            }
        }
        o
    }

    fn weight(&self, _from: &FormatId, _to: &FormatId) -> u32 {
        2
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let (from, to) = (task.from.as_str(), task.to.as_str());
        if !self.caps.iter().any(|c| c.from.as_str() == from && c.to.as_str() == to) {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        }
        let stage = Stage::new(ctx, "ghostscript")?;
        let in_name = format!("input.{}", tool::ext(from));
        stage.input(&task.input, &in_name)?;
        let out_name = format!("out.{}", tool::ext(to));
        let mut spec = self.base();
        let mut notes = Vec::new();
        let mut page = None;
        match to {
            "png" | "jpeg" | "tiff" => {
                let dpi = task.options.int_or("dpi", 150).clamp(36, 1200);
                let device = match to {
                    "png" => "png16m",
                    "jpeg" => "jpeg",
                    _ => "tiff24nc",
                };
                spec = spec.arg(format!("-sDEVICE={device}")).arg(format!("-r{dpi}")).args(["-dTextAlphaBits=4", "-dGraphicsAlphaBits=4"]);
                if to == "tiff" {
                    spec = spec.arg("-sCompression=lzw");
                } else {
                    let p = task.options.int_or("page", 1).max(1);
                    page = Some(p);
                    spec = spec.arg(format!("-dFirstPage={p}")).arg(format!("-dLastPage={p}"));
                    if p == 1 {
                        notes.push(
                            tr!("première page seulement (option « Page à convertir »)", "first page only (“Page to convert” option)")
                                .to_owned(),
                        );
                    }
                }
                if to == "jpeg" {
                    spec = spec.arg(format!("-dJPEGQ={}", task.options.int_or("quality", 90).clamp(1, 100)));
                }
            }
            "pdf" => spec = spec.arg("-sDEVICE=pdfwrite"),
            "ps" => spec = spec.arg("-sDEVICE=ps2write"),
            _ => return Err(EngineError::Unsupported(to.into())),
        }
        if from == "eps" {
            spec = spec.arg("-dEPSCrop");
        }
        spec = spec
            .arg(format!("-sOutputFile={out_name}"))
            .arg(&in_name)
            .cwd(&stage.dir)
            .timeout(ctx.timeout.or(Some(Duration::from_secs(1800))));
        let mut total: Option<u32> = None;
        tool::run_ok(spec, ctx, &[], &mut |l| {
            // « Processing pages 1 through 12. » puis « Page 3 ».
            if let Some(rest) = l.strip_prefix("Processing pages ") {
                total = rest.split_whitespace().nth(2).and_then(|n| n.trim_end_matches('.').parse().ok());
            } else if let (Some(n), Some(t)) = (l.strip_prefix("Page ").and_then(|n| n.trim().parse::<u32>().ok()), total)
                && page.is_none()
            {
                tool::report_fraction(ctx, f64::from(n) / f64::from(t.max(1)));
            }
        })?;
        let produced = stage.path(&out_name);
        if !produced.is_file()
            && let (Some(p), Some(t)) = (page, total)
            && p > i64::from(t)
        {
            return Err(EngineError::InvalidOption {
                key: "page".into(),
                message: trf!("le document n'a que {t} page(s)", "the document only has {t} page(s)"),
            });
        }
        tool::deliver(&produced, &task.output, "Ghostscript")?;
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        match task.to.as_str() {
            "png" | "jpeg" | "tiff" => match tool::decode_image(output) {
                Some(Ok(s)) => Verification::Decoded(s),
                Some(Err(e)) => Verification::Failed(e),
                None => Verification::SignatureOnly,
            },
            "pdf" => match tool::check_pdf(output) {
                Ok(_) => self.render_check(output, ctx),
                Err(e) => Verification::Failed(e),
            },
            _ => self.render_check(output, ctx),
        }
    }
}
