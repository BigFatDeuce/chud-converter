//! Adaptateur LibreOffice : documents bureautiques (Writer, Calc, Impress) en mode sans interface.
//!
//! - Profil utilisateur **isolé** (`-env:UserInstallation`), jamais celui de l'utilisateur : une
//!   fenêtre LibreOffice ouverte n'est pas touchée, aucune macro ni extension n'est chargée.
//!   Les profils sont réutilisés d'une conversion à l'autre (le premier démarrage est lent),
//!   un profil par conversion simultanée, supprimés quand le moteur est déchargé.
//! - La source est placée sous un nom simple (`input.docx`) dans le dossier de travail.
//! - Le code de retour de LibreOffice ne dit rien de fiable : le fichier produit doit exister,
//!   puis sa structure est contrôlée (ZIP intact, PDF complet…).

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, FormatId, StepOutput, StepTask,
    Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Writer,
    Calc,
    Impress,
}

const INPUTS: &[(&str, Family)] = &[
    ("doc", Family::Writer),
    ("docx", Family::Writer),
    ("docm", Family::Writer),
    ("odt", Family::Writer),
    ("rtf", Family::Writer),
    ("txt", Family::Writer),
    ("html", Family::Writer),
    ("wpd", Family::Writer),
    ("pages", Family::Writer),
    ("abw", Family::Writer),
    ("xlsx", Family::Calc),
    ("xlsm", Family::Calc),
    ("xls", Family::Calc),
    ("ods", Family::Calc),
    ("numbers", Family::Calc),
    ("csv", Family::Calc),
    ("pptx", Family::Impress),
    ("ppt", Family::Impress),
    ("odp", Family::Impress),
    ("key", Family::Impress),
];

/// (famille, cible, filtre d'export)
const OUTPUTS: &[(Family, &str, &str)] = &[
    (Family::Writer, "docx", "docx:MS Word 2007 XML"),
    (Family::Writer, "doc", "doc:MS Word 97"),
    (Family::Writer, "odt", "odt:writer8"),
    (Family::Writer, "rtf", "rtf:Rich Text Format"),
    (Family::Writer, "txt", "txt:Text (encoded):UTF8"),
    (Family::Writer, "html", "html:HTML (StarWriter)"),
    (Family::Writer, "pdf", "pdf:writer_pdf_Export"),
    (Family::Calc, "xlsx", "xlsx:Calc MS Excel 2007 XML"),
    (Family::Calc, "xls", "xls:MS Excel 97"),
    (Family::Calc, "ods", "ods:calc8"),
    (Family::Calc, "csv", "csv:Text - txt - csv (StarCalc):44,34,76,1"),
    (Family::Calc, "html", "html:HTML (StarCalc)"),
    (Family::Calc, "pdf", "pdf:calc_pdf_Export"),
    (Family::Impress, "pptx", "pptx:Impress MS PowerPoint 2007 XML"),
    (Family::Impress, "ppt", "ppt:MS PowerPoint 97"),
    (Family::Impress, "odp", "odp:impress8"),
    (Family::Impress, "pdf", "pdf:impress_pdf_Export"),
];

fn family(id: &str) -> Option<Family> {
    INPUTS.iter().find(|i| i.0 == id).map(|i| i.1)
}

fn filter(fam: Family, to: &str) -> Option<&'static str> {
    OUTPUTS.iter().find(|o| o.0 == fam && o.1 == to).map(|o| o.2)
}

fn preserved(fam: Family, to: &str) -> FeatureSet {
    let pdf = ["multipage", "vector", "text_layer", "images", "fonts_embedded", "metadata"];
    match (fam, to) {
        (_, "txt" | "csv") => FeatureSet::new(),
        (_, "pdf") => features(&pdf),
        (Family::Writer, "html") | (Family::Calc, "html") => features(&["rich_text", "styles", "tables"]),
        (Family::Writer, "rtf") => features(&["rich_text", "styles", "images", "tables", "metadata"]),
        (Family::Writer, _) => features(&["rich_text", "styles", "images", "tables", "comments", "track_changes", "metadata", "forms"]),
        (Family::Calc, _) => features(&["multi_sheet", "formulas", "styles", "charts", "images", "comments", "types", "metadata"]),
        (Family::Impress, _) => features(&["slides", "rich_text", "styles", "images", "animation", "notes", "media", "metadata"]),
    }
}

/// Adresse `file:///` d'un dossier, pour `-env:UserInstallation`.
fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::from(if s.starts_with('/') { "file://" } else { "file:///" });
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub struct LibreOffice {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
    profiles: Mutex<Vec<PathBuf>>,
    root: PathBuf,
}

/// Profil emprunté le temps d'une conversion.
struct Profile<'a> {
    owner: &'a LibreOffice,
    dir: Option<PathBuf>,
}

impl Drop for Profile<'_> {
    fn drop(&mut self) {
        if let (Some(d), Ok(mut free)) = (self.dir.take(), self.owner.profiles.lock()) {
            free.push(d);
        }
    }
}

impl LibreOffice {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        let mut caps = Vec::new();
        for (from, fam) in INPUTS {
            for (f, to, _) in OUTPUTS.iter().filter(|o| o.0 == *fam) {
                debug_assert_eq!(f, fam);
                if from == to {
                    continue;
                }
                let fidelity = if matches!(*to, "txt" | "csv") { Fidelity::Lossy } else { Fidelity::Depends };
                caps.push(ConversionCap::new(from, to, fidelity, &preserved(*fam, to), 3));
            }
        }
        let root = std::env::temp_dir().join(format!("chud-libreoffice-{}", std::process::id()));
        Ok(Self {
            info: tool::engine_info(
                "libreoffice",
                "LibreOffice",
                version,
                origin,
                &exe,
                "MPL-2.0",
                "https://www.libreoffice.org",
                tr!(
                    "Bureautique : Word, Excel, PowerPoint, OpenDocument, PDF (mise en page conservée).",
                    "Office documents: Word, Excel, PowerPoint, OpenDocument, PDF (layout kept)."
                ),
            ),
            exe,
            caps,
            profiles: Mutex::new(Vec::new()),
            root,
        })
    }

    fn profile(&self) -> Result<Profile<'_>, EngineError> {
        let reused = self.profiles.lock().ok().and_then(|mut v| v.pop());
        let dir = match reused {
            Some(d) => d,
            None => {
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                let d = self.root.join(format!("profile-{stamp}"));
                std::fs::create_dir_all(&d)?;
                d
            }
        };
        Ok(Profile { owner: self, dir: Some(dir) })
    }
}

impl Drop for LibreOffice {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Engine for LibreOffice {
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
        let fam = family(from).ok_or_else(|| EngineError::Unsupported(from.into()))?;
        let filt = filter(fam, to).ok_or_else(|| EngineError::Unsupported(format!("{from} → {to}")))?;
        let stage = Stage::new(ctx, "libreoffice")?;
        let input = stage.input(&task.input, &format!("input.{}", tool::ext(from)))?;
        let outdir = stage.subdir("out")?;
        let profile = self.profile()?;
        let dir = profile.dir.as_deref().expect("profil");
        let mut spec = ProcessSpec::new(&self.exe)
            .args(["--headless", "--invisible", "--norestore", "--nolockcheck", "--nodefault", "--nologo", "--nofirststartwizard"])
            .arg(format!("-env:UserInstallation={}", file_url(dir)))
            .args(["--convert-to", filt, "--outdir"])
            .arg(&outdir);
        if from == "csv" {
            spec = spec.arg("--infilter=Text - txt - csv (StarCalc):44,34,76,1");
        }
        spec = spec.arg(&input).cwd(&stage.dir).timeout(ctx.timeout.or(Some(Duration::from_secs(900))));
        let mut lines = Vec::new();
        tool::run_ok(spec, ctx, &[], &mut |l| {
            if !l.trim().is_empty() {
                lines.push(l.trim().to_owned());
            }
        })?;
        let produced = tool::find_with_ext(&outdir, &tool::ext(to)).ok_or_else(|| {
            let why = lines.iter().rev().find(|l| l.to_lowercase().contains("error")).cloned().unwrap_or_default();
            EngineError::Other(if why.is_empty() {
                tr!(
                    "LibreOffice n'a produit aucun fichier (document illisible ou protégé ?)",
                    "LibreOffice produced no file (unreadable or protected document?)"
                )
                .into()
            } else {
                trf!("LibreOffice n'a produit aucun fichier : {why}", "LibreOffice produced no file: {why}")
            })
        })?;
        tool::deliver(&produced, &task.output, "LibreOffice")?;
        let mut notes = Vec::new();
        if to == "csv" {
            notes.push(tr!("seule la première feuille est exportée en CSV", "only the first sheet is exported to CSV").to_owned());
        }
        if to == "html" {
            notes.push(tr!("les images ne sont pas intégrées au fichier HTML", "images are not embedded in the HTML file").to_owned());
        }
        if from == "docm" || from == "xlsm" {
            notes.push(tr!("les macros ne sont pas reprises", "macros are not carried over").to_owned());
        }
        tool::report_fraction(ctx, 1.0);
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, _ctx: &ExecContext) -> Verification {
        verify_office(output, task.to.as_str())
    }
}

/// Contrôle structurel d'un fichier bureautique selon son format.
pub(crate) fn verify_office(output: &Path, to: &str) -> Verification {
    let r = match to {
        "pdf" => tool::check_pdf(output),
        "docx" | "xlsx" | "pptx" => tool::check_zip(output, &["[Content_Types].xml"]),
        "odt" | "ods" | "odp" => tool::check_zip(output, &["mimetype", "content.xml"]),
        "doc" | "xls" | "ppt" => starts_with(output, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
            .map(|_| tr!("document OLE (format binaire Microsoft) reconnu", "OLE document (Microsoft binary format) recognized").into()),
        "rtf" => starts_with(output, b"{\\rtf").map(|_| tr!("document RTF reconnu", "RTF document recognized").into()),
        "txt" | "csv" => tool::check_text(output),
        "html" => std::fs::read(output).map_err(|e| e.to_string()).and_then(|b| {
            let t = String::from_utf8_lossy(&b).to_lowercase();
            if t.contains("<html") {
                Ok(tr!("document HTML reconnu", "HTML document recognized").into())
            } else {
                Err(tr!("pas de HTML", "no HTML").into())
            }
        }),
        _ => return Verification::SignatureOnly,
    };
    match r {
        Ok(s) => Verification::Structural(s),
        Err(e) => Verification::Failed(e),
    }
}

fn starts_with(p: &Path, sig: &[u8]) -> Result<(), String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut b = vec![0; sig.len()];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    if b == sig { Ok(()) } else { Err(tr!("signature du format absente", "format signature missing").into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_escaped() {
        assert_eq!(file_url(Path::new("/tmp/a b/p")), "file:///tmp/a%20b/p");
        assert_eq!(file_url(Path::new(r"C:\Users\Élo\p")), "file:///C:/Users/%C3%89lo/p");
    }

    #[test]
    fn families_do_not_mix() {
        let lo = LibreOffice::new(std::env::current_exe().unwrap(), None, EngineOrigin::System).unwrap();
        let has = |a: &str, b: &str| lo.caps.iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(has("docx", "pdf") && has("xlsx", "ods") && has("pptx", "pdf") && has("doc", "docx") && has("csv", "xlsx"));
        assert!(!has("docx", "xlsx") && !has("pptx", "docx") && !has("xlsx", "xlsx"));
        let csv = lo.caps.iter().find(|c| c.from.as_str() == "xlsx" && c.to.as_str() == "csv").unwrap();
        assert_eq!(csv.fidelity, Fidelity::Lossy);
    }
}
