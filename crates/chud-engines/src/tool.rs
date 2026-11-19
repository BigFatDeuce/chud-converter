//! Outils partagés par les adaptateurs de moteurs externes (Pandoc, LibreOffice, qpdf…).
//!
//! - Les fichiers sont **préparés** dans un dossier de travail propre à l'étape, sous un nom simple
//!   (`input.docx`) : un nom d'utilisateur contenant `[`, `:` ou des caractères exotiques n'est
//!   jamais interprété par l'outil (sélection de page d'ImageMagick, préfixe de format…).
//!   L'original n'est jamais modifié : lien physique quand c'est possible, copie sinon.
//! - Chaque outil est lancé avec des arguments séparés, un délai et un plafond mémoire.
//! - Un code de retour nul ne suffit jamais : le fichier produit doit exister, être non vide,
//!   puis il est vérifié par l'adaptateur.

use crate::util::{file_size, run_tool, tail_lines};
use chud_core::{EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, FormatCatalog, Progress};
use chud_core::{tr, trf};
use chud_process::{ProcessOutput, ProcessSpec};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub(crate) fn engine_info(
    id: &str,
    name: &str,
    version: Option<String>,
    origin: EngineOrigin,
    exe: &Path,
    license: &str,
    homepage: &str,
    description: &str,
) -> EngineInfo {
    EngineInfo {
        id: EngineId::new(id),
        name: name.into(),
        version,
        license: license.into(),
        homepage: homepage.into(),
        origin,
        path: Some(exe.to_path_buf()),
        description: description.into(),
    }
}

/// Extension de fichier à utiliser pour un format du catalogue (« docx », « jpg »…).
pub(crate) fn ext(format: &str) -> String {
    FormatCatalog::builtin()
        .get(format)
        .and_then(|d| d.output_extension().map(str::to_owned).or_else(|| d.extensions.first().cloned()))
        .unwrap_or_else(|| format.to_owned())
}

/// Lance un outil au démarrage (liste des formats, version) : court délai, sortie bornée.
pub(crate) fn query(exe: &Path, args: &[&str]) -> Result<ProcessOutput, String> {
    let spec = ProcessSpec::new(exe).args(args.iter()).timeout(Some(Duration::from_secs(30)));
    chud_process::run_capture(&spec, 8 * 1024 * 1024).map_err(|e| e.to_string())
}

/// Dossier de travail d'une étape, supprimé à la fin.
pub(crate) struct Stage {
    pub dir: PathBuf,
}

impl Stage {
    pub fn new(ctx: &ExecContext, prefix: &str) -> Result<Self, EngineError> {
        std::fs::create_dir_all(&ctx.work_dir)?;
        for i in 0..1000u32 {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            let p = ctx.work_dir.join(format!("{prefix}-{}-{nanos}-{i}", std::process::id()));
            match std::fs::create_dir(&p) {
                Ok(()) => return Ok(Self { dir: p }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(EngineError::Other(tr!("impossible de créer un dossier de travail", "cannot create a working folder").into()))
    }

    /// Place la source dans le dossier sous un nom simple. L'original n'est jamais modifié.
    pub fn input(&self, src: &Path, name: &str) -> Result<PathBuf, EngineError> {
        let dest = self.dir.join(name);
        if std::fs::hard_link(src, &dest).is_err() {
            std::fs::copy(src, &dest)?;
        }
        Ok(dest)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn subdir(&self, name: &str) -> Result<PathBuf, EngineError> {
        let p = self.dir.join(name);
        std::fs::create_dir_all(&p)?;
        Ok(p)
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Exécute l'outil ; un code non nul (hors `accept`) est une erreur avec la fin de ses messages.
pub(crate) fn run_ok(
    spec: ProcessSpec,
    ctx: &ExecContext,
    accept: &[i32],
    on_line: &mut dyn FnMut(&str),
) -> Result<ProcessOutput, EngineError> {
    let sink = std::cell::RefCell::new((on_line, Vec::<String>::new(), Vec::<String>::new()));
    let out = run_tool(
        spec,
        ctx,
        &mut |l| {
            let mut g = sink.borrow_mut();
            (g.0)(l);
            keep_tail(&mut g.1, l);
        },
        &mut |l| {
            let mut g = sink.borrow_mut();
            (g.0)(l);
            keep_tail(&mut g.2, l);
        },
    )?;
    let (_, out_lines, lines_err) = sink.into_inner();
    let code = out.code.unwrap_or(-1);
    if code != 0 && !accept.contains(&code) {
        let mut detail = tail_lines(&lines_err.join("\n"), 3);
        if detail.is_empty() {
            detail = tail_lines(&out_lines.join("\n"), 3);
        }
        if detail.is_empty() {
            detail = tail_lines(&out.stderr_tail, 3);
        }
        return Err(EngineError::Process { code: out.code, detail });
    }
    Ok(out)
}

fn keep_tail(v: &mut Vec<String>, l: &str) {
    if !l.trim().is_empty() {
        v.push(l.chars().take(400).collect());
        if v.len() > 12 {
            v.remove(0);
        }
    }
}

/// Avancement « 45% » ou « 45 % » trouvé dans une ligne.
pub(crate) fn percent_in(line: &str) -> Option<f64> {
    let i = line.find('%')?;
    let digits: String = line[..i].trim_end().chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let v: f64 = digits.chars().rev().collect::<String>().parse().ok()?;
    (0.0..=100.0).contains(&v).then_some(v / 100.0)
}

pub(crate) fn report_fraction(ctx: &ExecContext, f: f64) {
    ctx.report(Progress { fraction: Some(f.clamp(0.0, 1.0)), ..Default::default() });
}

/// Livre le fichier produit à l'emplacement attendu par la tâche (non vide, sinon erreur).
pub(crate) fn deliver(produced: &Path, output: &Path, tool: &str) -> Result<(), EngineError> {
    if !produced.is_file() {
        return Err(EngineError::Other(trf!("{tool} n'a produit aucun fichier", "{tool} produced no file")));
    }
    if file_size(produced) == 0 {
        return Err(EngineError::Other(trf!("{tool} a produit un fichier vide", "{tool} produced an empty file")));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(produced, output).is_err() {
        std::fs::copy(produced, output)?;
        let _ = std::fs::remove_file(produced);
    }
    Ok(())
}

/// Premier fichier du dossier portant cette extension (sorties nommées par l'outil lui-même).
pub(crate) fn find_with_ext(dir: &Path, ext: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case(ext)))
        .collect();
    found.sort();
    found.into_iter().next()
}

fn head_tail(path: &Path, n: usize) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut head = vec![0; n.min(len as usize)];
    f.read_exact(&mut head)?;
    let start = len.saturating_sub(n as u64);
    f.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail)?;
    Ok((head, tail))
}

/// Contrôle structurel d'un PDF : en-tête, marque de fin, au moins une page déclarée.
pub(crate) fn check_pdf(path: &Path) -> Result<String, String> {
    let (head, tail) = head_tail(path, 4096).map_err(|e| e.to_string())?;
    if !head.starts_with(b"%PDF-") {
        return Err(tr!("en-tête PDF absent", "PDF header missing").into());
    }
    if !tail.windows(5).any(|w| w == b"%%EOF") {
        return Err(tr!("fin de fichier PDF absente (fichier tronqué)", "PDF end-of-file marker missing (truncated file)").into());
    }
    let version = String::from_utf8_lossy(&head[5..head.len().min(8)]).trim().to_owned();
    Ok(trf!("PDF {version} complet (en-tête et fin de fichier)", "complete PDF {version} (header and end of file)"))
}

/// Contrôle d'un conteneur ZIP (DOCX, ODT, EPUB…) : lisible en entier, membres attendus présents.
pub(crate) fn check_zip(path: &Path, required: &[&str]) -> Result<String, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| trf!("conteneur ZIP illisible : {e}", "unreadable ZIP container: {e}"))?;
    for name in required {
        if z.by_name(name).is_err() {
            return Err(trf!("élément « {name} » absent", "item “{name}” missing"));
        }
    }
    // Chaque membre est décompressé : un fichier tronqué ou corrompu échoue ici.
    let n = z.len();
    for i in 0..n {
        let mut m = z.by_index(i).map_err(|e| e.to_string())?;
        std::io::copy(&mut m, &mut std::io::sink()).map_err(|e| trf!("membre {i} illisible : {e}", "member {i} unreadable: {e}"))?;
    }
    Ok(trf!("conteneur ZIP intact ({n} éléments, tous décompressés)", "intact ZIP container ({n} items, all decompressed)"))
}

/// Texte UTF-8 valide et non vide.
pub(crate) fn check_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| tr!("texte non UTF-8", "text is not UTF-8").to_string())?;
    let words = text.split_whitespace().count();
    if words == 0 {
        return Err(tr!("aucun texte dans le résultat", "no text in the result").into());
    }
    Ok(trf!("texte UTF-8 valide ({words} mots)", "valid UTF-8 text ({words} words)"))
}

/// Décodage complet d'une image par la bibliothèque `image` (formats qu'elle sait lire).
pub(crate) fn decode_image(path: &Path) -> Option<Result<String, String>> {
    let fmt = image::ImageFormat::from_path(path).ok()?;
    if !fmt.reading_enabled() {
        return None;
    }
    let r = image::ImageReader::open(path).map_err(|e| e.to_string()).and_then(|r| r.with_guessed_format().map_err(|e| e.to_string()));
    Some(
        r.and_then(|r| r.decode().map_err(|e| e.to_string()))
            .map(|img| trf!("image entièrement décodée ({}×{})", "image fully decoded ({}×{})", img.width(), img.height())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_parsing() {
        assert_eq!(percent_in("34% Converting input to HTML..."), Some(0.34));
        assert_eq!(percent_in("Progress: 7%"), Some(0.07));
        assert_eq!(percent_in("Progress: 100 %"), Some(1.0));
        assert_eq!(percent_in("no percent"), None);
        assert_eq!(percent_in("500%"), None);
    }

    #[test]
    fn pdf_and_text_checks() {
        let dir = std::env::temp_dir().join(format!("chud-tool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.pdf");
        std::fs::write(&p, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\ntrailer\n%%EOF\n").unwrap();
        assert!(check_pdf(&p).is_ok());
        std::fs::write(&p, b"%PDF-1.7\n1 0 obj\n").unwrap();
        assert!(check_pdf(&p).is_err(), "tronqué");
        let t = dir.join("a.txt");
        std::fs::write(&t, "bonjour le monde").unwrap();
        assert!(check_text(&t).unwrap().contains('3'));
        std::fs::write(&t, "  \n").unwrap();
        assert!(check_text(&t).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
