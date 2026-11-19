//! Adaptateur Pandoc : documents texte et balisés (Markdown, HTML, DOCX, ODT, EPUB, LaTeX…).
//!
//! - Toujours lancé avec `--sandbox` : Pandoc ne lit aucun fichier en dehors de la source et
//!   n'accède jamais au réseau (images distantes, `\include`, feuilles de style…).
//! - Les formats lus et écrits sont demandés à l'exécutable au démarrage.
//! - Certaines compilations (paquets Debian, Ubuntu, Fedora) n'intègrent pas leurs fichiers de
//!   données : en `--sandbox`, elles ne savent plus écrire DOCX, ODT ni EPUB (« Could not find data
//!   file »). Ces sorties sont alors essayées au démarrage et retirées si elles échouent, plutôt
//!   que de lever le bac à sable (LibreOffice et Calibre les produisent aussi).
//! - Pandoc reconstruit le document à partir de sa structure : les styles et la mise en page
//!   fine ne sont pas conservés (LibreOffice est préféré entre formats bureautiques).
//! - Vérification : le résultat est relu par Pandoc et doit contenir du texte.

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, StepOutput, StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// (format du catalogue, lecteur Pandoc, écrivain Pandoc)
const MAP: &[(&str, Option<&str>, Option<&str>)] = &[
    ("md", Some("markdown"), Some("markdown")),
    ("html", Some("html"), Some("html")),
    ("docx", Some("docx"), Some("docx")),
    ("odt", Some("odt"), Some("odt")),
    ("rtf", Some("rtf"), Some("rtf")),
    ("latex", Some("latex"), Some("latex")),
    ("rst", Some("rst"), Some("rst")),
    ("org", Some("org"), Some("org")),
    ("asciidoc", Some("asciidoc"), Some("asciidoc")),
    ("typst", Some("typst"), Some("typst")),
    ("epub", Some("epub"), Some("epub3")),
    ("ipynb", Some("ipynb"), Some("ipynb")),
    ("fb2", Some("fb2"), Some("fb2")),
    ("txt", None, Some("plain")),
];

fn reader(id: &str) -> Option<&'static str> {
    MAP.iter().find(|m| m.0 == id).and_then(|m| m.1)
}

fn writer(id: &str) -> Option<&'static str> {
    MAP.iter().find(|m| m.0 == id).and_then(|m| m.2)
}

/// Formats qui embarquent leurs images (le reste ne garde que des liens vers elles).
fn embeds_images(id: &str) -> bool {
    matches!(id, "docx" | "odt" | "epub" | "html" | "rtf" | "fb2" | "ipynb")
}

fn preserved(to: &str) -> FeatureSet {
    match to {
        "txt" => FeatureSet::new(),
        t if embeds_images(t) => features(&["rich_text", "tables", "images"]),
        _ => features(&["rich_text", "tables"]),
    }
}

pub struct Pandoc {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
    readers: HashSet<String>,
}

/// Essai réel d'une sortie en `--sandbox` sur un petit document (dossier temporaire retiré).
fn sandbox_can_write(exe: &Path, writer: &str) -> bool {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("chud-pandoc-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(dir.join("in.md"), "# Essai\n\nTexte.\n").is_err() {
        return true; // essai impossible : la conversion dira elle-même si elle échoue
    }
    let spec = ProcessSpec::new(exe)
        .args(["--sandbox", "-f", "markdown", "-t", writer, "-o", "out.bin", "in.md"])
        .cwd(&dir)
        .timeout(Some(std::time::Duration::from_secs(30)));
    let ok = chud_process::run_capture(&spec, 64 * 1024).is_ok_and(|o| o.success())
        && std::fs::metadata(dir.join("out.bin")).is_ok_and(|m| m.len() > 0);
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

fn names(text: &str) -> HashSet<String> {
    text.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect()
}

impl Pandoc {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let list = |arg: &str| -> Result<HashSet<String>, String> {
            let out = tool::query(&exe, &[arg])?;
            if !out.success() {
                return Err(trf!("pandoc {arg} a échoué", "pandoc {arg} failed"));
            }
            Ok(names(&out.stdout_text()))
        };
        let (inputs, mut outputs) = (list("--list-input-formats")?, list("--list-output-formats")?);
        // DOCX réussi : les fichiers de données sont intégrés, rien d'autre à essayer.
        if outputs.contains("docx") && !sandbox_can_write(&exe, "docx") {
            outputs.remove("docx");
            for w in ["odt", "epub3"] {
                if outputs.contains(w) && !sandbox_can_write(&exe, w) {
                    outputs.remove(w);
                }
            }
        }
        Ok(Self::with_formats(exe, version, origin, &inputs, &outputs))
    }

    pub fn with_formats(
        exe: PathBuf,
        version: Option<String>,
        origin: EngineOrigin,
        inputs: &HashSet<String>,
        outputs: &HashSet<String>,
    ) -> Self {
        let readable: Vec<&str> = MAP.iter().filter(|m| m.1.is_some_and(|r| inputs.contains(r))).map(|m| m.0).collect();
        let writable: Vec<&str> = MAP.iter().filter(|m| m.2.is_some_and(|w| outputs.contains(w))).map(|m| m.0).collect();
        let mut caps = Vec::new();
        for from in &readable {
            for to in &writable {
                if from == to {
                    continue;
                }
                let fidelity = if *to == "txt" { Fidelity::Lossy } else { Fidelity::Depends };
                caps.push(ConversionCap::new(from, to, fidelity, &preserved(to), 4));
            }
        }
        Self {
            info: tool::engine_info(
                "pandoc",
                "Pandoc",
                version,
                origin,
                &exe,
                "GPL-2.0-or-later",
                "https://pandoc.org",
                tr!(
                    "Documents texte et balisés : Markdown, HTML, DOCX, ODT, EPUB, LaTeX… (structure conservée, styles non).",
                    "Text and markup documents: Markdown, HTML, DOCX, ODT, EPUB, LaTeX… (structure kept, styles not)."
                ),
            ),
            exe,
            caps,
            readers: readable.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn base(&self) -> ProcessSpec {
        ProcessSpec::new(&self.exe).arg("--sandbox")
    }
}

impl Engine for Pandoc {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let (from, to) = (task.from.as_str(), task.to.as_str());
        let (Some(r), Some(w)) = (reader(from), writer(to)) else {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        };
        let stage = Stage::new(ctx, "pandoc")?;
        let out = stage.path(&format!("out.{}", tool::ext(to)));
        let mut spec = self.base().args(["-f", r, "-t", w]);
        if matches!(to, "html" | "latex" | "rtf" | "typst") {
            spec = spec.arg("-s");
        }
        let mut notes = Vec::new();
        if to == "html" {
            // Images de la source (DOCX, ODT, EPUB) intégrées au fichier HTML.
            spec = spec.arg("--embed-resources");
        }
        if !embeds_images(to) && embeds_images(from) {
            notes.push(
                tr!(
                    "les images éventuelles de la source ne sont pas reprises : ce format ne garde que des liens vers des fichiers",
                    "any images in the source are not kept: this format only keeps links to files"
                )
                .to_owned(),
            );
        }
        spec = spec.arg("-o").arg(&out).arg(&task.input);
        let mut warnings = 0;
        tool::run_ok(spec, ctx, &[], &mut |l| {
            if l.contains("[WARNING]") {
                warnings += 1;
            }
        })?;
        if warnings > 0 {
            notes.push(trf!(
                "Pandoc a signalé {warnings} avertissement(s) (éléments non convertibles ignorés)",
                "Pandoc reported {warnings} warning(s) (unconvertible items skipped)"
            ));
        }
        tool::deliver(&out, &task.output, "Pandoc")?;
        tool::report_fraction(ctx, 1.0);
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        let to = task.to.as_str();
        if to == "txt" {
            return match tool::check_text(output) {
                Ok(s) => Verification::Structural(s),
                Err(e) => Verification::Failed(e),
            };
        }
        if matches!(to, "docx" | "odt" | "epub")
            && let Err(e) = tool::check_zip(output, &[])
        {
            return Verification::Failed(e);
        }
        let Some(r) = reader(to).filter(|_| self.readers.contains(to)) else {
            return Verification::SignatureOnly;
        };
        let spec = self.base().args(["-f", r, "-t", "plain"]).arg(output).capture_stdout(64 * 1024 * 1024);
        let mut text = String::new();
        match crate::util::run_tool(
            spec,
            ctx,
            &mut |l| {
                text.push_str(l);
                text.push('\n');
            },
            &mut |_| {},
        ) {
            Ok(o) if o.success() => {
                let words = text.split_whitespace().count();
                if words == 0 {
                    Verification::Failed(
                        tr!("le document produit ne contient aucun texte", "the produced document contains no text").into(),
                    )
                } else {
                    Verification::Decoded(trf!("relu par Pandoc : {words} mots", "re-read by Pandoc: {words} words"))
                }
            }
            Ok(o) => Verification::Failed(trf!(
                "Pandoc ne relit pas le résultat (code {:?})",
                "Pandoc cannot re-read the result (code {:?})",
                o.code
            )),
            Err(e) => Verification::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> Pandoc {
        let all = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
        Pandoc::with_formats(
            PathBuf::from("/x/pandoc"),
            None,
            EngineOrigin::System,
            &all(&["markdown", "html", "docx", "odt", "epub"]),
            &all(&["markdown", "html", "docx", "plain", "epub3"]),
        )
    }

    #[test]
    fn capabilities_follow_the_executable() {
        let p = fake();
        let has = |a: &str, b: &str| p.caps.iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(has("md", "docx") && has("docx", "md") && has("odt", "epub") && has("docx", "txt"));
        assert!(!has("txt", "md"), "Pandoc ne lit pas le texte brut comme format");
        assert!(!has("md", "odt"), "écrivain absent de la liste");
        assert!(!has("md", "md"));
        let txt = p.caps.iter().find(|c| c.to.as_str() == "txt").unwrap();
        assert_eq!(txt.fidelity, Fidelity::Lossy);
        let md = p.caps.iter().find(|c| c.to.as_str() == "md").unwrap();
        assert!(!md.preserves.contains(&chud_core::Feature::new("images")), "Markdown ne garde que des liens");
    }
}
