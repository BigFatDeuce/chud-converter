//! Adaptateur Tesseract : reconnaissance de texte (OCR) dans les images, vers du texte brut ou
//! un PDF dont le texte est sélectionnable et cherchable.
//!
//! - Les langues proposées sont celles réellement installées (`--list-langs`).
//! - La reconnaissance n'est jamais garantie : le résultat est marqué « avec perte » et une
//!   image sans texte reconnu est signalée comme un échec, pas comme une réussite vide.

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

const INPUTS: &[&str] = &["png", "jpeg", "tiff", "bmp", "gif", "webp", "pnm", "jp2"];

/// Formats lus d'après la liste des bibliothèques affichée par `tesseract --version` : une
/// compilation de Leptonica sans giflib (Ubuntu) ne lit pas le GIF. BMP et PNM sont toujours lus ;
/// si la liste est introuvable, tous les formats sont gardés (la conversion dira si elle échoue).
fn readable_inputs(version_text: &str) -> Vec<&'static str> {
    let t = version_text.to_lowercase();
    if !t.contains("libpng") && !t.contains("libjpeg") {
        return INPUTS.to_vec();
    }
    INPUTS
        .iter()
        .copied()
        .filter(|f| match *f {
            "png" => t.contains("libpng"),
            "jpeg" => t.contains("libjpeg"),
            "tiff" => t.contains("libtiff"),
            "gif" => t.contains("libgif") || t.contains("giflib"),
            "webp" => t.contains("libwebp"),
            "jp2" => t.contains("libopenjp2"),
            _ => true,
        })
        .collect()
}

pub struct Tesseract {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
    langs: Vec<String>,
}

fn lang_label(code: &str) -> String {
    let name = match code {
        "fra" => tr!("français", "French"),
        "eng" => tr!("anglais", "English"),
        "deu" => tr!("allemand", "German"),
        "spa" => tr!("espagnol", "Spanish"),
        "ita" => tr!("italien", "Italian"),
        "por" => tr!("portugais", "Portuguese"),
        "nld" => tr!("néerlandais", "Dutch"),
        _ => code,
    };
    name.to_owned()
}

/// Langues lues dans la sortie de `--list-langs` (la première ligne est un titre).
fn parse_langs(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| !l.contains("List of available languages"))
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "osd" && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .map(str::to_owned)
        .collect()
}

impl Tesseract {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let out = tool::query(&exe, &["--list-langs"])?;
        let langs = parse_langs(&format!("{}\n{}", out.stdout_text(), out.stderr_tail));
        if langs.is_empty() {
            return Err(tr!("aucune langue de reconnaissance installée", "no recognition language installed").into());
        }
        let inputs = match tool::query(&exe, &["--version"]) {
            Ok(v) => readable_inputs(&format!("{}\n{}", v.stdout_text(), v.stderr_tail)),
            Err(_) => INPUTS.to_vec(),
        };
        Ok(Self::with_inputs(exe, version, origin, langs, &inputs))
    }

    pub fn with_langs(exe: PathBuf, version: Option<String>, origin: EngineOrigin, langs: Vec<String>) -> Self {
        Self::with_inputs(exe, version, origin, langs, INPUTS)
    }

    fn with_inputs(exe: PathBuf, version: Option<String>, origin: EngineOrigin, langs: Vec<String>, inputs: &[&str]) -> Self {
        let mut caps = Vec::new();
        for from in inputs {
            caps.push(ConversionCap::new(from, "txt", Fidelity::Lossy, &FeatureSet::new(), 6));
            caps.push(ConversionCap::new(from, "pdf", Fidelity::Lossy, &features(&["images", "text_layer", "multipage"]), 6));
        }
        Self {
            info: tool::engine_info(
                "tesseract",
                "Tesseract OCR",
                version,
                origin,
                &exe,
                "Apache-2.0",
                "https://github.com/tesseract-ocr/tesseract",
                tr!(
                    "Reconnaissance de texte (OCR) : images vers texte ou PDF cherchable.",
                    "Text recognition (OCR): images to text or searchable PDF."
                ),
            ),
            exe,
            caps,
            langs,
        }
    }

    fn default_lang(&self) -> String {
        let has = |l: &str| self.langs.iter().any(|x| x == l);
        match (has("fra"), has("eng")) {
            (true, true) => "fra+eng".into(),
            (true, false) => "fra".into(),
            (false, true) => "eng".into(),
            _ => self.langs[0].clone(),
        }
    }

    fn choices(&self) -> Vec<(String, String)> {
        let mut c = Vec::new();
        if self.langs.iter().any(|l| l == "fra") && self.langs.iter().any(|l| l == "eng") {
            c.push(("fra+eng".to_owned(), tr!("français et anglais", "French and English").to_owned()));
        }
        c.extend(self.langs.iter().map(|l| (l.clone(), lang_label(l))));
        c
    }
}

impl Engine for Tesseract {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, _to: &FormatId) -> Vec<OptionSpec> {
        let choices = self.choices();
        let refs: Vec<(&str, &str)> = choices.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        vec![OptionSpec::choice("lang", tr!("Langue du texte", "Text language"), &refs, &self.default_lang())]
    }

    fn weight(&self, _from: &FormatId, _to: &FormatId) -> u32 {
        2
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let (from, to) = (task.from.as_str(), task.to.as_str());
        if !self.caps.iter().any(|c| c.from.as_str() == from) || !matches!(to, "txt" | "pdf") {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        }
        let default = self.default_lang();
        let lang = task.options.str_or("lang", &default).to_owned();
        let valid = !lang.is_empty() && lang.split('+').all(|l| self.langs.iter().any(|x| x == l));
        if !valid {
            return Err(EngineError::InvalidOption {
                key: "lang".into(),
                message: trf!("langue non installée : {lang}", "language not installed: {lang}"),
            });
        }
        let stage = Stage::new(ctx, "tesseract")?;
        let in_name = format!("input.{}", tool::ext(from));
        stage.input(&task.input, &in_name)?;
        let spec = ProcessSpec::new(&self.exe)
            .arg(&in_name)
            .arg("out")
            .args(["-l", &lang])
            .arg(if to == "pdf" { "pdf" } else { "txt" })
            .env("OMP_THREAD_LIMIT", "2")
            .cwd(&stage.dir)
            .timeout(ctx.timeout.or(Some(Duration::from_secs(1800))));
        tool::run_ok(spec, ctx, &[], &mut |_| {})?;
        let produced = stage.path(&format!("out.{to}"));
        if to == "txt" && std::fs::read_to_string(&produced).is_ok_and(|t| t.trim().is_empty()) {
            return Err(EngineError::Other(tr!("aucun texte reconnu dans l'image", "no text recognized in the image").into()));
        }
        tool::deliver(&produced, &task.output, "Tesseract")?;
        Ok(StepOutput {
            notes: vec![trf!(
                "texte reconnu automatiquement ({lang}) : relisez-le, des erreurs sont possibles",
                "automatically recognized text ({lang}): proofread it, mistakes are possible"
            )],
        })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        let r = if task.to.as_str() == "pdf" {
            tool::check_pdf(output)
        } else {
            tool::check_text(output).map_err(|_| tr!("aucun texte reconnu dans l'image", "no text recognized in the image").to_owned())
        };
        match r {
            Ok(s) => Verification::Structural(s),
            Err(e) => Verification::Failed(e),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn inputs_follow_leptonica_libraries() {
        let ubuntu = "tesseract 5.3.4\n leptonica-1.82.0\n  libjpeg 8d (libjpeg-turbo 2.1.5) : libpng 1.6.43 : libtiff 4.5.1 : zlib 1.3 : libwebp 1.3.2 : libopenjp2 2.5.0\n";
        let ins = readable_inputs(ubuntu);
        assert!(ins.contains(&"png") && ins.contains(&"webp") && ins.contains(&"bmp") && !ins.contains(&"gif"));
        let full = "leptonica-1.85.0\n  libgif 5.2.2 : libjpeg 8d : libpng 1.6.44 : libtiff 4.7.0 : zlib 1.3.1 : libwebp 1.4.0 : libopenjp2 2.5.2\n";
        assert_eq!(readable_inputs(full).len(), INPUTS.len());
        assert_eq!(readable_inputs("rien").len(), INPUTS.len());
    }

    use super::*;

    #[test]
    fn languages() {
        let text = "List of available languages in \"C:\\x/tessdata/\" (3):\neng\nfra\nosd\n";
        assert_eq!(parse_langs(text), ["eng", "fra"]);
        let t = Tesseract::with_langs(PathBuf::from("/x"), None, EngineOrigin::System, vec!["eng".into(), "fra".into()]);
        assert_eq!(t.default_lang(), "fra+eng");
        assert_eq!(t.choices()[0].0, "fra+eng");
    }
}
