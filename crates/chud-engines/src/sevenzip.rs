//! Adaptateur 7-Zip : extraction, création et conversion d'archives.
//!
//! Sécurité (dans cet ordre, à chaque extraction) :
//! 1. **liste** des entrées (`7z l -slt`) avant toute écriture ;
//! 2. **validation** : chemins absolus, `..`, lecteurs, flux alternatifs NTFS (`:`), noms réservés
//!    Windows, caractères de contrôle, doublons, archives chiffrées, nombre d'entrées, taille totale et
//!    taux de compression (bombes de décompression) ;
//! 3. **extraction** dans un répertoire de travail, sous surveillance de la taille écrite ;
//! 4. **contrôle a posteriori** : aucun lien symbolique ni point d'analyse (jonction), nombre et taille
//!    des fichiers conformes à l'index.
//!
//! Les jokers sont désactivés (`-spd`), aucun mot de passe n'est jamais demandé (un mot de passe
//! factice est fourni pour qu'une archive chiffrée échoue au lieu d'attendre une saisie).

use crate::util::{dir_size, file_size, run_tool, run_tool_with, tail_lines};
use chud_core::feature::features;
use chud_core::operation::ARCHIVE_CREATE;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, Fidelity, FormatCatalog, FormatId,
    OperationReport, OperationTask, OptionSpec, OptionValues, Progress, SourceInfo, StepOutput, StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DUMMY_PASSWORD: &str = "-pchud-no-password";
const MAX_ENTRIES: usize = 1_000_000;
/// Taux de compression au-delà duquel une grosse archive est considérée comme une bombe.
const MAX_RATIO: u64 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// Archive à arborescence (zip, 7z, tar…).
    Tree,
    /// Flux compressé d'un seul fichier (gz, bz2, xz…).
    Single,
    /// tar dans un flux compressé (tar.gz…) : le champ donne le type 7-Zip du flux extérieur.
    Layered(&'static str),
}

struct ArcFmt {
    id: &'static str,
    /// Type 7-Zip (`-t`) ; pour `Layered`, type de l'archive intérieure (tar).
    ty: &'static str,
    shape: Shape,
}

const fn a(id: &'static str, ty: &'static str, shape: Shape) -> ArcFmt {
    ArcFmt { id, ty, shape }
}

static ARCS: &[ArcFmt] = &[
    a("zip", "zip", Shape::Tree),
    a("7z", "7z", Shape::Tree),
    a("rar", "rar", Shape::Tree),
    a("tar", "tar", Shape::Tree),
    a("gz", "gzip", Shape::Single),
    a("bz2", "bzip2", Shape::Single),
    a("xz", "xz", Shape::Single),
    a("zst", "zstd", Shape::Single),
    a("lzma", "lzma", Shape::Single),
    a("tgz", "tar", Shape::Layered("gzip")),
    a("tbz2", "tar", Shape::Layered("bzip2")),
    a("txz", "tar", Shape::Layered("xz")),
    a("tzst", "tar", Shape::Layered("zstd")),
    a("iso", "iso", Shape::Tree),
    a("wim", "wim", Shape::Tree),
    a("cab", "cab", Shape::Tree),
    a("arj", "arj", Shape::Tree),
    a("lzh", "lzh", Shape::Tree),
    a("cpio", "cpio", Shape::Tree),
    a("deb", "ar", Shape::Tree),
    a("rpm", "rpm", Shape::Tree),
    a("dmg", "dmg", Shape::Tree),
    a("vhd", "vhd", Shape::Tree),
    a("squashfs", "squashfs", Shape::Tree),
    a("cbz", "zip", Shape::Tree),
    a("cbr", "rar", Shape::Tree),
    a("cb7", "7z", Shape::Tree),
    a("cbt", "tar", Shape::Tree),
];

fn arc(id: &str) -> Option<&'static ArcFmt> {
    ARCS.iter().find(|f| f.id == id)
}

const DIR: &str = "directory";

/// Formats proposés pour réunir plusieurs fichiers, le plus répandu d'abord.
const ARCHIVE_TARGETS: &[&str] = &["zip", "7z", "tar", "tgz", "tbz2", "txz"];

/// Entrée d'archive telle que listée par 7-Zip.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub path: String,
    pub size: Option<u64>,
    pub is_dir: bool,
    pub encrypted: bool,
    pub link: bool,
    pub unix_mode: bool,
}

pub struct SevenZip {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
    readable: HashSet<String>,
    creatable: HashSet<String>,
}

/// Analyse la section « Formats: » de `7z i`.
fn parse_formats(text: &str) -> (HashSet<String>, HashSet<String>) {
    let mut read = HashSet::new();
    let mut create = HashSet::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("Formats:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim().is_empty() || line.ends_with(':') {
            break;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 3 {
            continue;
        }
        let flags = toks[1];
        let update_flags =
            |t: &str| t.len() == 5 && t.ends_with(|c: char| c.is_ascii_digit()) && t.chars().all(|c| "wudn.0123456789".contains(c));
        let name = if update_flags(toks[2]) { toks.get(3).copied() } else { Some(toks[2]) };
        let Some(name) = name else { continue };
        let name = name.to_ascii_lowercase();
        if flags.starts_with('C') {
            create.insert(name.clone());
        }
        read.insert(name);
    }
    (read, create)
}

/// Analyse la sortie `7z l -slt -ba`.
fn parse_listing(text: &str) -> Vec<Entry> {
    // Une entrée par bloc « clé = valeur », blocs séparés par une ligne vide. La ligne « Path »
    // manque pour un flux compressé seul dont le nom n'est pas stocké (xz, bzip2, zstd) : le
    // bloc compte quand même, sinon l'extraction paraîtrait contenir un fichier de trop.
    let mut out = Vec::new();
    let mut cur: Option<Entry> = None;
    for line in text.lines().map(|l| l.trim_end_matches('\r')) {
        if line.trim().is_empty() {
            if let Some(e) = cur.take() {
                out.push(e);
            }
            continue;
        }
        let Some((k, v)) = line.split_once(" = ").or_else(|| line.strip_suffix(" =").map(|k| (k, ""))) else { continue };
        if k == "Path" && cur.as_ref().is_some_and(|e| !e.path.is_empty()) {
            out.extend(cur.take());
        }
        let e = cur.get_or_insert_with(Entry::default);
        match k {
            "Path" => e.path = v.to_owned(),
            "Folder" => e.is_dir = v.trim() == "+",
            "Size" => e.size = v.trim().parse().ok(),
            "Encrypted" => e.encrypted = v.trim() == "+",
            "Symbolic Link" | "Hard Link" | "Link" => e.link |= !v.trim().is_empty(),
            "Attributes" => {
                let v = v.trim();
                if v.contains('D') && !v.contains(' ') && v.chars().all(|c| c.is_ascii_uppercase()) {
                    e.is_dir = true;
                }
                // Mode Unix (« -rwxr-xr-x », « lrwxrwxrwx »…) : présent en fin de champ.
                if let Some(mode) = v.split_whitespace().last()
                    && mode.len() == 10
                    && mode[1..].chars().all(|c| "rwxsStT-".contains(c))
                {
                    e.unix_mode = true;
                    if mode.starts_with('l') {
                        e.link = true;
                    }
                    if mode.starts_with('d') {
                        e.is_dir = true;
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(e) = cur {
        out.push(e);
    }
    out
}

fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches("./").to_owned()
}

const RESERVED: &[&str] = &["con", "prn", "aux", "nul", "conin$", "conout$"];

fn reserved_windows(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    let b = stem.as_bytes();
    b.len() == 4 && (stem.starts_with("com") || stem.starts_with("lpt")) && b[3].is_ascii_digit()
}

/// Vérifie qu'un chemin d'entrée ne peut pas sortir du dossier d'extraction ni piéger Windows.
pub fn check_entry_path(raw: &str) -> Result<(), String> {
    let p = raw.replace('\\', "/");
    if p.is_empty() {
        return Err(tr!("nom vide", "empty name").into());
    }
    if p.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return Err(trf!("caractère de contrôle dans « {} »", "control character in “{}”", p.escape_debug()));
    }
    if p.starts_with('/') {
        return Err(trf!("chemin absolu « {p} »", "absolute path “{p}”"));
    }
    if p.len() > 4096 {
        return Err(tr!("chemin trop long", "path too long").into());
    }
    let windows = cfg!(windows);
    if p.as_bytes().get(1) == Some(&b':') || (windows && p.contains(':')) {
        return Err(trf!("lecteur ou flux NTFS dans « {p} »", "drive or NTFS stream in “{p}”"));
    }
    for comp in p.split('/') {
        match comp {
            "" | "." => continue,
            ".." => return Err(trf!("remontée de dossier (« .. ») dans « {p} »", "parent folder reference (“..”) in “{p}”")),
            c => {
                if windows {
                    if reserved_windows(c) {
                        return Err(trf!("nom réservé par Windows « {c} »", "name reserved by Windows “{c}”"));
                    }
                    if c.ends_with('.') || c.ends_with(' ') {
                        return Err(trf!("nom terminé par un point ou une espace « {c} »", "name ending with a dot or a space “{c}”"));
                    }
                    if c.chars().any(|ch| "<>\"|?*".contains(ch)) {
                        return Err(trf!("caractère interdit par Windows dans « {c} »", "character forbidden by Windows in “{c}”"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Contrôles globaux sur l'index d'une archive avant extraction.
pub fn check_entries(entries: &[Entry], archive_size: u64, max_unpacked: u64) -> Result<u64, EngineError> {
    if entries.len() > MAX_ENTRIES {
        return Err(EngineError::Refused(trf!(
            "{} entrées : au-delà de la limite de {MAX_ENTRIES}",
            "{} entries: over the limit of {MAX_ENTRIES}",
            entries.len()
        )));
    }
    if entries.iter().any(|e| e.encrypted) {
        return Err(EngineError::Refused(
            tr!("archive protégée par mot de passe : non prise en charge pour l'instant", "password-protected archive: not supported yet")
                .into(),
        ));
    }
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut total: u64 = 0;
    for e in entries {
        if let Err(m) = check_entry_path(&e.path) {
            problems.push(m);
        }
        let key = norm(&e.path).trim_end_matches('/').to_owned();
        let key = if cfg!(windows) { key.to_lowercase() } else { key };
        if !e.is_dir && !seen.insert(key.clone()) {
            problems.push(trf!("entrée en double « {} »", "duplicate entry “{}”", e.path));
        }
        total = total.saturating_add(e.size.unwrap_or(0));
    }
    if !problems.is_empty() {
        let n = problems.len();
        problems.truncate(5);
        return Err(EngineError::Refused(trf!(
            "chemins dangereux dans l'archive ({n}) : {}{}",
            "dangerous paths in the archive ({n}): {}{}",
            problems.join(tr!(" ; ", "; ")),
            if n > 5 { tr!(" ; …", "; …") } else { "" }
        )));
    }
    if total > max_unpacked {
        return Err(EngineError::Refused(trf!(
            "taille décompressée annoncée {} supérieure à la limite {}",
            "announced unpacked size {} exceeds the limit {}",
            human(total),
            human(max_unpacked)
        )));
    }
    if total > (1 << 30) && total / archive_size.max(1) > MAX_RATIO {
        return Err(EngineError::Refused(trf!(
            "taux de compression anormal ({}:1) : bombe de décompression probable",
            "abnormal compression ratio ({}:1): probable decompression bomb",
            total / archive_size.max(1)
        )));
    }
    Ok(total)
}

pub fn human(n: u64) -> String {
    // Mêmes unités que le reste de l'application (Ko en français, KB en anglais).
    let en = chud_core::i18n::lang() == chud_core::i18n::Lang::En;
    const U: [[&str; 2]; 5] = [["o", "B"], ["Ko", "KB"], ["Mo", "MB"], ["Go", "GB"], ["To", "TB"]];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    let unit = U[i][usize::from(en)];
    let text = if i == 0 { format!("{n} {unit}") } else { format!("{v:.1} {unit}") };
    if en { text } else { text.replace('.', ",") }
}

/// Parcourt un dossier extrait sans suivre les liens. Refuse liens et points d'analyse.
fn walk_files(root: &Path) -> Result<BTreeMap<String, u64>, String> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| trf!("lecture de {} : {e}", "reading {}: {e}", d.display()))?;
        for ent in rd {
            let ent = ent.map_err(|e| e.to_string())?;
            let p = ent.path();
            let m = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() || is_reparse_point(&m) {
                return Err(trf!("lien ou jonction « {} »", "link or junction “{}”", p.strip_prefix(root).unwrap_or(&p).display()));
            }
            if m.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                files.insert(rel, m.len());
            }
        }
    }
    Ok(files)
}

#[cfg(windows)]
fn is_reparse_point(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_m: &std::fs::Metadata) -> bool {
    false
}

fn unique_dir(base: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    for i in 0..1000u32 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let p = base.join(format!("{prefix}-{}-{nanos}-{i}", std::process::id()));
        match std::fs::create_dir(&p) {
            Ok(()) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(tr!("impossible de créer un dossier de travail", "cannot create a working folder")))
}

/// Supprime un dossier de travail à la sortie de portée.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn with_prefix(prefix: &str, p: &Path) -> OsString {
    let mut s = OsString::from(prefix);
    s.push(p.as_os_str());
    s
}

fn parse_percent(seg: &str) -> Option<f64> {
    let t = seg.trim_start();
    let end = t.find('%')?;
    let n: u32 = t[..end].trim().parse().ok()?;
    (n <= 100).then(|| n as f64 / 100.0)
}

impl SevenZip {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let spec = ProcessSpec::new(&exe).arg("i").timeout(Some(Duration::from_secs(20)));
        let out = chud_process::run_capture(&spec, 4 * 1024 * 1024).map_err(|e| e.to_string())?;
        if !out.success() {
            return Err(trf!("7z i a échoué : {}", "7z i failed: {}", tail_lines(&out.stderr_tail, 2)));
        }
        let (readable, creatable) = parse_formats(&out.stdout_text());
        if readable.is_empty() {
            return Err(tr!("liste des formats 7-Zip illisible", "unreadable 7-Zip format list").into());
        }
        Ok(Self::with_formats(exe, version, origin, readable, creatable))
    }

    pub fn with_formats(
        exe: PathBuf,
        version: Option<String>,
        origin: EngineOrigin,
        readable: HashSet<String>,
        creatable: HashSet<String>,
    ) -> Self {
        let mut me = Self {
            info: EngineInfo {
                id: EngineId::new("7zip"),
                name: "7-Zip".into(),
                version,
                license: tr!(
                    "LGPL-2.1-or-later (+ restriction unRAR pour le code RAR)",
                    "LGPL-2.1-or-later (+ unRAR restriction for the RAR code)"
                )
                .into(),
                homepage: "https://www.7-zip.org".into(),
                origin,
                path: Some(exe.clone()),
                description: tr!(
                    "Archives : extraction sécurisée, création et conversion.",
                    "Archives: safe extraction, creation and conversion."
                )
                .into(),
            },
            exe,
            caps: Vec::new(),
            readable,
            creatable,
        };
        me.caps = me.build_caps();
        me
    }

    fn can_read(&self, f: &ArcFmt) -> bool {
        let inner = self.readable.contains(f.ty) || (f.ty == "rar" && self.readable.contains("rar5"));
        match f.shape {
            Shape::Layered(outer) => inner && self.readable.contains(outer),
            _ => inner,
        }
    }

    fn can_write(&self, f: &ArcFmt) -> bool {
        match f.shape {
            Shape::Layered(outer) => self.creatable.contains("tar") && self.creatable.contains(outer),
            _ => self.creatable.contains(f.ty),
        }
    }

    fn build_caps(&self) -> Vec<ConversionCap> {
        let keep = features(&["directories", "timestamps"]);
        let mut caps = Vec::new();
        for from in ARCS.iter().filter(|f| self.can_read(f)) {
            caps.push(ConversionCap::new(from.id, DIR, Fidelity::Lossless, &keep, 1));
            for to in ARCS.iter().filter(|f| self.can_write(f)) {
                if to.shape == Shape::Single && from.shape != Shape::Single {
                    continue; // une arborescence ne tient pas dans un flux d'un seul fichier
                }
                let cost = if from.id == to.id { 3 } else { 2 };
                caps.push(ConversionCap::new(from.id, to.id, Fidelity::Lossless, &keep, cost));
            }
        }
        for to in ARCS.iter().filter(|f| self.can_write(f) && f.shape != Shape::Single) {
            caps.push(ConversionCap::new(DIR, to.id, Fidelity::Lossless, &keep, 1));
        }
        caps
    }

    /// Type 7-Zip à imposer pour lire un fichier (RAR 4 / RAR 5, VHD / VHDX).
    fn read_type(&self, f: &ArcFmt, path: &Path) -> &'static str {
        let mut magic = [0u8; 8];
        let n = std::fs::File::open(path).and_then(|mut fh| std::io::Read::read(&mut fh, &mut magic)).unwrap_or(0);
        match f.ty {
            "rar" if n >= 8 && &magic[..8] == b"Rar!\x1a\x07\x01\x00" => "rar5",
            "vhd" if n >= 8 && &magic[..8] == b"vhdxfile" => "vhdx",
            t => t,
        }
    }

    /// Lecture (`l`, `x`, `t`) : mot de passe factice pour qu'une archive chiffrée échoue au lieu
    /// d'attendre une saisie.
    fn base_read(&self) -> ProcessSpec {
        ProcessSpec::new(&self.exe).args(["-spd", "-sccUTF-8", DUMMY_PASSWORD])
    }

    /// Écriture (`a`) : surtout **pas** de `-p`, qui chiffrerait l'archive produite.
    fn base_write(&self) -> ProcessSpec {
        ProcessSpec::new(&self.exe).args(["-spd", "-sccUTF-8"])
    }

    /// Liste les entrées d'une archive (type imposé).
    pub fn list(&self, archive: &Path, ty: &str, ctx: Option<&ExecContext>) -> Result<Vec<Entry>, EngineError> {
        let spec = ProcessSpec::new(&self.exe)
            .args(["l", "-slt", "-ba", "-spd", "-sccUTF-8", DUMMY_PASSWORD])
            .arg(format!("-t{ty}"))
            .arg("--")
            .arg(archive)
            .capture_stdout(256 * 1024 * 1024)
            .timeout(Some(Duration::from_secs(300)));
        let out = match ctx {
            Some(c) => run_tool(spec, c, &mut |_| {}, &mut |_| {})?,
            None => chud_process::run_capture(&spec, 256 * 1024 * 1024).map_err(crate::util::map_process_error)?,
        };
        if out.stdout_truncated {
            return Err(EngineError::Refused(tr!("index de l'archive démesuré", "oversized archive index").into()));
        }
        if !out.success() {
            let err = tail_lines(&out.stderr_tail, 3);
            if err.to_lowercase().contains("password") {
                return Err(EngineError::Refused(
                    tr!(
                        "archive protégée par mot de passe : non prise en charge pour l'instant",
                        "password-protected archive: not supported yet"
                    )
                    .into(),
                ));
            }
            return Err(EngineError::InvalidInput(format!("archive illisible : {err}")));
        }
        let mut entries = parse_listing(&out.stdout_text());
        // Flux compressé seul sans nom stocké : 7-Zip l'extrait sous le nom de l'archive sans
        // son extension (« notes.txt.xz » → « notes.txt »).
        if let [only] = entries.as_mut_slice()
            && only.path.is_empty()
        {
            only.path = archive.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "contenu".into());
        }
        Ok(entries)
    }

    fn max_unpacked(o: &OptionValues) -> u64 {
        (o.int_or("max_unpacked_gb", 64).clamp(1, 16384) as u64) << 30
    }

    /// Exécute `7z x` vers `dest` en surveillant la taille écrite.
    fn extract_raw(
        &self,
        archive: &Path,
        ty: &str,
        dest: &Path,
        limit: u64,
        ctx: &ExecContext,
        span: (f64, f64),
    ) -> Result<(), EngineError> {
        std::fs::create_dir_all(dest)?;
        let spec = self
            .base_read()
            .args(["x", "-y", "-bb0", "-bso0", "-bsp1", "-bse1"])
            .arg(format!("-t{ty}"))
            .arg(with_prefix("-o", dest))
            .arg("--")
            .arg(archive);
        let last = Cell::new(Instant::now());
        let exceeded = Cell::new(false);
        let watchdog = || {
            if last.get().elapsed() >= Duration::from_millis(500) {
                last.set(Instant::now());
                if dir_size(dest) > limit {
                    exceeded.set(true);
                }
            }
            exceeded.get()
        };
        let mut on_out = |seg: &str| {
            if let Some(f) = parse_percent(seg) {
                ctx.report(Progress {
                    fraction: Some(span.0 + f * (span.1 - span.0)),
                    message: Some("extraction".into()),
                    ..Default::default()
                });
            }
        };
        let res = run_tool_with(spec, ctx, &watchdog, &mut on_out, &mut |_| {});
        if exceeded.get() {
            return Err(EngineError::Refused(trf!(
                "extraction interrompue : plus de {} écrits (bombe de décompression ?)",
                "extraction stopped: more than {} written (decompression bomb?)",
                human(limit)
            )));
        }
        let out = res?;
        if !out.success() {
            let err = tail_lines(&out.stderr_tail, 3);
            if err.to_lowercase().contains("password") {
                return Err(EngineError::Refused(
                    tr!(
                        "archive protégée par mot de passe : non prise en charge pour l'instant",
                        "password-protected archive: not supported yet"
                    )
                    .into(),
                ));
            }
            return Err(EngineError::Process { code: out.code, detail: err });
        }
        Ok(())
    }

    /// Liste, valide, extrait et contrôle. Renvoie la liste des fichiers extraits.
    fn extract_checked(
        &self,
        f: &ArcFmt,
        archive: &Path,
        dest: &Path,
        o: &OptionValues,
        ctx: &ExecContext,
        notes: &mut Vec<String>,
    ) -> Result<BTreeMap<String, u64>, EngineError> {
        let limit = Self::max_unpacked(o);
        if let Shape::Layered(outer) = f.shape {
            let tmp = TempDir(unique_dir(&ctx.work_dir, "outer")?);
            self.extract_raw(archive, outer, &tmp.0, limit, ctx, (0.0, 0.4))?;
            let files = walk_files(&tmp.0).map_err(EngineError::Refused)?;
            if files.len() != 1 {
                return Err(EngineError::InvalidInput(trf!(
                    "le flux compressé contient {} fichiers au lieu d'une archive tar",
                    "the compressed stream holds {} files instead of a tar archive",
                    files.len()
                )));
            }
            let inner = tmp.0.join(files.keys().next().expect("un fichier"));
            let tar = arc("tar").expect("tar");
            return self.extract_checked(tar, &inner, dest, o, ctx, notes);
        }
        let ty = self.read_type(f, archive);
        let entries = self.list(archive, ty, Some(ctx))?;
        let total = check_entries(&entries, file_size(archive), limit)?;
        let links = entries.iter().filter(|e| e.link).count();
        if links > 0 {
            return Err(EngineError::Refused(trf!(
                "l'archive contient {links} lien(s) symbolique(s) : non extraits par sécurité dans cette version",
                "the archive holds {links} symbolic link(s): not extracted for safety in this version"
            )));
        }
        ctx.check_cancelled()?;
        self.extract_raw(archive, ty, dest, limit, ctx, (0.0, 0.6))?;
        let files =
            walk_files(dest).map_err(|e| EngineError::Refused(trf!("contenu extrait refusé : {e}", "extracted content refused: {e}")))?;
        let expected_files = entries.iter().filter(|e| !e.is_dir).count();
        let got: u64 = files.values().sum();
        let sizes_known = entries.iter().filter(|e| !e.is_dir).all(|e| e.size.is_some());
        if files.len() != expected_files || (sizes_known && got != total) {
            return Err(EngineError::Process {
                code: None,
                detail: trf!(
                    "extraction incomplète : {} fichiers ({}) au lieu de {expected_files} ({})",
                    "incomplete extraction: {} files ({}) instead of {expected_files} ({})",
                    files.len(),
                    human(got),
                    human(total)
                ),
            });
        }
        notes.push(trf!(
            "{} fichiers extraits ({}) après validation de l'index",
            "{} files extracted ({}) after checking the index",
            files.len(),
            human(got)
        ));
        Ok(files)
    }

    /// Crée l'archive `out` à partir des éléments `names` de `cwd`.
    #[allow(clippy::too_many_arguments)]
    fn pack(
        &self,
        f: &ArcFmt,
        cwd: &Path,
        names: &[OsString],
        out: &Path,
        o: &OptionValues,
        ctx: &ExecContext,
        span: (f64, f64),
    ) -> Result<(), EngineError> {
        let level = o.int_or("level", 5).clamp(0, 9);
        match f.shape {
            Shape::Layered(outer) => {
                let tmp = TempDir(unique_dir(&ctx.work_dir, "pack")?);
                let tar_path = tmp.0.join("contenu.tar");
                let middle = span.0 + (span.1 - span.0) * 0.4;
                self.pack(arc("tar").expect("tar"), cwd, names, &tar_path, o, ctx, (span.0, middle))?;
                let spec = self
                    .base_write()
                    .args(["a", "-y", "-bb0", "-bso0", "-bsp1", "-bse1"])
                    .arg(format!("-t{outer}"))
                    .arg(format!("-mx={level}"))
                    .arg(out)
                    .arg(with_prefix("@", &write_list(&tmp.0, &[OsString::from("contenu.tar")])?))
                    .cwd(&tmp.0);
                self.run_pack(spec, ctx, (middle, span.1))
            }
            Shape::Single => {
                if names.len() != 1 || !cwd.join(&names[0]).is_file() {
                    return Err(EngineError::Unsupported(
                        tr!(
                            "ce format compresse un seul fichier, pas une arborescence",
                            "this format compresses a single file, not a folder tree"
                        )
                        .into(),
                    ));
                }
                let spec = self
                    .base_write()
                    .args(["a", "-y", "-bb0", "-bso0", "-bsp1", "-bse1"])
                    .arg(format!("-t{}", f.ty))
                    .arg(format!("-mx={level}"))
                    .arg(out)
                    .arg(with_prefix("@", &write_list(&ctx.work_dir, names)?))
                    .cwd(cwd);
                self.run_pack(spec, ctx, span)
            }
            Shape::Tree => {
                let mut spec =
                    self.base_write().args(["a", "-y", "-bb0", "-bso0", "-bsp1", "-bse1", "-scsUTF-8"]).arg(format!("-t{}", f.ty));
                if f.ty != "tar" {
                    spec = spec.arg(format!("-mx={level}"));
                }
                if f.ty == "zip" {
                    spec = spec.arg("-mcu=on");
                }
                if f.ty == "7z" {
                    spec = spec.arg(if o.bool_or("solid", true) { "-ms=on" } else { "-ms=off" });
                }
                let list = write_list(&ctx.work_dir, names)?;
                let spec = spec.arg(out).arg(with_prefix("@", &list)).cwd(cwd);
                let r = self.run_pack(spec, ctx, span);
                let _ = std::fs::remove_file(&list);
                r
            }
        }
    }

    fn run_pack(&self, spec: ProcessSpec, ctx: &ExecContext, span: (f64, f64)) -> Result<(), EngineError> {
        let mut on_out = |seg: &str| {
            if let Some(f) = parse_percent(seg) {
                ctx.report(Progress {
                    fraction: Some(span.0 + f * (span.1 - span.0)),
                    message: Some("compression".into()),
                    ..Default::default()
                });
            }
        };
        let out = run_tool(spec, ctx, &mut on_out, &mut |_| {})?;
        if !out.success() {
            return Err(EngineError::Process { code: out.code, detail: tail_lines(&out.stderr_tail, 3) });
        }
        Ok(())
    }

    /// Fichiers attendus (chemin relatif → taille) pour comparer une archive produite à sa source.
    fn expected_files(&self, task: &StepTask) -> Option<BTreeMap<String, u64>> {
        if task.from.as_str() == DIR {
            let name = task.input.file_name()?.to_string_lossy().into_owned();
            let files = walk_files(&task.input).ok()?;
            return Some(files.into_iter().map(|(k, v)| (format!("{name}/{k}"), v)).collect());
        }
        let f = arc(task.from.as_str())?;
        if f.shape != Shape::Tree {
            return None;
        }
        let entries = self.list(&task.input, self.read_type(f, &task.input), None).ok()?;
        Some(entries.into_iter().filter(|e| !e.is_dir).map(|e| (norm(&e.path), e.size.unwrap_or(0))).collect())
    }
}

/// Écrit une liste de fichiers pour 7-Zip (UTF-8, un nom par ligne).
fn write_list(dir: &Path, names: &[OsString]) -> Result<PathBuf, EngineError> {
    let mut text = String::new();
    for n in names {
        let s = n.to_str().ok_or_else(|| EngineError::InvalidInput(tr!("nom de fichier non Unicode", "non-Unicode file name").into()))?;
        if s.contains('\n') || s.contains('\r') {
            return Err(EngineError::Refused(tr!("nom de fichier contenant un saut de ligne", "file name containing a line break").into()));
        }
        text.push_str(s);
        text.push('\n');
    }
    let path = unique_dir(dir, "list")?.join("liste.txt");
    std::fs::write(&path, text)?;
    Ok(path)
}

/// Fichier ou dossier à mettre à la racine d'une archive.
struct Member {
    path: PathBuf,
    /// Nom dans l'archive : celui d'origine, sauf homonyme.
    name: String,
    renamed: bool,
    is_dir: bool,
    /// Fichiers contenus : chemin relatif (vide pour un fichier seul) et taille.
    files: BTreeMap<String, u64>,
}

/// Nom libre dans l'archive : « a.txt » puis « a (2).txt ». Comparé sans tenir compte de la casse :
/// une archive doit pouvoir s'extraire sous Windows.
fn free_name(name: &str, is_dir: bool, taken: &HashSet<String>) -> String {
    if !taken.contains(&name.to_lowercase()) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => name.split_at(i),
        _ => (name, ""),
    };
    (2..).map(|n| format!("{stem} ({n}){ext}")).find(|c| !taken.contains(&c.to_lowercase())).unwrap_or_else(|| name.to_owned())
}

/// Éléments d'une archive, dans l'ordre donné. Refuse les liens, qui pourraient faire entrer des
/// fichiers extérieurs.
fn members(inputs: &[PathBuf]) -> Result<Vec<Member>, EngineError> {
    let mut taken = HashSet::new();
    let mut seen = HashSet::new();
    let mut out: Vec<Member> = Vec::new();
    for p in inputs {
        if !seen.insert(std::fs::canonicalize(p)?) {
            continue; // le même fichier, donné deux fois
        }
        let m = std::fs::symlink_metadata(p)?;
        if m.file_type().is_symlink() || is_reparse_point(&m) {
            return Err(EngineError::Refused(trf!(
                "« {} » est un lien : choisissez le fichier lui-même",
                "“{}” is a link: choose the file itself",
                p.display()
            )));
        }
        let original = p
            .file_name()
            .ok_or_else(|| EngineError::InvalidInput(trf!("« {} » n'a pas de nom", "“{}” has no name", p.display())))?
            .to_str()
            .ok_or_else(|| EngineError::InvalidInput(tr!("nom de fichier non Unicode", "non-Unicode file name").into()))?
            .to_owned();
        let files = if m.is_dir() {
            walk_files(p).map_err(|e| EngineError::Refused(trf!("« {original} » : {e}", "“{original}”: {e}")))?
        } else {
            BTreeMap::from([(String::new(), m.len())])
        };
        let name = free_name(&original, m.is_dir(), &taken);
        taken.insert(name.to_lowercase());
        out.push(Member { path: p.clone(), renamed: name != original, name, is_dir: m.is_dir(), files });
    }
    Ok(out)
}

/// Copie un élément sous un autre nom : liens physiques si possible, copie sinon.
fn link_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            link_or_copy(&e.path(), &to.join(e.file_name()))?;
        }
        return Ok(());
    }
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

fn top_level(dir: &Path) -> Result<Vec<OsString>, EngineError> {
    let mut v: Vec<OsString> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.file_name())).collect();
    v.sort();
    Ok(v)
}

impl Engine for SevenZip {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        let mut o = Vec::new();
        if let Some(f) = arc(to.as_str()) {
            if f.ty != "tar" || matches!(f.shape, Shape::Layered(_)) {
                o.push(OptionSpec::int(
                    "level",
                    tr!("Niveau de compression (0 = stockage, 9 = maximal)", "Compression level (0 = store, 9 = maximum)"),
                    0,
                    9,
                    5,
                ));
            }
            if f.ty == "7z" {
                o.push(OptionSpec::bool("solid", tr!("Archive solide (plus compacte)", "Solid archive (more compact)"), true).advanced());
            }
        }
        o.push(
            OptionSpec::int(
                "max_unpacked_gb",
                tr!("Taille décompressée maximale acceptée (Gio)", "Maximum accepted unpacked size (GiB)"),
                1,
                16384,
                64,
            )
            .advanced()
            .help(tr!("Protection contre les bombes de décompression", "Protection against decompression bombs")),
        );
        o
    }

    fn can_probe(&self, format: &FormatId) -> bool {
        arc(format.as_str()).is_some_and(|f| self.can_read(f))
    }

    fn probe(&self, path: &Path, format: &FormatId) -> Option<SourceInfo> {
        let f = arc(format.as_str())?;
        let ty = match f.shape {
            Shape::Layered(outer) => outer,
            _ => self.read_type(f, path),
        };
        let entries = self.list(path, ty, None).ok()?;
        let files = entries.iter().filter(|e| !e.is_dir).count();
        let dirs = entries.iter().filter(|e| e.is_dir).count();
        let total: u64 = entries.iter().filter_map(|e| e.size).sum();
        let encrypted = entries.iter().any(|e| e.encrypted);
        let links = entries.iter().filter(|e| e.link).count();
        let mut summary = vec![
            (tr!("Entrées", "Entries").to_string(), trf!("{files} fichier(s), {dirs} dossier(s)", "{files} file(s), {dirs} folder(s)")),
            (tr!("Taille décompressée", "Unpacked size").into(), human(total)),
            (tr!("Taille de l'archive", "Archive size").into(), human(file_size(path))),
            (tr!("Chiffrement", "Encryption").into(), if encrypted { tr!("oui", "yes") } else { tr!("non", "no") }.into()),
        ];
        if links > 0 {
            summary.push((tr!("Liens symboliques", "Symbolic links").into(), links.to_string()));
        }
        let mut feats = vec!["timestamps"];
        if dirs > 0 || entries.iter().any(|e| norm(&e.path).contains('/')) {
            feats.push("directories");
        }
        if encrypted {
            feats.push("encryption");
        }
        if links > 0 {
            feats.push("symlinks");
        }
        if entries.iter().any(|e| e.unix_mode) {
            feats.push("permissions");
        }
        Some(SourceInfo {
            // Archive tar compressée : seul le flux extérieur est visible sans extraction.
            features: if matches!(f.shape, Shape::Layered(_)) { None } else { Some(features(&feats)) },
            summary,
            ..Default::default()
        })
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let mut notes = Vec::new();
        let (from, to) = (task.from.as_str(), task.to.as_str());
        match (from, to) {
            (DIR, _) => {
                let f = arc(to).ok_or_else(|| EngineError::Unsupported(to.into()))?;
                // Un lien dans le dossier source pourrait faire entrer des fichiers extérieurs.
                let files = walk_files(&task.input).map_err(|e| {
                    EngineError::Refused(trf!(
                        "dossier source : {e} (liens non pris en charge)",
                        "source folder: {e} (links not supported)"
                    ))
                })?;
                let parent = task.input.parent().ok_or_else(|| EngineError::InvalidInput(tr!("dossier racine", "root folder").into()))?;
                let name = task
                    .input
                    .file_name()
                    .ok_or_else(|| EngineError::InvalidInput(tr!("dossier sans nom", "folder without a name").into()))?;
                self.pack(f, parent, &[name.to_os_string()], &task.output, &task.options, ctx, (0.0, 0.98))?;
                notes.push(trf!("{} fichiers compressés ({})", "{} files compressed ({})", files.len(), human(files.values().sum())));
            }
            (_, DIR) => {
                let f = arc(from).ok_or_else(|| EngineError::Unsupported(from.into()))?;
                self.extract_checked(f, &task.input, &task.output, &task.options, ctx, &mut notes)?;
            }
            _ => {
                let (Some(src), Some(dst)) = (arc(from), arc(to)) else {
                    return Err(EngineError::Unsupported(format!("{from} → {to}")));
                };
                let tmp = TempDir(unique_dir(&ctx.work_dir, "contenu")?);
                self.extract_checked(src, &task.input, &tmp.0, &task.options, ctx, &mut notes)?;
                ctx.check_cancelled()?;
                let names = top_level(&tmp.0)?;
                if names.is_empty() {
                    return Err(EngineError::InvalidInput("archive vide".into()));
                }
                self.pack(dst, &tmp.0, &names, &task.output, &task.options, ctx, (0.6, 0.98))?;
                notes.push(trf!("contenu recompressé au format {to}", "content recompressed as {to}"));
            }
        }
        ctx.report(Progress { fraction: Some(1.0), ..Default::default() });
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        if task.to.as_str() == DIR {
            return match walk_files(output) {
                Ok(files) if files.is_empty() && task.from.as_str() != DIR => {
                    Verification::Failed(tr!("aucun fichier extrait", "no file extracted").into())
                }
                Ok(files) => Verification::Structural(trf!(
                    "{} fichiers ({}) conformes à l'index de l'archive, aucun lien",
                    "{} files ({}) match the archive index, no links",
                    files.len(),
                    human(files.values().sum())
                )),
                Err(e) => Verification::Failed(e),
            };
        }
        let Some(f) = arc(task.to.as_str()) else { return Verification::SignatureOnly };
        let ty = match f.shape {
            Shape::Layered(outer) => outer,
            _ => f.ty,
        };
        let spec = self.base_read().args(["t", "-bb0", "-bso0", "-bsp0", "-bse1"]).arg(format!("-t{ty}")).arg("--").arg(output);
        match run_tool(spec, ctx, &mut |_| {}, &mut |_| {}) {
            Ok(o) if o.success() => {}
            Ok(o) => {
                return Verification::Failed(trf!(
                    "test d'intégrité échoué : {}",
                    "integrity test failed: {}",
                    tail_lines(&o.stderr_tail, 3)
                ));
            }
            Err(e) => return Verification::Failed(trf!("test d'intégrité impossible : {e}", "integrity test impossible: {e}")),
        }
        if f.shape != Shape::Tree {
            return Verification::Decoded(
                tr!(
                    "test d'intégrité 7-Zip réussi (données décompressées, sommes de contrôle vérifiées)",
                    "7-Zip integrity test passed (data unpacked, checksums verified)"
                )
                .into(),
            );
        }
        let Some(expected) = self.expected_files(task) else {
            return Verification::Decoded(
                tr!(
                    "test d'intégrité 7-Zip réussi (données décompressées, sommes de contrôle vérifiées)",
                    "7-Zip integrity test passed (data unpacked, checksums verified)"
                )
                .into(),
            );
        };
        let produced: BTreeMap<String, u64> = match self.list(output, f.ty, Some(ctx)) {
            Ok(entries) => entries.into_iter().filter(|e| !e.is_dir).map(|e| (norm(&e.path), e.size.unwrap_or(0))).collect(),
            Err(e) => return Verification::Failed(trf!("relecture de l'index impossible : {e}", "cannot read the index back: {e}")),
        };
        if produced != expected {
            let missing: Vec<&String> = expected.keys().filter(|k| !produced.contains_key(*k)).take(3).collect();
            return Verification::Failed(trf!(
                "contenu différent de la source ({} fichiers attendus, {} produits ; absents : {:?})",
                "content differs from the source ({} files expected, {} produced; missing: {:?})",
                expected.len(),
                produced.len(),
                missing
            ));
        }
        Verification::Decoded(trf!(
            "test d'intégrité réussi et {} fichiers identiques en nom et en taille à la source",
            "integrity test passed and {} files identical in name and size to the source",
            produced.len()
        ))
    }

    fn operations(&self) -> &[&'static str] {
        &[ARCHIVE_CREATE]
    }

    fn operation_options(&self, operation: &str) -> Vec<OptionSpec> {
        if operation != ARCHIVE_CREATE {
            return Vec::new();
        }
        let catalog = FormatCatalog::builtin();
        let choices: Vec<(&str, String)> = ARCHIVE_TARGETS
            .iter()
            .filter_map(|id| arc(id).filter(|f| self.can_write(f)))
            .map(|f| {
                let def = catalog.get(f.id);
                let ext = def.and_then(|d| d.output_extension()).unwrap_or(f.id);
                (f.id, format!("{} (.{ext})", def.map_or(f.id, |d| d.label())))
            })
            .collect();
        let choices: Vec<(&str, &str)> = choices.iter().map(|(id, l)| (*id, l.as_str())).collect();
        vec![
            OptionSpec::choice("format", tr!("Format de l'archive", "Archive format"), &choices, choices.first().map_or("zip", |c| c.0)),
            OptionSpec::int(
                "level",
                tr!("Niveau de compression (0 = stockage, 9 = maximal)", "Compression level (0 = store, 9 = maximum)"),
                0,
                9,
                5,
            ),
            OptionSpec::bool("solid", tr!("7z : archive solide (plus compacte)", "7z: solid archive (more compact)"), true).advanced(),
        ]
    }

    fn operation_target(&self, operation: &str, options: &OptionValues) -> Result<FormatId, EngineError> {
        let f = arc(options.str_or("format", "zip")).filter(|f| ARCHIVE_TARGETS.contains(&f.id) && self.can_write(f));
        match f {
            Some(f) if operation == ARCHIVE_CREATE => Ok(FormatId::new(f.id)),
            _ => Err(EngineError::Unsupported(operation.into())),
        }
    }

    fn assess_operation(&self, operation: &str, inputs: &[PathBuf], options: &OptionValues) -> Result<OperationReport, EngineError> {
        self.operation_target(operation, options)?;
        let list = members(inputs)?;
        let files: usize = list.iter().map(|m| m.files.len()).sum();
        let size: u64 = list.iter().flat_map(|m| m.files.values()).sum();
        if files == 0 {
            return Err(EngineError::InvalidInput(tr!("aucun fichier à archiver", "no file to archive").into()));
        }
        let mut r = OperationReport::default();
        r.notes.push(trf!("{files} fichier(s), {}", "{files} file(s), {}", human(size)));
        r.notes.push(
            tr!(
                "contenus, noms, dossiers et dates conservés ; les droits d'accès propres à ce système ne sont pas repris",
                "contents, names, folders and dates are kept; access rights specific to this system are not"
            )
            .into(),
        );
        let renamed: Vec<&Member> = list.iter().filter(|m| m.renamed).collect();
        for m in renamed.iter().take(3) {
            r.notes.push(trf!(
                "deux éléments portent le même nom : « {} » devient « {} » dans l'archive",
                "two items have the same name: “{}” becomes “{}” in the archive",
                m.path.display(),
                m.name
            ));
        }
        if renamed.len() > 3 {
            r.notes.push(trf!("… et {} autre(s) renommé(s)", "… and {} more renamed", renamed.len() - 3));
        }
        Ok(r)
    }

    fn execute_operation(&self, task: &OperationTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let target = self.operation_target(&task.operation, &task.options)?;
        let f = arc(target.as_str()).ok_or_else(|| EngineError::Unsupported(target.to_string()))?;
        let report = self.assess_operation(&task.operation, &task.inputs, &task.options)?;
        let tmp = TempDir(unique_dir(&ctx.work_dir, "noms")?);
        let mut names = Vec::new();
        for (i, m) in members(&task.inputs)?.iter().enumerate() {
            ctx.check_cancelled()?;
            // 7-Zip enregistre le dernier élément d'un chemin absolu ; un homonyme est préparé à
            // part sous son nouveau nom.
            let path = if m.renamed {
                let dir = tmp.0.join(i.to_string());
                std::fs::create_dir_all(&dir)?;
                let to = dir.join(&m.name);
                link_or_copy(&m.path, &to)?;
                to
            } else {
                m.path.clone()
            };
            names.push(std::path::absolute(path)?.into_os_string());
        }
        self.pack(f, &ctx.work_dir, &names, &task.output, &task.options, ctx, (0.0, 0.98))?;
        ctx.report(Progress { fraction: Some(1.0), ..Default::default() });
        Ok(StepOutput { notes: report.notes })
    }

    fn verify_operation(&self, task: &OperationTask, ctx: &ExecContext) -> Verification {
        let Some(f) = arc(task.target.as_str()) else { return Verification::SignatureOnly };
        let expected: BTreeMap<String, u64> = match members(&task.inputs) {
            Ok(list) => list
                .into_iter()
                .flat_map(|m| {
                    let Member { name, is_dir, files, .. } = m;
                    files.into_iter().map(move |(rel, n)| (if is_dir { format!("{name}/{rel}") } else { name.clone() }, n))
                })
                .collect(),
            Err(e) => return Verification::Failed(e.to_string()),
        };
        // Une archive tar compressée est relue en entier : son index est dans le flux.
        let _outer;
        let (file, ty) = match f.shape {
            Shape::Layered(outer) => {
                let tmp = match unique_dir(&ctx.work_dir, "relecture") {
                    Ok(d) => TempDir(d),
                    Err(e) => return Verification::Failed(e.to_string()),
                };
                if let Err(e) = self.extract_raw(&task.output, outer, &tmp.0, u64::MAX, ctx, (0.98, 0.99)) {
                    return Verification::Failed(e.to_string());
                }
                let Ok(files) = walk_files(&tmp.0) else {
                    return Verification::Failed(tr!("relecture impossible", "cannot read back").into());
                };
                let [name] = files.keys().collect::<Vec<_>>()[..] else {
                    return Verification::Failed(tr!("flux compressé inattendu", "unexpected compressed stream").into());
                };
                let p = tmp.0.join(name);
                _outer = tmp; // gardé jusqu'à la fin de la relecture
                (p, "tar")
            }
            _ => (task.output.clone(), f.ty),
        };
        let spec = self.base_read().args(["t", "-bb0", "-bso0", "-bsp0", "-bse1"]).arg(format!("-t{ty}")).arg("--").arg(&file);
        match run_tool(spec, ctx, &mut |_| {}, &mut |_| {}) {
            Ok(o) if o.success() => {}
            Ok(o) => {
                return Verification::Failed(trf!(
                    "test d'intégrité échoué : {}",
                    "integrity test failed: {}",
                    tail_lines(&o.stderr_tail, 3)
                ));
            }
            Err(e) => return Verification::Failed(trf!("test d'intégrité impossible : {e}", "integrity test impossible: {e}")),
        }
        let produced: BTreeMap<String, u64> = match self.list(&file, ty, Some(ctx)) {
            Ok(entries) => entries.into_iter().filter(|e| !e.is_dir).map(|e| (norm(&e.path), e.size.unwrap_or(0))).collect(),
            Err(e) => return Verification::Failed(trf!("relecture de l'index impossible : {e}", "cannot read the index back: {e}")),
        };
        if produced != expected {
            let missing: Vec<&String> = expected.keys().filter(|k| !produced.contains_key(*k)).take(3).collect();
            return Verification::Failed(trf!(
                "contenu différent des fichiers choisis ({} attendus, {} produits ; absents : {:?})",
                "content differs from the chosen files ({} expected, {} produced; missing: {:?})",
                expected.len(),
                produced.len(),
                missing
            ));
        }
        Verification::Decoded(trf!(
            "test d'intégrité réussi et {} fichiers identiques en nom et en taille aux originaux",
            "integrity test passed and {} files identical in name and size to the originals",
            produced.len()
        ))
    }

    fn weight(&self, _from: &FormatId, _to: &FormatId) -> u32 {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_format_table() {
        let t = "Formats:\n 0 C...F..........c.a.m+.. w...0  7z       7z            7 z BC AF\n 0  ......................  Rar      rar r00       R a r\n 0 CK.................m+.. .u..1  gzip     gz gzip tgz (.tar)\n 0  K.....................  zstd     zst tzst (.tar)\n\nCodecs:\n";
        let (r, c) = parse_formats(t);
        assert!(r.contains("7z") && r.contains("rar") && r.contains("gzip") && r.contains("zstd"));
        assert!(c.contains("7z") && c.contains("gzip"));
        assert!(!c.contains("rar") && !c.contains("zstd"));
    }

    #[test]
    fn parses_listing() {
        let t = "Path = d\nFolder = +\nSize = 0\nAttributes = D\n\nPath = d\\a.txt\nFolder = -\nSize = 6\nAttributes = A\nEncrypted = -\n\nPath = lien\nSize = 4\nAttributes = A lrwxrwxrwx\n";
        let e = parse_listing(t);
        assert_eq!(e.len(), 3);
        assert!(e[0].is_dir && !e[1].is_dir && e[1].size == Some(6));
        assert!(e[2].link && e[2].unix_mode);
    }

    #[test]
    fn parses_nameless_stream_and_crlf() {
        // xz, bzip2, zstd : pas de ligne « Path » ; sortie Windows en CRLF.
        let e = parse_listing("Size = 520\r\nPacked Size = 96\r\nMethod = LZMA2:12 CRC32\r\n\r\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].size, Some(520));
        assert!(e[0].path.is_empty(), "nom fourni ensuite par `list` d'après l'archive");
        let e = parse_listing("Size = \r\nPacked Size = \r\n\r\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].size, None);
        let e = parse_listing("Path = é.txt\r\nSize = 4\r\n");
        assert_eq!(e[0].path, "é.txt", "sans retour chariot final");
    }

    #[test]
    fn rejects_dangerous_paths() {
        for bad in ["../evil", "a/../../evil", "/etc/passwd", "C:/Windows/x", "a\\..\\..\\b", "x\u{1}y"] {
            assert!(check_entry_path(bad).is_err(), "{bad} devrait être refusé");
        }
        if cfg!(windows) {
            for bad in ["CON", "dir/nul.txt", "a:stream", "trailing.", "COM1.log", "q?.txt"] {
                assert!(check_entry_path(bad).is_err(), "{bad} devrait être refusé sous Windows");
            }
        }
        for ok in ["a.txt", "dir/sub/é 文件.txt", "..hidden", "a..b", "-dash.txt"] {
            assert!(check_entry_path(ok).is_ok(), "{ok} devrait être accepté");
        }
    }

    #[test]
    fn rejects_bombs_and_duplicates() {
        let big = Entry { path: "big.bin".into(), size: Some(50 << 30), ..Default::default() };
        assert!(check_entries(std::slice::from_ref(&big), 1 << 20, 64 << 30).is_err(), "ratio anormal");
        assert!(check_entries(&[big], 40 << 30, 32 << 30).is_err(), "au-delà de la limite");
        let d = |p: &str| Entry { path: p.into(), size: Some(1), ..Default::default() };
        assert!(check_entries(&[d("a.txt"), d("dir/../a.txt")], 10, 1 << 30).is_err());
        assert!(check_entries(&[d("a.txt"), d("b.txt")], 10, 1 << 30).is_ok());
        let enc = Entry { encrypted: true, ..d("s.txt") };
        assert!(matches!(check_entries(&[enc], 10, 1 << 30), Err(EngineError::Refused(_))));
    }

    #[test]
    fn archive_names_never_clash() {
        let taken: HashSet<String> = ["a.txt", "a (2).txt", "photos"].map(String::from).into();
        assert_eq!(free_name("b.txt", false, &taken), "b.txt");
        assert_eq!(free_name("A.TXT", false, &taken), "A (2).TXT".replace("(2)", "(3)"));
        assert_eq!(free_name("Photos", true, &taken), "Photos (2)");
        assert_eq!(free_name(".profil", false, &HashSet::from([".profil".to_owned()])), ".profil (2)");
    }

    #[test]
    fn progress_parsing() {
        assert_eq!(parse_percent("  45% 3 - fichier"), Some(0.45));
        assert_eq!(parse_percent("100%"), Some(1.0));
        assert_eq!(parse_percent("abc"), None);
    }
}
