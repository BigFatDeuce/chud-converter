//! Adaptateur Calibre (`ebook-convert`) : livres numériques EPUB, MOBI, AZW3, FB2, bandes
//! dessinées (CBZ, CBR, CB7), et documents vers livres numériques.
//!
//! - Réglages, cache et fichiers temporaires de Calibre dans le dossier de travail : la
//!   bibliothèque et la configuration Calibre de l'utilisateur ne sont jamais touchées.
//! - Les livres protégés par DRM sont refusés (Calibre ne les ouvre pas, et Chud Converter ne
//!   contourne aucune protection).
//! - Vérification : structure du fichier produit (EPUB/DOCX intacts, en-tête MOBI, PDF complet…).

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, FormatId, StepOutput, StepTask,
    Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::time::Duration;

const EBOOKS: &[&str] = &["epub", "mobi", "azw3", "fb2", "lit", "pdb", "lrf"];
const COMICS: &[&str] = &["cbz", "cbr", "cb7"];
// Pas d'ODT : le lecteur ODF de Calibre plante sur les images de LibreOffice (« KeyError:
// 'Bitmap_20_1' ») ; LibreOffice et Pandoc lisent l'ODT, Calibre prend le relais après eux.
const DOCS: &[&str] = &["docx", "rtf", "txt", "html"];
/// Formats écrits.
const WRITES: &[&str] = &["epub", "mobi", "azw3", "fb2", "pdf", "docx", "rtf", "txt"];
const EBOOK_OUT: &[&str] = &["epub", "mobi", "azw3", "fb2"];

fn preserved(to: &str) -> FeatureSet {
    match to {
        "epub" | "azw3" => features(&["rich_text", "styles", "images", "tables", "metadata", "cover_art", "toc"]),
        "mobi" | "fb2" => features(&["rich_text", "images", "metadata", "cover_art", "toc"]),
        "pdf" => features(&["images", "text_layer", "metadata", "multipage", "fonts_embedded"]),
        "docx" => features(&["rich_text", "styles", "images", "tables", "metadata"]),
        "rtf" => features(&["rich_text", "images"]),
        _ => FeatureSet::new(),
    }
}

pub struct Calibre {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
}

impl Calibre {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        let mut caps = Vec::new();
        let mut add = |from: &str, to: &str, fidelity: Fidelity| {
            if from != to {
                caps.push(ConversionCap::new(from, to, fidelity, &preserved(to), 3));
            }
        };
        for from in EBOOKS {
            for to in WRITES {
                add(from, to, if *to == "txt" { Fidelity::Lossy } else { Fidelity::Depends });
            }
        }
        for from in DOCS {
            for to in EBOOK_OUT {
                add(from, to, Fidelity::Depends);
            }
        }
        for from in COMICS {
            for to in ["epub", "mobi", "azw3", "pdf"] {
                add(from, to, Fidelity::Depends);
            }
        }
        // Un PDF n'a pas de structure de texte : la mise en page est perdue à la refusion.
        for to in ["epub", "mobi", "azw3", "docx", "txt"] {
            add("pdf", to, Fidelity::Lossy);
        }
        Ok(Self {
            info: tool::engine_info(
                "calibre",
                "Calibre (ebook-convert)",
                version,
                origin,
                &exe,
                "GPL-3.0-or-later",
                "https://calibre-ebook.com",
                tr!(
                    "Livres numériques : EPUB, MOBI, AZW3, FB2, bandes dessinées (sans DRM).",
                    "E-books: EPUB, MOBI, AZW3, FB2, comics (DRM-free)."
                ),
            ),
            exe,
            caps,
        })
    }
}

impl Engine for Calibre {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
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
        let stage = Stage::new(ctx, "calibre")?;
        // Calibre tire le titre d'un document qui n'en a pas du nom du fichier : on lui donne le
        // nom d'origine (caractères sûrs seulement), pas le nom de travail « input ».
        let stem = task.name.as_deref().map(safe_stem).filter(|s| !s.is_empty()).unwrap_or_else(|| "input".into());
        let input = stage.input(&task.input, &format!("{stem}.{}", tool::ext(from)))?;
        let out = stage.path(&format!("output.{}", tool::ext(to)));
        let mut spec = ProcessSpec::new(&self.exe).arg(&input).arg(&out);
        if COMICS.contains(&from) {
            // Images reprises telles quelles (sans redimensionnement ni retouche).
            spec = spec.arg("--no-process");
        }
        for (k, d) in [("CALIBRE_CONFIG_DIRECTORY", "config"), ("CALIBRE_CACHE_DIRECTORY", "cache"), ("CALIBRE_TEMP_DIR", "tmp")] {
            spec = spec.env(k, stage.subdir(d)?);
        }
        // Messages en anglais quelle que soit la langue du système : ils sont lus plus bas.
        spec = spec.env("CALIBRE_OVERRIDE_LANG", "en");
        spec = spec.cwd(&stage.dir).timeout(ctx.timeout.or(Some(Duration::from_secs(1800))));
        let (mut drm, mut saved) = (false, false);
        let r = tool::run_ok(spec, ctx, &[], &mut |l| {
            if l.contains("DRM") {
                drm = true;
            }
            if l.trim_start().starts_with("Output saved to") {
                saved = true;
            }
            if l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit())
                && let Some(f) = tool::percent_in(l.split_whitespace().next().unwrap_or(""))
            {
                tool::report_fraction(ctx, f);
            }
        });
        if drm {
            return Err(EngineError::Refused(
                tr!("ce livre est protégé par un DRM : il ne peut pas être converti", "this book is DRM-protected: it cannot be converted")
                    .into(),
            ));
        }
        let mut notes = Vec::new();
        match r {
            Ok(_) => {}
            // Sous Windows, Calibre plante parfois en se refermant (code 0xC0000005), une fois le
            // fichier écrit et annoncé : le résultat est gardé, et vérifié comme les autres.
            Err(EngineError::Process { .. }) if saved && crate::util::file_size(&out) > 0 => notes.push(
                tr!(
                    "Calibre s'est arrêté brutalement après avoir écrit le fichier : le résultat a été vérifié",
                    "Calibre crashed after writing the file: the result was checked"
                )
                .to_owned(),
            ),
            Err(e) => return Err(e),
        }
        if to == "txt" && std::fs::read(&out).is_ok_and(|b| b.iter().all(u8::is_ascii_whitespace)) {
            return Err(EngineError::Other(
                tr!(
                    "aucun texte à extraire : ce livre ne contient que des images (une bande dessinée, par exemple)",
                    "no text to extract: this book only contains images (a comic, for example)"
                )
                .into(),
            ));
        }
        tool::deliver(&out, &task.output, "Calibre")?;
        if from == "pdf" {
            notes.push(
                tr!(
                    "texte extrait du PDF et remis en forme : la mise en page d'origine n'est pas conservée",
                    "text extracted from the PDF and reflowed: the original layout is not kept"
                )
                .to_owned(),
            );
        }
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        let r = match task.to.as_str() {
            "epub" => tool::check_zip(output, &["mimetype", "META-INF/container.xml"]),
            "mobi" | "azw3" => mobi_header(output),
            "fb2" => std::fs::read(output).map_err(|e| e.to_string()).and_then(|b| {
                if String::from_utf8_lossy(&b[..b.len().min(4096)]).contains("<FictionBook") {
                    Ok(tr!("document FictionBook reconnu", "FictionBook document recognized").into())
                } else {
                    Err(tr!("élément FictionBook absent", "FictionBook element missing").into())
                }
            }),
            _ => return crate::libreoffice::verify_document(output, task),
        };
        match r {
            Ok(s) => Verification::Structural(s),
            Err(e) => Verification::Failed(e),
        }
    }
}

/// En-tête « BOOKMOBI » d'un MOBI/AZW3 (base de données Palm, octet 60).
fn mobi_header(p: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 68];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).map_err(|e| e.to_string())?;
    if &b[60..68] == b"BOOKMOBI" {
        Ok(tr!("livre Mobipocket reconnu (en-tête BOOKMOBI)", "Mobipocket book recognized (BOOKMOBI header)").into())
    } else {
        Err(tr!("en-tête BOOKMOBI absent", "BOOKMOBI header missing").into())
    }
}

/// Nom de fichier sûr tiré du nom d'origine : lettres, chiffres, espaces, « - » et « _ »,
/// 60 caractères au plus. Le titre du livre en est déduit quand le document n'en a pas.
fn safe_stem(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).take(60).collect();
    s.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_file_keeps_a_safe_original_name() {
        assert_eq!(safe_stem("diapos é"), "diapos é");
        assert_eq!(safe_stem("../a:b*c?\"d<e>|f"), "abcdef");
        assert_eq!(safe_stem("  .  "), "");
        assert_eq!(safe_stem(&"x".repeat(100)).len(), 60);
    }

    #[test]
    fn capabilities() {
        let c = Calibre::new(std::env::current_exe().unwrap(), None, EngineOrigin::System).unwrap();
        let has = |a: &str, b: &str| c.caps.iter().any(|x| x.from.as_str() == a && x.to.as_str() == b);
        assert!(has("epub", "mobi") && has("mobi", "epub") && has("azw3", "pdf") && has("docx", "epub") && has("cbz", "epub"));
        assert!(!has("docx", "pdf"), "bureautique → PDF : LibreOffice");
        assert!(!has("odt", "epub"), "lecteur ODF de Calibre peu fiable");
        assert!(!has("epub", "epub"));
        assert_eq!(c.caps.iter().find(|x| x.from.as_str() == "pdf").unwrap().fidelity, Fidelity::Lossy);
    }
}
