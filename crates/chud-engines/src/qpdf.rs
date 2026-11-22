//! Adaptateur qpdf : réécriture sans perte d'un PDF (réparation, linéarisation pour l'affichage
//! web, compression de la structure, extraction de pages) et analyse des PDF. Fusion, division et
//! rotation : voir `qpdf_ops.rs`.
//!
//! - Le contenu des pages n'est jamais recompressé ni modifié : seule la structure est réécrite.
//! - Un PDF protégé par mot de passe est refusé (aucun mot de passe n'est demandé ni deviné).
//! - Vérification : `qpdf --check` sur le résultat, et nombre de pages attendu.

use crate::tool;
use chud_core::feature::features;
use chud_core::operation::{PDF_MERGE, PDF_ROTATE, PDF_SPLIT};
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, Fidelity, FormatId, OperationReport, OperationTask,
    OptionSpec, OptionValue, OptionValues, SourceInfo, StepOutput, StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Qpdf {
    info: EngineInfo,
    pub(crate) exe: PathBuf,
    caps: Vec<ConversionCap>,
    /// Analyses déjà faites (opérations sur les pages).
    pub(crate) docs: crate::qpdf_ops::Cache,
}

/// Plage de pages au format qpdf (« 1-3,5,8-z »), sans rien d'autre.
pub(crate) fn valid_range(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.split(',').all(|part| {
            let part = part.trim();
            !part.is_empty()
                && part.split('-').count() <= 2
                && part.split('-').all(|n| n == "z" || (!n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()) && n != "0"))
        })
}

/// Plus grand numéro de page écrit dans la plage (« z », la dernière, n'en est pas un).
pub(crate) fn highest_page(range: &str) -> u32 {
    range.split([',', '-']).filter_map(|n| n.trim().parse().ok()).max().unwrap_or(0)
}

impl Qpdf {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        let keep = features(&[
            "multipage",
            "vector",
            "text_layer",
            "images",
            "fonts_embedded",
            "forms",
            "bookmarks",
            "metadata",
            "comments",
            "cmyk",
            "icc",
            "encryption",
        ]);
        Ok(Self {
            info: tool::engine_info(
                "qpdf",
                "qpdf",
                version,
                origin,
                &exe,
                "Apache-2.0",
                "https://qpdf.sourceforge.io",
                tr!(
                    "PDF sans perte : réparation, optimisation pour le web, extraction de pages, analyse.",
                    "Lossless PDF: repair, web optimization, page extraction, analysis."
                ),
            ),
            caps: vec![ConversionCap::new("pdf", "pdf", Fidelity::Lossless, &keep, 2)],
            exe,
            docs: Default::default(),
        })
    }

    pub(crate) fn pages(&self, path: &Path) -> Option<u32> {
        let spec = ProcessSpec::new(&self.exe).arg("--show-npages").arg(path).timeout(Some(Duration::from_secs(60)));
        let out = chud_process::run_capture(&spec, 4096).ok()?;
        if !out.success() {
            return None;
        }
        out.stdout_text().trim().parse().ok()
    }

    fn encrypted(&self, path: &Path) -> Option<bool> {
        let spec = ProcessSpec::new(&self.exe).arg("--is-encrypted").arg(path).timeout(Some(Duration::from_secs(60)));
        match chud_process::run_capture(&spec, 4096).ok()?.code {
            Some(0) => Some(true),
            Some(2) => Some(false),
            _ => None,
        }
    }
}

impl Engine for Qpdf {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, _to: &FormatId) -> Vec<OptionSpec> {
        vec![
            OptionSpec::bool("linearize", tr!("Optimiser pour l'affichage web (linéariser)", "Optimize for web viewing (linearize)"), true),
            OptionSpec::bool("compress", tr!("Compacter la structure (fichier plus petit)", "Compact the structure (smaller file)"), true),
            OptionSpec::text("pages", tr!("Pages à garder (ex. 1-3,5) ; vide : toutes", "Pages to keep (e.g. 1-3,5); empty: all"), 200, "")
                .help(tr!("« z » désigne la dernière page", "“z” means the last page")),
        ]
    }

    fn can_probe(&self, format: &FormatId) -> bool {
        format.as_str() == "pdf"
    }

    fn probe(&self, path: &Path, _format: &FormatId) -> Option<SourceInfo> {
        let encrypted = self.encrypted(path)?;
        let mut summary = Vec::new();
        match self.pages(path) {
            Some(n) => summary.push((tr!("Pages", "Pages").to_string(), n.to_string())),
            None if encrypted => {
                summary.push((tr!("Accès", "Access").to_string(), tr!("protégé par mot de passe", "password-protected").to_string()))
            }
            None => return None,
        }
        summary.push((tr!("Chiffrement", "Encryption").into(), if encrypted { tr!("oui", "yes") } else { tr!("non", "no") }.into()));
        Some(SourceInfo { summary, ..Default::default() })
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        if (task.from.as_str(), task.to.as_str()) != ("pdf", "pdf") {
            return Err(EngineError::Unsupported(format!("{} → {}", task.from, task.to)));
        }
        let range = match task.options.get("pages") {
            Some(OptionValue::Text(t)) if !t.trim().is_empty() => {
                let t = t.replace(' ', "");
                if !valid_range(&t) {
                    return Err(EngineError::InvalidOption {
                        key: "pages".into(),
                        message: tr!("plage invalide (exemples : 1-3, 2,4,6, 5-z)", "invalid range (examples: 1-3, 2,4,6, 5-z)").into(),
                    });
                }
                // Vérifiée avant de convertir : une page absente donne un refus clair, pas
                // l'erreur brute de qpdf.
                if let Some(n) = self.pages(&task.input)
                    && highest_page(&t) > n
                {
                    return Err(EngineError::InvalidOption {
                        key: "pages".into(),
                        message: trf!("le document n'a que {n} page(s)", "the document only has {n} page(s)"),
                    });
                }
                Some(t)
            }
            _ => None,
        };
        let stage = tool::Stage::new(ctx, "qpdf")?;
        // Nom simple dans le dossier de travail : qpdf lit « @fichier » comme une liste d'arguments.
        stage.input(&task.input, "input.pdf")?;
        let out = stage.path("out.pdf");
        let mut spec = ProcessSpec::new(&self.exe);
        if task.options.bool_or("linearize", true) {
            spec = spec.arg("--linearize");
        }
        if task.options.bool_or("compress", true) {
            spec = spec.args(["--object-streams=generate", "--compress-streams=y"]);
        }
        spec = spec.arg("input.pdf");
        if let Some(r) = &range {
            spec = spec.args(["--pages", "."]).arg(r).arg("--");
        }
        spec = spec.arg("out.pdf").cwd(&stage.dir);
        let mut msgs = Vec::new();
        let res = tool::run_ok(spec, ctx, &[3], &mut |l| msgs.push(l.to_owned()));
        if let Err(EngineError::Process { detail, .. }) = &res
            && detail.to_lowercase().contains("password")
        {
            return Err(EngineError::Refused(tr!("PDF protégé par mot de passe", "password-protected PDF").into()));
        }
        let o = res?;
        tool::deliver(&out, &task.output, "qpdf")?;
        let mut notes =
            vec![tr!("structure réécrite, contenu des pages inchangé", "structure rewritten, page content unchanged").to_owned()];
        if o.code == Some(3) {
            notes.push(tr!("qpdf a réparé des erreurs du fichier d'origine", "qpdf repaired errors in the original file").to_owned());
        }
        if let Some(r) = range {
            notes.push(trf!("pages gardées : {r}", "pages kept: {r}"));
        }
        Ok(StepOutput { notes })
    }

    fn operations(&self) -> &[&'static str] {
        &[PDF_MERGE, PDF_SPLIT, PDF_ROTATE]
    }

    fn operation_options(&self, operation: &str) -> Vec<OptionSpec> {
        self.op_options(operation)
    }

    fn operation_target(&self, operation: &str, _options: &OptionValues) -> Result<FormatId, EngineError> {
        match operation {
            PDF_MERGE | PDF_ROTATE => Ok(FormatId::new("pdf")),
            PDF_SPLIT => Ok(FormatId::new("directory")),
            o => Err(EngineError::Unsupported(o.into())),
        }
    }

    fn assess_operation(&self, operation: &str, inputs: &[PathBuf], options: &OptionValues) -> Result<OperationReport, EngineError> {
        self.op_assess(operation, inputs, options)
    }

    fn execute_operation(&self, task: &OperationTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        self.op_execute(task, ctx)
    }

    fn verify_operation(&self, task: &OperationTask, ctx: &ExecContext) -> Verification {
        self.op_verify(task, ctx)
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        let spec = ProcessSpec::new(&self.exe).arg("--check").arg(output);
        let last = std::cell::RefCell::new(String::new());
        let r = crate::util::run_tool(spec, ctx, &mut |l| *last.borrow_mut() = l.to_owned(), &mut |l| *last.borrow_mut() = l.to_owned());
        let last = last.into_inner();
        match r {
            Ok(o) if matches!(o.code, Some(0) | Some(3)) => {
                let n = self.pages(output);
                let expected = (task.options.str_or("pages", "").trim().is_empty()).then(|| self.pages(&task.input)).flatten();
                match (n, expected) {
                    (Some(a), Some(b)) if a != b => Verification::Failed(trf!("{a} pages au lieu de {b}", "{a} pages instead of {b}")),
                    (Some(a), _) => Verification::Structural(trf!(
                        "qpdf --check : structure valide, {a} pages",
                        "qpdf --check: valid structure, {a} pages"
                    )),
                    (None, _) => Verification::Failed(tr!("nombre de pages illisible", "unreadable page count").into()),
                }
            }
            Ok(o) => Verification::Failed(trf!(
                "qpdf --check a trouvé des erreurs (code {:?}) : {last}",
                "qpdf --check found errors (code {:?}): {last}",
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
    fn ranges_are_strict() {
        for ok in ["1", "1-3", "1-3,5", "5-z", "z", "2,4,6"] {
            assert!(valid_range(ok), "{ok}");
        }
        for bad in ["", "0", "1--3", "a", "1;rm", "--x", "1-3-5", "-1"] {
            assert!(!valid_range(bad), "{bad}");
        }
        assert_eq!(highest_page("2-3"), 3);
        assert_eq!(highest_page("1,8-z"), 8);
        assert_eq!(highest_page("z"), 0);
    }
}
