//! Adaptateur LibreOffice : documents bureautiques (Writer, Calc, Impress) en mode sans interface.
//!
//! - Profil utilisateur **isolé** (`-env:UserInstallation`), jamais celui de l'utilisateur : une
//!   fenêtre LibreOffice ouverte n'est pas touchée, aucune macro ni extension n'est chargée.
//!   Les profils sont réutilisés d'une conversion à l'autre (le premier démarrage est lent),
//!   un profil par conversion simultanée, supprimés quand le moteur est déchargé. Chaque moteur
//!   a son propre dossier de profils : l'ancien moteur, déchargé après un rechargement de la
//!   liste, ne supprime pas les profils du nouveau en pleine conversion (LibreOffice afficherait
//!   alors « User installation could not be completed » et attendrait un clic).
//! - La source est placée sous un nom simple (`input.docx`) dans le dossier de travail.
//! - Le code de retour de LibreOffice ne dit rien de fiable : le fichier produit doit exister,
//!   puis sa structure est contrôlée (ZIP intact, PDF complet…).
//! - Seuls les modules installés sont annoncés : les distributions Linux livrent Writer, Calc et
//!   Impress dans des paquets séparés (voir [`missing_modules`]).

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FeatureSet, Fidelity, FormatId, MissingPart, StepOutput,
    StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Numéro du prochain moteur LibreOffice de ce processus (un dossier de profils chacun).
static INSTANCES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Writer,
    Calc,
    Impress,
}

const FAMILIES: [Family; 3] = [Family::Writer, Family::Calc, Family::Impress];

impl Family {
    /// Fichier de configuration déposé par le module dans `share/registry`.
    fn registry_file(self) -> &'static str {
        match self {
            Family::Writer => "writer.xcd",
            Family::Calc => "calc.xcd",
            Family::Impress => "impress.xcd",
        }
    }

    fn missing(self) -> MissingPart {
        let (name, category) = match self {
            Family::Writer => (tr!("Writer (textes)", "Writer (documents)"), "document"),
            Family::Calc => (tr!("Calc (classeurs)", "Calc (spreadsheets)"), "spreadsheet"),
            Family::Impress => (tr!("Impress (présentations)", "Impress (presentations)"), "presentation"),
        };
        MissingPart { name: name.into(), categories: vec![category.into()] }
    }
}

/// Modules présents à côté de `exe`. Chaque module dépose son `share/registry/<module>.xcd`
/// (Fedora : `libreoffice-calc` apporte `calc.xcd`) ; sans lui, LibreOffice refuse d'ouvrir
/// les fichiers du module (« source file could not be loaded »). Disposition inconnue (pas de
/// `main.xcd`, comme sous macOS) : tous les modules sont supposés présents.
fn installed_families(exe: &Path) -> Vec<Family> {
    let real = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let Some(registry) = real.parent().and_then(Path::parent).map(|d| d.join("share").join("registry")) else {
        return FAMILIES.to_vec();
    };
    if !registry.join("main.xcd").is_file() {
        return FAMILIES.to_vec();
    }
    FAMILIES.into_iter().filter(|f| registry.join(f.registry_file()).is_file()).collect()
}

/// Modules de LibreOffice absents de l'installation trouvée en `exe` (paquets de la
/// distribution non installés).
pub fn missing_modules(exe: &Path) -> Vec<MissingPart> {
    let present = installed_families(exe);
    FAMILIES.into_iter().filter(|f| !present.contains(f)).map(Family::missing).collect()
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
    /// Profils créés jusqu'ici (leur numéro).
    created: AtomicUsize,
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
        let present = installed_families(&exe);
        for (from, fam) in INPUTS.iter().filter(|(_, f)| present.contains(f)) {
            for (f, to, _) in OUTPUTS.iter().filter(|o| o.0 == *fam) {
                debug_assert_eq!(f, fam);
                if from == to {
                    continue;
                }
                let fidelity = if matches!(*to, "txt" | "csv") { Fidelity::Lossy } else { Fidelity::Depends };
                caps.push(ConversionCap::new(from, to, fidelity, &preserved(*fam, to), 3));
            }
        }
        let n = INSTANCES.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("chud-libreoffice-{}-{n}", std::process::id()));
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
            created: AtomicUsize::new(0),
            root,
        })
    }

    fn profile(&self) -> Result<Profile<'_>, EngineError> {
        let reused = self.profiles.lock().ok().and_then(|mut v| v.pop());
        let dir = match reused {
            Some(d) => d,
            None => {
                // Un numéro, pas l'heure : deux conversions simultanées n'ont jamais le même profil.
                let d = self.root.join(format!("profile-{}", self.created.fetch_add(1, Ordering::Relaxed)));
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
        verify_document(output, task)
    }
}

/// Contrôle de structure, puis relecture du texte quand l'original et le résultat sont lisibles :
/// les mots de l'original doivent se retrouver dans le résultat.
pub(crate) fn verify_document(output: &Path, task: &StepTask) -> Verification {
    match verify_office(output, task.to.as_str()) {
        Verification::Structural(s) => match crate::doctext::compare(&task.input, task.from.as_str(), output, task.to.as_str()) {
            Some(Ok(text)) => Verification::Decoded(format!("{s} ; {text}")),
            Some(Err(e)) => Verification::Failed(e),
            None => Verification::Structural(s),
        },
        other => other,
    }
}

/// Contrôle structurel d'un fichier bureautique selon son format.
pub(crate) fn verify_office(output: &Path, to: &str) -> Verification {
    let r = match to {
        "pdf" => tool::check_pdf(output),
        // La partie principale doit exister : un paquet vide ou d'un autre type est refusé.
        "docx" => tool::check_zip(output, &["[Content_Types].xml", "word/document.xml"]),
        "xlsx" => tool::check_zip(output, &["[Content_Types].xml", "xl/workbook.xml"]),
        "pptx" => tool::check_zip(output, &["[Content_Types].xml", "ppt/presentation.xml"]),
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

    /// Un paquet Office sans sa partie principale (vide, ou d'un autre type) est refusé.
    #[test]
    fn office_packages_need_their_main_part() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("chud-office-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack = |name: &str, parts: &[&str]| {
            let p = dir.join(name);
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            for part in parts {
                z.start_file(*part, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(b"<x/>").unwrap();
            }
            z.finish().unwrap();
            p
        };
        let empty = pack("vide.docx", &["[Content_Types].xml"]);
        let sheet = pack("classeur.docx", &["[Content_Types].xml", "xl/workbook.xml"]);
        let doc = pack("lettre.docx", &["[Content_Types].xml", "word/document.xml"]);
        assert!(matches!(verify_office(&empty, "docx"), Verification::Failed(_)));
        assert!(matches!(verify_office(&sheet, "docx"), Verification::Failed(_)));
        assert!(matches!(verify_office(&sheet, "xlsx"), Verification::Structural(_)));
        assert!(matches!(verify_office(&doc, "docx"), Verification::Structural(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn urls_are_escaped() {
        assert_eq!(file_url(Path::new("/tmp/a b/p")), "file:///tmp/a%20b/p");
        assert_eq!(file_url(Path::new(r"C:\Users\Élo\p")), "file:///C:/Users/%C3%89lo/p");
    }

    /// Installation découpée comme sous Fedora : Writer seul. Calc et Impress ne sont pas
    /// annoncés (ils échoueraient : « source file could not be loaded »).
    #[test]
    fn only_installed_modules_are_offered() {
        let root = std::env::temp_dir().join(format!("chud-lo-modules-{}", std::process::id()));
        let program = root.join("program");
        let registry = root.join("share").join("registry");
        std::fs::create_dir_all(&program).unwrap();
        std::fs::create_dir_all(&registry).unwrap();
        let exe = program.join("soffice");
        std::fs::write(&exe, b"").unwrap();
        for f in ["main.xcd", "writer.xcd"] {
            std::fs::write(registry.join(f), b"").unwrap();
        }
        let lo = LibreOffice::new(exe.clone(), None, EngineOrigin::System).unwrap();
        let has = |a: &str, b: &str| lo.caps.iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(has("docx", "pdf") && has("txt", "odt"));
        assert!(!has("csv", "xlsx") && !has("xlsx", "pdf") && !has("pptx", "pdf") && !has("odp", "pptx"));
        let missing = missing_modules(&exe);
        assert_eq!(missing.len(), 2);
        assert_eq!(missing[0].categories, ["spreadsheet"]);
        assert_eq!(missing[1].categories, ["presentation"]);
        // Les trois modules : rien ne manque.
        for f in ["calc.xcd", "impress.xcd"] {
            std::fs::write(registry.join(f), b"").unwrap();
        }
        assert!(missing_modules(&exe).is_empty());
        assert!(LibreOffice::new(exe, None, EngineOrigin::System).unwrap().caps.iter().any(|c| c.from.as_str() == "pptx"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Liste des moteurs rechargée (après une installation) : l'ancien moteur LibreOffice,
    /// déchargé, ne supprime pas le profil que le nouveau est en train d'utiliser.
    #[test]
    fn each_engine_keeps_its_own_profiles() {
        let exe = std::env::current_exe().unwrap();
        let old = LibreOffice::new(exe.clone(), None, EngineOrigin::System).unwrap();
        let new = LibreOffice::new(exe, None, EngineOrigin::System).unwrap();
        assert_ne!(old.root, new.root);
        drop(old.profile().unwrap());
        let busy = new.profile().unwrap();
        let other = new.profile().unwrap();
        assert_ne!(busy.dir, other.dir, "deux conversions simultanées, deux profils");
        let dir = busy.dir.clone().unwrap();
        let root = new.root.clone();
        drop(old);
        assert!(dir.is_dir(), "profil en cours d'utilisation supprimé");
        drop((busy, other));
        drop(new);
        assert!(!root.exists(), "profils supprimés avec leur moteur");
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
