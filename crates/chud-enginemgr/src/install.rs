//! Installation gérée des moteurs (docs/ENGINE-MANAGER.md).
//!
//! Machine à états d'une installation, toujours dans cet ordre, sans retour en arrière silencieux :
//!
//! ```text
//! téléchargement ──► vérification SHA-256 ──► extraction contrôlée ──► essai réel ──► activation
//!      │                    │                        │                     │
//!      └────────────────────┴──── échec / annulation : tout est retiré, rien n'est activé ──┘
//! ```
//!
//! - Chaque fichier vient d'une adresse HTTPS figée dans le manifeste, avec sa taille et son
//!   empreinte SHA-256 : un fichier différent d'un seul octet est refusé avant d'être ouvert.
//! - L'extraction se fait dans `versions/<v>.partial` : pas de chemin qui sort du dossier, pas de
//!   lien, taille et nombre de fichiers limités. Le dossier n'est renommé qu'une fois l'exécutable
//!   essayé avec succès.
//! - `current` est remplacé en une seule opération ; la version précédente est gardée pour revenir
//!   en arrière. Les installations du système ne sont jamais touchées.

use crate::{AppPaths, Manifest, exe_name, find_exe, probe_version};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Taille totale extraite maximale (LibreOffice et Calibre font environ 1 Go une fois extraits).
const MAX_EXTRACTED: u64 = 4 * 1024 * 1024 * 1024;
/// Nombre maximal d'éléments dans une archive.
const MAX_ENTRIES: usize = 80_000;

/// Comment le paquet téléchargé devient un dossier.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMethod {
    /// Une archive ZIP (FFmpeg pour Windows).
    Zip,
    /// Une archive `.tar.xz` (7-Zip pour Linux).
    TarXz,
    /// Une archive `.tar.gz` (Pandoc pour Linux).
    TarGz,
    /// L'installeur auto-extractible de 7-Zip, ouvert par `7zr` (deux fichiers : `7zr`, puis l'installeur).
    /// L'installeur lui-même n'est jamais lancé.
    SevenzipSfx,
    /// Une archive `.7z`, ouverte par `7zr` (deux fichiers : `7zr`, puis l'archive). Windows.
    Sevenzip,
    /// Un installeur NSIS ouvert comme une archive par 7-Zip, jamais exécuté (trois fichiers :
    /// `7zr`, l'installeur de 7-Zip qui fournit `7z` complet, puis l'installeur NSIS). Windows.
    Nsis,
    /// Un paquet Windows Installer extrait par `msiexec /a` (installation « administrative » :
    /// copie des fichiers dans le dossier de Chud Converter, rien n'est enregistré sur le
    /// système, aucun droit administrateur). Windows.
    Msi,
}

impl InstallMethod {
    fn file_count(self) -> usize {
        match self {
            InstallMethod::Zip | InstallMethod::TarXz | InstallMethod::TarGz | InstallMethod::Msi => 1,
            InstallMethod::SevenzipSfx | InstallMethod::Sevenzip => 2,
            InstallMethod::Nsis => 3,
        }
    }

    fn windows_only(self) -> bool {
        matches!(self, InstallMethod::SevenzipSfx | InstallMethod::Sevenzip | InstallMethod::Nsis | InstallMethod::Msi)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Fichier de données ajouté après extraction (ex. langues de Tesseract), vérifié comme les autres.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// Chemin relatif, dans le dossier de la version.
    pub dest: String,
}

impl Extra {
    fn download(&self) -> Download {
        Download { url: self.url.clone(), sha256: self.sha256.clone(), size: self.size }
    }
}

fn check_download(f: &Download) -> Result<(), String> {
    if !f.url.starts_with("https://") {
        return Err(trf!("adresse non HTTPS : {}", "non-HTTPS address: {}", f.url));
    }
    if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(trf!("empreinte SHA-256 invalide pour {}", "invalid SHA-256 for {}", f.url));
    }
    if f.size == 0 {
        return Err(trf!("taille manquante pour {}", "missing size for {}", f.url));
    }
    Ok(())
}

/// Nom de fichier simple (sans dossier ni caractère réservé).
fn plain_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

/// Source d'installation d'un moteur pour une plateforme (section `[[install]]` du manifeste).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallSource {
    /// `windows-x86_64`, `linux-x86_64`…
    pub platform: String,
    pub version: String,
    pub method: InstallMethod,
    /// Qui fabrique ce paquet.
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_en: Option<String>,
    /// Licence de ce paquet précis (si elle diffère de celle du moteur).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Page de la publication, pour vérifier par soi-même.
    pub page: String,
    pub files: Vec<Download>,
    /// Fichiers de données ajoutés après extraction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<Extra>,
    /// Renommages après extraction : `[nom trouvé dans l'arborescence, nouveau nom]`
    /// (ExifTool pour Windows s'appelle `exiftool(-k).exe` dans son archive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renames: Vec<[String; 2]>,
    /// Fichiers recopiés d'un dossier vers un autre après extraction : `[source, destination]`
    /// (LibreOffice range son runtime Visual C++ dans `System64`, à côté de `program` il suffit).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<[String; 2]>,
    /// Éléments retirés à la racine après extraction (installeurs embarqués, désinstalleurs…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    /// Le moteur a besoin du runtime Visual C++ de Microsoft (Windows) : installé s'il manque.
    #[serde(default)]
    pub vcruntime: bool,
}

impl InstallSource {
    pub fn publisher_l(&self) -> &str {
        match (chud_core::lang(), &self.publisher_en) {
            (chud_core::Lang::En, Some(en)) => en,
            _ => &self.publisher,
        }
    }

    /// Taille à télécharger, runtime Visual C++ compris s'il manque.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum::<u64>()
            + self.extras.iter().map(|f| f.size).sum::<u64>()
            + if self.needs_runtime() { vc_redist().size } else { 0 }
    }

    /// Le runtime Visual C++ sera installé avec ce moteur (il manque sur ce système).
    pub fn needs_runtime(&self) -> bool {
        self.vcruntime && cfg!(windows) && !vcruntime_present()
    }

    /// Contrôles faits au chargement du manifeste.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !safe_version(&self.version) {
            return Err(trf!("version d'installation invalide : {}", "invalid install version: {}", self.version));
        }
        let expected = self.method.file_count();
        if self.files.len() != expected {
            return Err(trf!("{} fichier(s) attendu(s) pour cette méthode", "{} file(s) expected for this method", expected));
        }
        if self.method.windows_only() && !self.platform.starts_with("windows-") {
            return Err(tr!("cette méthode d'installation n'existe que sous Windows", "this install method only exists on Windows").into());
        }
        if self.vcruntime && !self.platform.starts_with("windows-") {
            return Err(tr!("le runtime Visual C++ ne concerne que Windows", "the Visual C++ runtime only concerns Windows").into());
        }
        for f in &self.files {
            check_download(f)?;
        }
        for e in &self.extras {
            check_download(&e.download())?;
            if safe_relative(Path::new(&e.dest)).is_none() || !e.dest.split(['/', '\\']).all(plain_name) {
                return Err(trf!("destination refusée : {}", "destination refused: {}", e.dest));
            }
        }
        for [from, to] in &self.renames {
            if !plain_name(from) || !plain_name(to) {
                return Err(trf!("renommage refusé : {} → {}", "rename refused: {} → {}", from, to));
            }
        }
        for pair in &self.copies {
            if !pair.iter().all(|d| safe_relative(Path::new(d)).is_some() && d.split(['/', '\\']).all(plain_name)) {
                return Err(trf!("copie refusée : {} → {}", "copy refused: {} → {}", pair[0], pair[1]));
            }
        }
        if !self.remove.iter().all(|n| plain_name(n)) {
            return Err(tr!("élément à retirer invalide", "invalid item to remove").into());
        }
        if !self.page.starts_with("https://") {
            return Err(tr!("la page de publication doit être en HTTPS", "the release page must use HTTPS").into());
        }
        Ok(())
    }
}

/// Plateforme courante, au format des manifestes.
pub fn current_platform() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

fn safe_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 64 && v != "." && !v.contains("..") && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}

/// Étape en cours, envoyée à l'interface pendant l'installation.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum InstallProgress {
    /// `received` / `total` : octets de l'ensemble des fichiers.
    Downloading {
        received: u64,
        total: u64,
    },
    Verifying,
    /// `done` / `total` : avancement estimé (unités arbitraires) ; `total = 0` : inconnu.
    Extracting {
        done: u64,
        total: u64,
    },
    /// Installation du runtime Visual C++ de Microsoft (Windows peut demander une autorisation).
    Runtime,
    Checking,
    /// Gestionnaire de paquets du système : dernière ligne affichée.
    System {
        line: String,
    },
}

#[derive(Debug)]
pub enum InstallError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Cancelled => f.write_str(tr!("installation annulée", "installation cancelled")),
            InstallError::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for InstallError {
    fn from(e: String) -> Self {
        InstallError::Failed(e)
    }
}

fn io_err(what: &str, e: std::io::Error) -> InstallError {
    InstallError::Failed(format!("{what} : {e}"))
}

/// Ce qui est noté dans `<engines>/<id>/install.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct InstallRecord {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    pub installed_at: u64,
    pub publisher: String,
    pub files: Vec<Download>,
}

fn record_path(root: &Path) -> PathBuf {
    root.join("install.json")
}

/// Installation gérée existante d'un moteur, s'il y en a une.
pub fn record(paths: &AppPaths, id: &str) -> Option<InstallRecord> {
    let text = std::fs::read_to_string(record_path(&paths.engines_dir().join(id))).ok()?;
    serde_json::from_str(&text).ok()
}

/// Version précédente encore présente sur le disque (retour en arrière possible).
pub fn previous_version(paths: &AppPaths, id: &str) -> Option<String> {
    let r = record(paths, id)?;
    let prev = r.previous?;
    (safe_version(&prev) && paths.engines_dir().join(id).join("versions").join(&prev).is_dir()).then_some(prev)
}

/// Verrou d'installation : un seul processus à la fois installe un moteur donné.
struct Lock(PathBuf);

impl Lock {
    fn take(root: &Path) -> Result<Self, InstallError> {
        std::fs::create_dir_all(root).map_err(|e| io_err(tr!("création du dossier du moteur", "creating the engine folder"), e))?;
        let p = root.join(".installing");
        // Un verrou de plus d'une heure vient d'un programme arrêté brutalement.
        if let Ok(m) = std::fs::metadata(&p)
            && m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(3600))
        {
            let _ = std::fs::remove_file(&p);
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(_) => Ok(Lock(p)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(tr!("une installation de ce moteur est déjà en cours", "this engine is already being installed").to_owned().into())
            }
            Err(e) => Err(io_err(tr!("verrou d'installation", "installation lock"), e)),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Résultat d'une installation réussie.
#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    pub version: String,
    pub path: PathBuf,
    pub previous: Option<String>,
}

/// Règle commune (application et ligne de commande) : peut-on installer une copie gérée ?
///
/// - absent, en panne ou déjà géré : oui ;
/// - prêt depuis le système : seulement sur demande explicite (`alongside`) — la copie gérée est
///   installée **à côté** et utilisée en priorité ; l'installation du système n'est pas touchée et
///   redevient active si la copie est retirée ;
/// - prêt depuis un chemin choisi par l'utilisateur : non (ce choix passe avant toute copie gérée).
pub fn check_allowed(status: &crate::EngineStatus, alongside: bool) -> Result<(), String> {
    use chud_core::EngineOrigin;
    if status.state != crate::EngineState::Ready {
        return Ok(());
    }
    let path = status.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    match status.origin {
        Some(EngineOrigin::Managed) => Ok(()),
        // Rien de portable à installer à côté (voie « paquets du système ») : il est déjà là.
        _ if status.installable.is_none() => {
            Err(trf!("{} est déjà installé par le système ({})", "{} is already installed by the system ({})", status.name, path))
        }
        Some(EngineOrigin::Custom) => Err(trf!(
            "{} utilise le chemin que vous avez choisi ({}) : une copie installée par Chud Converter ne serait pas utilisée",
            "{} uses the path you chose ({}): a copy installed by Chud Converter would not be used",
            status.name,
            path
        )),
        _ if alongside => Ok(()),
        _ => Err(trf!(
            "{} est déjà prêt ({}). Chud Converter ne remplace pas une installation existante ; pour utiliser sa propre copie à côté, demandez-le explicitement",
            "{} is already ready ({}). Chud Converter does not replace an existing install; to use its own copy alongside, ask for it explicitly",
            status.name,
            path
        )),
    }
}

/// Installe (ou met à jour) un moteur depuis la source de son manifeste pour cette plateforme.
pub fn install(
    paths: &AppPaths,
    m: &Manifest,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<Installed, InstallError> {
    if m.install_source().is_none() && crate::system::route(m).is_some() {
        return crate::system::install(paths, m, cancel, progress);
    }
    let src = m.install_source().ok_or_else(|| {
        InstallError::Failed(trf!(
            "{} ne peut pas être installé automatiquement sur cette plateforme ({})",
            "{} cannot be installed automatically on this platform ({})",
            m.name,
            current_platform()
        ))
    })?;
    let root = paths.engines_dir().join(&m.id);
    let _lock = Lock::take(&root)?;
    let downloads = root.join("downloads");
    let versions = root.join("versions");
    let partial = versions.join(format!("{}.partial", src.version));
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&downloads);
        let _ = std::fs::remove_dir_all(&partial);
    };
    cleanup();
    let result = (|| {
        let runtime = src.needs_runtime();
        let got = download_all(src, runtime, &downloads, cancel, progress)?;
        let files = &got.files;
        progress(InstallProgress::Extracting { done: 0, total: 0 });
        std::fs::create_dir_all(&partial).map_err(|e| io_err(tr!("création du dossier de version", "creating the version folder"), e))?;
        let mut report = |done: u64, total: u64| progress(InstallProgress::Extracting { done, total });
        match src.method {
            InstallMethod::Zip => extract_zip(&files[0], &partial, cancel, &mut report)?,
            InstallMethod::TarXz => extract_tar(&files[0], Compression::Xz, &partial, cancel, &mut report)?,
            InstallMethod::TarGz => extract_tar(&files[0], Compression::Gz, &partial, cancel, &mut report)?,
            InstallMethod::SevenzipSfx | InstallMethod::Sevenzip => extract_with_7zr(&files[0], &files[1], &partial)?,
            InstallMethod::Nsis => extract_nsis(&files[0], &files[1], &files[2], &downloads, &partial)?,
            InstallMethod::Msi => extract_msi_short(&files[0], &partial)?,
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        finish_tree(src, &partial, &got.extras)?;
        let deepest = check_tree(&partial)?;
        check_path_length(&versions.join(&src.version), deepest, &m.name)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        if let Some(redist) = &got.runtime {
            progress(InstallProgress::Runtime);
            install_vcruntime(redist)?;
        }
        progress(InstallProgress::Checking);
        let exe = check_executables(m, &partial)?;
        activate(src, &root, &partial, &exe)
    })();
    cleanup();
    result
}

/// Fichiers téléchargés et vérifiés, rangés par rôle.
struct Downloaded {
    files: Vec<PathBuf>,
    extras: Vec<(PathBuf, String)>,
    runtime: Option<PathBuf>,
}

fn download_all(
    src: &InstallSource,
    runtime: bool,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<Downloaded, InstallError> {
    std::fs::create_dir_all(dir).map_err(|e| io_err(tr!("création du dossier de téléchargement", "creating the download folder"), e))?;
    // Pile TLS : rustls avec « ring » (comme la mise à jour). Déjà installée par ailleurs : sans effet.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("chud-converter/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(3600))
        // Redirections (GitHub renvoie vers son stockage) : HTTPS uniquement.
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() != "https" {
                a.error("redirection non HTTPS refusée")
            } else if a.previous().len() > 8 {
                a.error("trop de redirections")
            } else {
                a.follow()
            }
        }))
        .build()
        .map_err(|e| InstallError::Failed(trf!("client HTTPS : {}", "HTTPS client: {}", e)))?;
    let mut list: Vec<Download> = src.files.clone();
    list.extend(src.extras.iter().map(Extra::download));
    if runtime {
        list.push(vc_redist());
    }
    let total: u64 = list.iter().map(|f| f.size).sum();
    let mut received_before = 0;
    let mut paths = Vec::new();
    for (i, f) in list.iter().enumerate() {
        let name = f.url.rsplit('/').next().map(|n| n.split('?').next().unwrap_or(n)).filter(|n| plain_name(n)).unwrap_or("download");
        let path = dir.join(format!("{i}-{name}"));
        download(&client, f, &path, cancel, &mut |n| progress(InstallProgress::Downloading { received: received_before + n, total }))?;
        received_before += f.size;
        paths.push(path);
    }
    progress(InstallProgress::Verifying);
    let mut rest = paths.split_off(src.files.len());
    let runtime = if runtime { rest.pop() } else { None };
    let extras = rest.into_iter().zip(src.extras.iter().map(|e| e.dest.clone())).collect();
    Ok(Downloaded { files: paths, extras, runtime })
}

/// Télécharge un fichier en calculant son empreinte au fil de l'eau ; refuse tout écart.
fn download(
    client: &reqwest::blocking::Client,
    f: &Download,
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), InstallError> {
    // Coupure réseau ou serveur momentanément indisponible (5xx) : deux nouveaux essais espacés
    // (les miroirs de SourceForge échouent parfois). Une réponse 4xx ne change pas : pas d'essai.
    let mut attempt = 0;
    let mut resp = loop {
        attempt += 1;
        let r = client.get(&f.url).send();
        let retry = match &r {
            Ok(resp) => resp.status().is_server_error(),
            Err(e) => !e.is_builder() && !e.is_redirect(),
        };
        if !retry || attempt >= 3 {
            break r
                .map_err(|e| InstallError::Failed(trf!("téléchargement impossible ({}) : {}", "download failed ({}): {}", f.url, e)))?;
        }
        for _ in 0..(attempt * 20) {
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    if !resp.status().is_success() {
        return Err(InstallError::Failed(trf!("le serveur a répondu {} pour {}", "the server answered {} for {}", resp.status(), f.url)));
    }
    let mut file = std::fs::File::create(path).map_err(|e| io_err(tr!("écriture du téléchargement", "writing the download"), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut n: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let k =
            resp.read(&mut buf).map_err(|e| InstallError::Failed(trf!("téléchargement interrompu : {}", "download interrupted: {}", e)))?;
        if k == 0 {
            break;
        }
        n += k as u64;
        if n > f.size {
            return Err(InstallError::Failed(trf!(
                "le fichier reçu dépasse la taille attendue ({} octets) : refusé",
                "the received file is larger than expected ({} bytes): refused",
                f.size
            )));
        }
        hasher.update(&buf[..k]);
        file.write_all(&buf[..k]).map_err(|e| io_err(tr!("écriture du téléchargement", "writing the download"), e))?;
        progress(n);
    }
    file.flush().map_err(|e| io_err(tr!("écriture du téléchargement", "writing the download"), e))?;
    if n != f.size {
        return Err(InstallError::Failed(trf!(
            "fichier incomplet : {} octets reçus sur {}",
            "incomplete file: {} of {} bytes received",
            n,
            f.size
        )));
    }
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != f.sha256 {
        return Err(InstallError::Failed(trf!(
            "empreinte SHA-256 différente de celle attendue pour {} : fichier refusé, rien n'est installé",
            "SHA-256 differs from the expected one for {}: file refused, nothing is installed",
            f.url
        )));
    }
    Ok(())
}

/// Chemin relatif sûr : uniquement des composants « normaux ».
fn safe_relative(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

struct Budget {
    bytes: u64,
    entries: usize,
}

impl Budget {
    fn new() -> Self {
        Budget { bytes: 0, entries: 0 }
    }

    fn entry(&mut self) -> Result<(), InstallError> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(tr!("archive refusée : trop de fichiers", "archive refused: too many files").to_owned().into());
        }
        Ok(())
    }

    fn add(&mut self, n: u64) -> Result<(), InstallError> {
        self.bytes += n;
        if self.bytes > MAX_EXTRACTED {
            return Err(tr!("archive refusée : contenu trop volumineux", "archive refused: content too large").to_owned().into());
        }
        Ok(())
    }

    /// Copie bornée : l'archive ne peut pas dépasser la taille totale autorisée.
    fn copy(&mut self, from: &mut dyn Read, to: &Path) -> Result<(), InstallError> {
        let mut file = std::fs::File::create(to).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
        let left = MAX_EXTRACTED - self.bytes;
        let n = std::io::copy(&mut from.take(left + 1), &mut file).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
        self.bytes += n;
        if self.bytes > MAX_EXTRACTED {
            return Err(tr!("archive refusée : contenu trop volumineux", "archive refused: content too large").to_owned().into());
        }
        Ok(())
    }
}

/// Avancement d'une extraction : (fait, total).
type Report<'a> = &'a mut dyn FnMut(u64, u64);

/// Lecteur qui compte les octets lus (avancement de la décompression).
struct Counting<'a, R> {
    inner: R,
    n: u64,
    total: u64,
    report: Report<'a>,
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.n += k as u64;
        (self.report)(self.n, self.total);
        Ok(k)
    }
}

fn extract_zip(archive: &Path, dest: &Path, cancel: &AtomicBool, report: Report) -> Result<(), InstallError> {
    let f = std::fs::File::open(archive).map_err(|e| io_err(tr!("ouverture de l'archive", "opening the archive"), e))?;
    let mut zip =
        zip::ZipArchive::new(f).map_err(|e| InstallError::Failed(trf!("archive ZIP illisible : {}", "unreadable ZIP archive: {}", e)))?;
    let mut budget = Budget::new();
    let mut links: Vec<(PathBuf, PathBuf)> = Vec::new();
    let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size())).sum();
    for i in 0..zip.len() {
        report(budget.bytes, total);
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        budget.entry()?;
        let mut entry =
            zip.by_index(i).map_err(|e| InstallError::Failed(trf!("archive ZIP illisible : {}", "unreadable ZIP archive: {}", e)))?;
        let Some(rel) = entry.enclosed_name().and_then(|p| safe_relative(&p)) else {
            return Err(InstallError::Failed(trf!(
                "chemin refusé dans l'archive : {}",
                "path refused in the archive: {}",
                entry.name().unwrap_or_default()
            )));
        };
        if entry.is_symlink() {
            // Le contenu d'un lien est le chemin visé. Recopié à la fin s'il reste dans l'archive.
            let mut link = String::new();
            let resolved = std::io::Read::take(&mut entry, 4096)
                .read_to_string(&mut link)
                .ok()
                .and_then(|_| safe_relative(&rel.parent().map(Path::to_path_buf).unwrap_or_default().join(link.trim())));
            match resolved {
                Some(r) => links.push((r, dest.join(&rel))),
                None => {
                    return Err(InstallError::Failed(trf!(
                        "lien refusé dans l'archive : {}",
                        "link refused in the archive: {}",
                        entry.name().unwrap_or_default()
                    )));
                }
            }
            continue;
        }
        let target = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
        }
        budget.copy(&mut entry, &target)?;
        report(budget.bytes, total);
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755));
        }
    }
    copy_links(dest, links, &mut budget)
}

/// Liens d'une archive recopiés comme fichiers ordinaires (plusieurs passes : un lien peut viser
/// un autre lien). `links` : (cible relative déjà validée, emplacement du lien). Un lien vers un
/// dossier ou vers rien n'est pas recopié ; les essais d'après installation diront s'il manquait.
fn copy_links(dest: &Path, mut links: Vec<(PathBuf, PathBuf)>, budget: &mut Budget) -> Result<(), InstallError> {
    for _ in 0..4 {
        let mut left = Vec::new();
        for (src, target) in links {
            let src_path = dest.join(&src);
            if src_path.is_file() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
                }
                let n = std::fs::copy(&src_path, &target).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
                budget.add(n)?;
            } else {
                left.push((src, target));
            }
        }
        links = left;
    }
    Ok(())
}

/// Écrivain borné vers un fichier : le XZ est décompressé sur le disque (FFmpeg pour Linux fait
/// environ 450 Mo une fois décompressé), sans pouvoir dépasser la taille autorisée, et
/// l'annulation est prise en compte pendant la décompression.
struct Bounded<'a> {
    file: std::io::BufWriter<std::fs::File>,
    written: u64,
    cancel: &'a AtomicBool,
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("annulé / cancelled"));
        }
        if self.written + buf.len() as u64 > MAX_EXTRACTED {
            return Err(std::io::Error::other("trop volumineux / too large"));
        }
        self.written += buf.len() as u64;
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Avancement : la décompression compte pour les neuf dixièmes (c'est le plus long), l'écriture
/// des fichiers pour le reste.
#[derive(Clone, Copy)]
enum Compression {
    Xz,
    Gz,
}

fn extract_tar(archive: &Path, comp: Compression, dest: &Path, cancel: &AtomicBool, report: Report) -> Result<(), InstallError> {
    const SCALE: u64 = 1000;
    let f = std::fs::File::open(archive).map_err(|e| io_err(tr!("ouverture de l'archive", "opening the archive"), e))?;
    let xz_size = f.metadata().map(|m| m.len()).unwrap_or(0).max(1);
    // Fichier intermédiaire dans le dossier des téléchargements (supprimé avec lui).
    let tar_path = archive.with_extension("tar");
    let out = std::fs::File::create(&tar_path).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
    let mut sink = Bounded { file: std::io::BufWriter::new(out), written: 0, cancel };
    let mut last = u64::MAX;
    let mut on_read = |n: u64, total: u64| {
        let done = n.min(total) * 900 / total;
        if done != last {
            last = done;
            report(done, SCALE);
        }
    };
    let counting = Counting { inner: f, n: 0, total: xz_size, report: &mut on_read };
    let input = std::io::BufReader::new(counting);
    let r = match comp {
        Compression::Xz => std::io::copy(&mut lzma_rust2::XzReader::new(input, true), &mut sink),
        Compression::Gz => std::io::copy(&mut flate2::read::MultiGzDecoder::new(input), &mut sink),
    }
    .and_then(|_| sink.flush());
    let tar_size = sink.written.max(1);
    drop(sink);
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallError::Cancelled);
    }
    r.map_err(|e| InstallError::Failed(trf!("archive compressée illisible : {}", "unreadable compressed archive: {}", e)))?;
    let tar_file = std::fs::File::open(&tar_path).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
    let mut tar = tar::Archive::new(std::io::BufReader::new(tar_file));
    let mut budget = Budget::new();
    let mut links: Vec<(PathBuf, PathBuf)> = Vec::new();
    let entries = tar.entries().map_err(|e| InstallError::Failed(trf!("archive TAR illisible : {}", "unreadable TAR archive: {}", e)))?;
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        budget.entry()?;
        let mut entry = entry.map_err(|e| InstallError::Failed(trf!("archive TAR illisible : {}", "unreadable TAR archive: {}", e)))?;
        let raw = entry.path().map_err(|e| InstallError::Failed(e.to_string()))?.into_owned();
        let Some(rel) = safe_relative(&raw) else {
            if raw.components().all(|c| matches!(c, Component::CurDir)) {
                continue; // l'entrée « ./ » elle-même
            }
            return Err(InstallError::Failed(trf!("chemin refusé dans l'archive : {}", "path refused in the archive: {}", raw.display())));
        };
        let target = dest.join(&rel);
        match entry.header().entry_type() {
            tar::EntryType::Directory => std::fs::create_dir_all(&target).map_err(|e| io_err(tr!("extraction", "extraction"), e))?,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io_err(tr!("extraction", "extraction"), e))?;
                }
                let mode = entry.header().mode().unwrap_or(0o644);
                budget.copy(&mut entry, &target)?;
                report(900 + budget.bytes.min(tar_size) * 100 / tar_size, SCALE);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755));
                }
                #[cfg(not(unix))]
                let _ = mode;
            }
            // En-têtes étendus (noms longs) : déjà appliqués par la bibliothèque.
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader | tar::EntryType::GNULongName | tar::EntryType::GNULongLink => {}
            // Liens (Pandoc, Calibre pour Linux en contiennent) : traités à la fin, quand leur
            // cible est extraite. Un lien qui sort de l'archive est refusé.
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let link = entry.link_name().ok().flatten().map(|l| l.into_owned());
                let base = if entry.header().entry_type() == tar::EntryType::Link {
                    PathBuf::new()
                } else {
                    rel.parent().map(Path::to_path_buf).unwrap_or_default()
                };
                let resolved = link.as_deref().and_then(|l| safe_relative(&base.join(l)));
                match resolved {
                    Some(r) => links.push((r, target)),
                    None => {
                        return Err(InstallError::Failed(trf!(
                            "lien refusé dans l'archive : {}",
                            "link refused in the archive: {}",
                            raw.display()
                        )));
                    }
                }
            }
            other => {
                return Err(InstallError::Failed(trf!(
                    "élément refusé dans l'archive ({:?}) : {}",
                    "item refused in the archive ({:?}): {}",
                    other,
                    raw.display()
                )));
            }
        }
    }
    copy_links(dest, links, &mut budget)?;
    // Les en-têtes du tar comptent dans sa taille : la fin est annoncée explicitement.
    report(SCALE, SCALE);
    Ok(())
}

/// Ouvre l'installeur 7-Zip avec `7zr` (vérifié lui aussi) : l'installeur n'est jamais exécuté.
fn extract_with_7zr(extractor: &Path, archive: &Path, dest: &Path) -> Result<(), InstallError> {
    // `7zr` doit porter l'extension .exe sous Windows pour être lancé.
    let exe = extractor.with_file_name(exe_name("7zr"));
    std::fs::rename(extractor, &exe).map_err(|e| io_err(tr!("préparation de 7zr", "preparing 7zr"), e))?;
    let mut out_arg = std::ffi::OsString::from("-o");
    out_arg.push(dest.as_os_str());
    let spec = ProcessSpec::new(&exe).args(["x", "-y", "-bso0", "-bsp0"]).arg(out_arg).arg(archive).timeout(Some(Duration::from_secs(300)));
    let out = chud_process::run_capture(&spec, 64 * 1024).map_err(|e| InstallError::Failed(trf!("7zr : {}", "7zr: {}", e)))?;
    if !out.success() {
        return Err(InstallError::Failed(trf!(
            "l'extraction par 7zr a échoué : {}",
            "extraction with 7zr failed: {}",
            out.stderr_tail.lines().last().unwrap_or("").trim()
        )));
    }
    Ok(())
}

/// Installeur NSIS : `7zr` ouvre l'installeur de 7-Zip pour obtenir `7z` complet (qui sait lire
/// le format NSIS), puis `7z` ouvre l'installeur du moteur. Aucun des deux n'est exécuté.
fn extract_nsis(sevenzr: &Path, sevenzip_installer: &Path, target: &Path, work: &Path, dest: &Path) -> Result<(), InstallError> {
    let tools = work.join("7z");
    std::fs::create_dir_all(&tools).map_err(|e| io_err(tr!("préparation de 7-Zip", "preparing 7-Zip"), e))?;
    extract_with_7zr(sevenzr, sevenzip_installer, &tools)?;
    let sz = find_exe(&tools, "7z").ok_or_else(|| {
        InstallError::Failed(tr!("7z introuvable dans l'installeur de 7-Zip", "7z not found in the 7-Zip installer").into())
    })?;
    let mut out_arg = std::ffi::OsString::from("-o");
    out_arg.push(dest.as_os_str());
    let spec =
        ProcessSpec::new(&sz).args(["x", "-y", "-bso0", "-bsp0", "-snl-"]).arg(out_arg).arg(target).timeout(Some(Duration::from_secs(900)));
    let out = chud_process::run_capture(&spec, 64 * 1024).map_err(|e| InstallError::Failed(trf!("7z : {}", "7z: {}", e)))?;
    if !out.success() {
        return Err(InstallError::Failed(trf!(
            "l'ouverture de l'installeur a échoué : {}",
            "opening the installer failed: {}",
            out.stderr_tail.lines().last().unwrap_or("").trim()
        )));
    }
    // Dossiers techniques de NSIS, jamais utiles au moteur.
    for name in ["$PLUGINSDIR", "$TEMP", "$_OUTDIR"] {
        let _ = std::fs::remove_dir_all(dest.join(name));
    }
    Ok(())
}

/// `msiexec /a` : copie les fichiers du paquet dans `dest` sans rien enregistrer sur le système.
/// `msiexec` ne sait pas écrire au-delà de 260 caractères (erreur 1320, code 1603) et certains
/// paquets ont des chemins internes très longs (Calibre : plus de 120 caractères). Le paquet est
/// donc déplié dans un dossier temporaire court, puis déplacé dans `dest` par Chud Converter, qui
/// gère les chemins longs.
fn extract_msi_short(msi: &Path, dest: &Path) -> Result<(), InstallError> {
    let short = std::env::temp_dir().join(format!("chud-msi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&short);
    std::fs::create_dir_all(&short).map_err(|e| io_err(tr!("création du dossier temporaire", "creating the temporary folder"), e))?;
    let r = extract_msi(msi, &short).and_then(|()| move_children(&short, dest));
    let _ = std::fs::remove_dir_all(&short);
    r
}

/// Déplace le contenu de `from` dans `to` (copie si les deux ne sont pas sur le même disque).
fn move_children(from: &Path, to: &Path) -> Result<(), InstallError> {
    let what = tr!("déplacement des fichiers extraits", "moving the extracted files");
    let rd = std::fs::read_dir(from).map_err(|e| io_err(what, e))?;
    for e in rd.flatten() {
        let target = to.join(e.file_name());
        if std::fs::rename(e.path(), &target).is_err() {
            copy_all(&e.path(), &target).map_err(|e| io_err(what, e))?;
        }
    }
    Ok(())
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::other(format!("lien refusé : {}", from.display())));
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn extract_msi(msi: &Path, dest: &Path) -> Result<(), InstallError> {
    if !cfg!(windows) {
        return Err(tr!("paquet MSI : Windows seulement", "MSI package: Windows only").to_owned().into());
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let msiexec = root.join("System32").join("msiexec.exe");
    // `msiexec` lit lui-même `TARGETDIR="…"` : les chemins viennent de Chud Converter (jamais de
    // l'utilisateur) et un chemin Windows ne peut pas contenir de guillemet.
    let quoted = |p: &Path| -> Result<std::ffi::OsString, InstallError> {
        let s = p.to_str().ok_or_else(|| InstallError::Failed(tr!("chemin non pris en charge", "unsupported path").into()))?;
        if s.contains('"') {
            return Err(tr!("chemin non pris en charge", "unsupported path").to_owned().into());
        }
        Ok(format!("\"{s}\"").into())
    };
    let mut target = std::ffi::OsString::from("TARGETDIR=");
    target.push(quoted(dest)?);
    let spec = ProcessSpec::new(&msiexec)
        .raw_arg("/a")
        .raw_arg(quoted(msi)?)
        .raw_arg("/qn")
        .raw_arg(target)
        .timeout(Some(Duration::from_secs(1800)));
    let out = chud_process::run_capture(&spec, 64 * 1024).map_err(|e| InstallError::Failed(trf!("msiexec : {}", "msiexec: {}", e)))?;
    match out.code {
        Some(0) => {}
        Some(1618) => {
            return Err(tr!(
                "une autre installation Windows est en cours : réessayez quand elle sera terminée",
                "another Windows installation is in progress: try again when it is finished"
            )
            .to_owned()
            .into());
        }
        code => return Err(InstallError::Failed(trf!("msiexec a échoué (code {:?})", "msiexec failed (code {:?})", code))),
    }
    // L'installation administrative recopie le paquet lui-même : inutile ensuite.
    if let Ok(rd) = std::fs::read_dir(dest) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("msi")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}

/// Après extraction : éléments inutiles retirés, renommages, fichiers de données ajoutés.
fn finish_tree(src: &InstallSource, dir: &Path, extras: &[(PathBuf, String)]) -> Result<(), InstallError> {
    for [from, to] in &src.copies {
        let (from, to) = (dir.join(from), dir.join(to));
        let rd = std::fs::read_dir(&from).map_err(|e| io_err(tr!("copie des bibliothèques", "copying the libraries"), e))?;
        for e in rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())) {
            let target = to.join(e.file_name());
            if !target.exists() {
                std::fs::copy(e.path(), &target).map_err(|e| io_err(tr!("copie des bibliothèques", "copying the libraries"), e))?;
            }
        }
    }
    for name in &src.remove {
        let p = dir.join(name);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    for [from, to] in &src.renames {
        let found = find_file(dir, from).ok_or_else(|| {
            InstallError::Failed(trf!("{} introuvable dans le paquet téléchargé", "{} not found in the downloaded package", from))
        })?;
        std::fs::rename(&found, found.with_file_name(to)).map_err(|e| io_err(tr!("renommage", "renaming"), e))?;
    }
    for (file, rel) in extras {
        let target = dir.join(safe_relative(Path::new(rel)).ok_or_else(|| InstallError::Failed(rel.clone()))?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(tr!("ajout des données", "adding the data"), e))?;
        }
        std::fs::copy(file, &target).map_err(|e| io_err(tr!("ajout des données", "adding the data"), e))?;
    }
    Ok(())
}

/// Premier fichier portant ce nom, jusqu'à quatre niveaux de profondeur.
fn find_file(base: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![(base.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let candidate = d.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if depth < 4
            && let Ok(rd) = std::fs::read_dir(&d)
        {
            stack.extend(rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| (e.path(), depth + 1)));
        }
    }
    None
}

/// Redistribuable Visual C++ 2015-2022 (x64) de Microsoft : adresse versionnée, empreinte figée
/// (l'adresse contient elle-même l'empreinte publiée par Microsoft).
pub fn vc_redist() -> Download {
    Download {
        url: "https://download.visualstudio.microsoft.com/download/pr/e9604e21-4b79-4096-a4b3-1b63d4b55b4d/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe".into(),
        sha256: "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b".into(),
        size: 25_635_768,
    }
}

/// Runtime Visual C++ présent sur le système (les DLL dont dépendent les moteurs compilés
/// avec Visual Studio).
pub fn vcruntime_present() -> bool {
    if !cfg!(windows) {
        return true;
    }
    let Some(root) = std::env::var_os("SystemRoot") else { return false };
    let sys = PathBuf::from(root).join("System32");
    ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "vcomp140.dll"].iter().all(|d| sys.join(d).is_file())
}

/// Lance l'installeur de Microsoft (vérifié). Windows affiche sa propre demande d'autorisation.
fn install_vcruntime(redist: &Path) -> Result<(), InstallError> {
    let exe = redist.with_file_name("VC_redist.x64.exe");
    std::fs::rename(redist, &exe).map_err(|e| io_err(tr!("préparation du runtime", "preparing the runtime"), e))?;
    let spec = ProcessSpec::new(&exe).args(["/install", "/quiet", "/norestart"]).timeout(Some(Duration::from_secs(900)));
    let out = chud_process::run_capture(&spec, 64 * 1024)
        .map_err(|e| InstallError::Failed(trf!("runtime Visual C++ : {}", "Visual C++ runtime: {}", e)))?;
    match out.code {
        // 1638 : une version plus récente est déjà là ; 3010 : redémarrage conseillé, mais les
        // fichiers sont en place.
        Some(0 | 1638 | 3010) => Ok(()),
        Some(1602) => Err(tr!(
            "installation du runtime Visual C++ refusée dans la fenêtre de Windows : le moteur ne peut pas fonctionner sans lui",
            "Visual C++ runtime installation declined in the Windows prompt: the engine cannot work without it"
        )
        .to_owned()
        .into()),
        code => Err(InstallError::Failed(trf!(
            "l'installation du runtime Visual C++ a échoué (code {:?})",
            "the Visual C++ runtime installation failed (code {:?})",
            code
        ))),
    }
}

/// Après extraction : aucun lien, taille raisonnable.
/// Renvoie aussi la longueur (en caractères UTF-16, comme Windows les compte) du plus long chemin
/// relatif à `dir`.
fn check_tree(dir: &Path) -> Result<usize, InstallError> {
    let mut stack = vec![dir.to_path_buf()];
    let mut budget = Budget::new();
    let mut deepest = 0;
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| io_err(tr!("lecture du dossier extrait", "reading the extracted folder"), e))?;
        for e in rd.flatten() {
            budget.entry()?;
            let meta = std::fs::symlink_metadata(e.path())
                .map_err(|e| io_err(tr!("lecture du dossier extrait", "reading the extracted folder"), e))?;
            if meta.file_type().is_symlink() {
                return Err(InstallError::Failed(trf!("lien refusé : {}", "link refused: {}", e.path().display())));
            }
            if let Ok(rel) = e.path().strip_prefix(dir) {
                deepest = deepest.max(rel.as_os_str().to_string_lossy().encode_utf16().count());
            }
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                budget.bytes += meta.len();
                if budget.bytes > MAX_EXTRACTED {
                    return Err(tr!("contenu extrait trop volumineux", "extracted content too large").to_owned().into());
                }
            }
        }
    }
    Ok(deepest)
}

/// Longueur maximale d'un chemin pour la plupart des programmes Windows (MAX_PATH, 260 avec le
/// zéro final). Un moteur installé plus profond ne trouverait pas ses propres fichiers.
const WINDOWS_MAX_PATH: usize = 259;

/// Refuse clairement une installation dont certains fichiers dépasseraient cette limite.
fn check_path_length(final_dir: &Path, deepest: usize, name: &str) -> Result<(), InstallError> {
    let base = final_dir.as_os_str().to_string_lossy().encode_utf16().count();
    if cfg!(windows) && base + 1 + deepest > WINDOWS_MAX_PATH {
        return Err(InstallError::Failed(trf!(
            "{name} contient des chemins de {deepest} caractères, et Windows limite un chemin complet à 260 : le dossier de Chud Converter ({base} caractères) est trop profond. Placez-le dans un dossier plus court (par exemple C:\\Chud Converter) puis réessayez",
            "{name} contains paths of {deepest} characters, and Windows limits a full path to 260: Chud Converter's folder ({base} characters) is too deep. Move it to a shorter folder (for example C:\\Chud Converter) and try again"
        )));
    }
    Ok(())
}

/// Essai réel : l'exécutable principal répond avec sa version, les compagnons se lancent.
pub(crate) fn check_executables(m: &Manifest, dir: &Path) -> Result<PathBuf, InstallError> {
    let exes = m.platform_executables();
    let primary =
        exes.iter().take(if m.executables.len() == 1 { exes.len() } else { 1 }).find_map(|e| find_exe(dir, e)).ok_or_else(|| {
            InstallError::Failed(trf!("{} introuvable dans le paquet téléchargé", "{} not found in the downloaded package", exes[0]))
        })?;
    probe_version(&primary, m).map_err(|e| {
        InstallError::Failed(trf!("le moteur installé ne fonctionne pas : {}", "the installed engine does not work: {}", e))
    })?;
    if m.executables.len() > 1 {
        for comp in &m.executables[1..] {
            let p = primary.parent().map(|d| d.join(exe_name(comp))).filter(|p| p.is_file()).ok_or_else(|| {
                InstallError::Failed(trf!("{} manque dans le paquet téléchargé", "{} is missing from the downloaded package", comp))
            })?;
            let spec = ProcessSpec::new(&p).args(m.detect.version_args.iter()).timeout(Some(Duration::from_secs(15)));
            let ok = chud_process::run_capture(&spec, 1024 * 1024).is_ok_and(|o| o.success());
            if !ok {
                return Err(InstallError::Failed(trf!("{} ne se lance pas", "{} does not start", comp)));
            }
        }
    }
    Ok(primary)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), InstallError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| io_err(tr!("écriture", "writing"), e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(tr!("écriture", "writing"), e))
}

fn read_current(root: &Path) -> Option<String> {
    let v = std::fs::read_to_string(root.join("current")).ok()?.trim().to_owned();
    safe_version(&v).then_some(v)
}

/// Sous Windows, l'antivirus examine les fichiers tout juste extraits et les garde ouverts un
/// instant : renommer leur dossier échoue alors (« accès refusé »). On réessaie quelques
/// secondes avant d'abandonner.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied && tries < 40 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(250));
            }
            r => return r,
        }
    }
}

fn activate(src: &InstallSource, root: &Path, partial: &Path, exe_in_partial: &Path) -> Result<Installed, InstallError> {
    let versions = root.join("versions");
    let final_dir = versions.join(&src.version);
    let old = read_current(root);
    if final_dir.exists() {
        // Réinstallation de la même version : l'ancienne copie est écartée d'abord.
        let aside = versions.join(format!("{}.old", src.version));
        let _ = std::fs::remove_dir_all(&aside);
        std::fs::rename(&final_dir, &aside).map_err(|e| {
            InstallError::Failed(trf!(
                "la version {} est en cours d'utilisation ({}) : terminez les conversions puis réessayez",
                "version {} is in use ({}): finish the conversions and try again",
                src.version,
                e
            ))
        })?;
        let _ = std::fs::remove_dir_all(&aside);
    }
    rename_retrying(partial, &final_dir).map_err(|e| io_err(tr!("activation de la version", "activating the version"), e))?;
    let rel = exe_in_partial.strip_prefix(partial).unwrap_or(exe_in_partial).to_path_buf();
    let previous = old.filter(|o| *o != src.version && versions.join(o).is_dir());
    let rec = InstallRecord {
        version: src.version.clone(),
        previous: previous.clone(),
        installed_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        publisher: src.publisher.clone(),
        files: src.files.clone(),
    };
    write_atomic(&record_path(root), &serde_json::to_string_pretty(&rec).unwrap_or_default())?;
    write_atomic(&root.join("current"), &src.version)?;
    prune(&versions, &[Some(src.version.as_str()), previous.as_deref()]);
    Ok(Installed { version: src.version.clone(), path: final_dir.join(rel), previous })
}

/// Ne garde que la version active et la précédente (un échec de suppression n'est pas grave :
/// un fichier encore ouvert sera retiré à la prochaine installation).
fn prune(versions: &Path, keep: &[Option<&str>]) {
    let Ok(rd) = std::fs::read_dir(versions) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !keep.iter().flatten().any(|k| *k == name) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Revient à la version précédente gardée sur le disque.
pub fn rollback(paths: &AppPaths, m: &Manifest) -> Result<String, InstallError> {
    let root = paths.engines_dir().join(&m.id);
    let _lock = Lock::take(&root)?;
    let mut rec = record(paths, &m.id).ok_or_else(|| {
        InstallError::Failed(trf!("{} n'a pas été installé par Chud Converter", "{} was not installed by Chud Converter", m.name))
    })?;
    let prev = previous_version(paths, &m.id).ok_or_else(|| {
        InstallError::Failed(tr!("aucune version précédente n'est disponible", "no previous version is available").into())
    })?;
    let dir = root.join("versions").join(&prev);
    let exe =
        m.platform_executables().iter().find_map(|e| find_exe(&dir, e)).ok_or_else(|| {
            InstallError::Failed(tr!("la version précédente est incomplète", "the previous version is incomplete").into())
        })?;
    probe_version(&exe, m).map_err(|e| {
        InstallError::Failed(trf!("la version précédente ne fonctionne pas : {}", "the previous version does not work: {}", e))
    })?;
    let now = std::mem::replace(&mut rec.version, prev.clone());
    rec.previous = Some(now);
    write_atomic(&record_path(&root), &serde_json::to_string_pretty(&rec).unwrap_or_default())?;
    write_atomic(&root.join("current"), &prev)?;
    Ok(prev)
}

/// Retire tout ce que Chud Converter a installé pour ce moteur. Les installations du système ne
/// sont jamais concernées : seul `<données>/engines/<id>` est supprimé.
pub fn uninstall(paths: &AppPaths, m: &Manifest) -> Result<(), InstallError> {
    let root = paths.engines_dir().join(&m.id);
    if !root.join("current").exists() && record(paths, &m.id).is_none() {
        return Err(InstallError::Failed(trf!(
            "{} n'a pas été installé par Chud Converter",
            "{} was not installed by Chud Converter",
            m.name
        )));
    }
    let _lock = Lock::take(&root)?;
    // D'abord désactiver (plus rien ne pointe vers une version), puis supprimer.
    let _ = std::fs::remove_file(root.join("current"));
    let _ = std::fs::remove_file(record_path(&root));
    let versions = root.join("versions");
    if versions.exists() {
        std::fs::remove_dir_all(&versions).map_err(|e| {
            InstallError::Failed(trf!(
                "certains fichiers sont encore utilisés ({}) : terminez les conversions puis réessayez",
                "some files are still in use ({}): finish the conversions and try again",
                e
            ))
        })?;
    }
    let _ = std::fs::remove_dir_all(root.join("downloads"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_deep_for_windows_is_refused_clearly() {
        let short = Path::new(r"C:\Users\a\AppData\Local\Chud Converter\engines\calibre\versions\9.18.0");
        assert!(check_path_length(short, 125, "Calibre").is_ok());
        let deep = PathBuf::from(format!(r"C:\{}\versions\9.18.0", "x".repeat(150)));
        assert_eq!(check_path_length(&deep, 125, "Calibre").is_err(), cfg!(windows));
    }

    #[test]
    fn versions_and_paths_are_checked() {
        assert!(safe_version("9.0.2"));
        assert!(safe_version("26.06"));
        assert!(!safe_version("../x"));
        assert!(!safe_version("a/b"));
        assert!(!safe_version(""));
        assert_eq!(safe_relative(Path::new("a/b.exe")), Some(PathBuf::from("a").join("b.exe")));
        assert_eq!(safe_relative(Path::new("../evil")), None);
        assert_eq!(safe_relative(Path::new("/abs")), None);
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn zip_extraction_refuses_escapes() {
        let dir = std::env::temp_dir().join(format!("chud-install-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.zip");
        std::fs::write(&ok, zip_with(&[("pkg/bin/tool.txt", b"hello")])).unwrap();
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_zip(&ok, &dest, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dest.join("pkg/bin/tool.txt")).unwrap(), b"hello");

        let bad = dir.join("bad.zip");
        std::fs::write(&bad, zip_with(&[("../evil.txt", b"x")])).unwrap();
        let dest2 = dir.join("out2");
        std::fs::create_dir_all(&dest2).unwrap();
        assert!(extract_zip(&bad, &dest2, &AtomicBool::new(false), &mut |_, _| {}).is_err());
        assert!(!dir.join("evil.txt").exists());

        // Liens Unix (qpdf pour Linux) : recopiés s'ils restent dans l'archive, refusés sinon.
        let links = |target: &str| {
            let mut buf = std::io::Cursor::new(Vec::new());
            let mut w = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("lib/libx.so.30.4", o).unwrap();
            w.write_all(b"ELF").unwrap();
            w.add_symlink("lib/libx.so.30", target, o).unwrap();
            w.finish().unwrap();
            buf.into_inner()
        };
        let l = dir.join("links.zip");
        std::fs::write(&l, links("libx.so.30.4")).unwrap();
        let dest3 = dir.join("out3");
        std::fs::create_dir_all(&dest3).unwrap();
        extract_zip(&l, &dest3, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dest3.join("lib/libx.so.30")).unwrap(), b"ELF");
        assert!(!std::fs::symlink_metadata(dest3.join("lib/libx.so.30")).unwrap().file_type().is_symlink());
        std::fs::write(&l, links("../../etc/passwd")).unwrap();
        let dest4 = dir.join("out4");
        std::fs::create_dir_all(&dest4).unwrap();
        assert!(extract_zip(&l, &dest4, &AtomicBool::new(false), &mut |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tar_xz_with(name: &str, data: &[u8]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(&mut h, name, data).unwrap();
        let tar = tar.into_inner().unwrap();
        xz(&tar, false)
    }

    /// Compression XZ ; `bcj` ajoute le filtre x86 que les archives Linux de 7-Zip utilisent.
    fn xz(data: &[u8], bcj: bool) -> Vec<u8> {
        let mut opts = lzma_rust2::XzOptions::with_preset(1);
        if bcj {
            opts.prepend_pre_filter(lzma_rust2::FilterType::BcjX86, 0);
        }
        let mut w = lzma_rust2::XzWriter::new(Vec::new(), opts).unwrap();
        w.write_all(data).unwrap();
        w.finish().unwrap()
    }

    #[test]
    fn tar_xz_with_bcj_filter_is_read() {
        let dir = std::env::temp_dir().join(format!("chud-install-bcj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let body: Vec<u8> = (0..20_000u32).flat_map(|i| [0xE8, i as u8, (i >> 8) as u8, 0, 0]).collect();
        let mut tar = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(&mut h, "7zz", &body[..]).unwrap();
        let archive = dir.join("bcj.tar.xz");
        std::fs::write(&archive, xz(&tar.into_inner().unwrap(), true)).unwrap();
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_tar(&archive, Compression::Xz, &dest, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dest.join("7zz")).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_xz_extraction_streams_to_disk_and_refuses_escapes() {
        let dir = std::env::temp_dir().join(format!("chud-install-txz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.tar.xz");
        std::fs::write(&ok, tar_xz_with("pkg/bin/tool", b"#!/bin/sh\n")).unwrap();
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let mut seen = Vec::new();
        extract_tar(&ok, Compression::Xz, &dest, &AtomicBool::new(false), &mut |d, t| seen.push((d, t))).unwrap();
        assert_eq!(seen.last(), Some(&(1000, 1000)), "avancement jusqu'au bout : {seen:?}");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "avancement croissant");
        assert_eq!(std::fs::read(dest.join("pkg/bin/tool")).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dest.join("pkg/bin/tool")).unwrap().permissions().mode() & 0o777, 0o755);
        }
        // Le tar ne permet pas d'écrire « ../ » avec l'API normale : en-tête modifié à la main.
        let mut raw = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.as_mut_bytes()[..9].copy_from_slice(b"../evil\0\0");
        h.set_size(1);
        h.set_cksum();
        raw.append(&h, &b"x"[..]).unwrap();
        let raw = raw.into_inner().unwrap();
        let bad = dir.join("bad.tar.xz");
        std::fs::write(&bad, xz(&raw, false)).unwrap();
        let dest2 = dir.join("out2");
        std::fs::create_dir_all(&dest2).unwrap();
        assert!(extract_tar(&bad, Compression::Xz, &dest2, &AtomicBool::new(false), &mut |_, _| {}).is_err());
        assert!(!dir.join("evil").exists());
        // Annulation pendant la décompression.
        assert!(matches!(extract_tar(&ok, Compression::Xz, &dest2, &AtomicBool::new(true), &mut |_, _| {}), Err(InstallError::Cancelled)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Temps réel sur une vraie archive : `CHUD_TEST_TARXZ=<fichier> cargo test … -- --ignored`.
    #[test]
    #[ignore = "archive fournie à la main"]
    fn tar_xz_real_archive() {
        let Some(p) = std::env::var_os("CHUD_TEST_TARXZ") else { return };
        let dest = std::env::temp_dir().join(format!("chud-install-txz-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let t = std::time::Instant::now();
        extract_tar(Path::new(&p), Compression::Xz, &dest, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert!(check_tree(&dest).unwrap() > 0);
        eprintln!("extraction : {:.1} s", t.elapsed().as_secs_f64());
        assert!(find_exe(&dest, "ffmpeg").or_else(|| find_exe(&dest, "7zz")).is_some() || cfg!(windows));
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn manifest_sources_are_valid() {
        for m in crate::builtin_manifests() {
            for s in &m.install {
                s.validate().unwrap_or_else(|e| panic!("{} : {e}", m.id));
            }
        }
        let ffmpeg = crate::builtin_manifests().into_iter().find(|m| m.id == "ffmpeg").unwrap();
        assert!(ffmpeg.install.iter().any(|s| s.platform == "windows-x86_64"));
    }

    #[test]
    fn existing_installs_are_never_replaced_silently() {
        use crate::{EngineState, EngineStatus};
        use chud_core::EngineOrigin;
        let st = |state: EngineState, origin: Option<EngineOrigin>| EngineStatus {
            id: "ffmpeg".into(),
            name: "FFmpeg".into(),
            state,
            path: Some(PathBuf::from("/usr/bin/ffmpeg")),
            companions: Default::default(),
            version: Some("7.1".into()),
            origin,
            license: String::new(),
            homepage: String::new(),
            description: String::new(),
            hint: None,
            supported: true,
            categories: vec!["video".into()],
            installable: Some(crate::InstallOffer {
                version: "9".into(),
                size: 1,
                publisher: String::new(),
                license: String::new(),
                page: String::new(),
                runtime: false,
                system: None,
            }),
            previous: None,
        };
        assert!(check_allowed(&st(EngineState::Missing, None), false).is_ok());
        assert!(check_allowed(&st(EngineState::Broken("x".into()), Some(EngineOrigin::System)), false).is_ok());
        assert!(check_allowed(&st(EngineState::Ready, Some(EngineOrigin::Managed)), false).is_ok());
        assert!(check_allowed(&st(EngineState::Ready, Some(EngineOrigin::System)), false).is_err(), "jamais sans demande");
        assert!(check_allowed(&st(EngineState::Ready, Some(EngineOrigin::System)), true).is_ok(), "à côté, sur demande");
        assert!(check_allowed(&st(EngineState::Ready, Some(EngineOrigin::Custom)), true).is_err(), "le chemin choisi passe avant");
        // Voie « paquets du système » : rien de portable à mettre à côté.
        let mut sys = st(EngineState::Ready, Some(EngineOrigin::System));
        sys.installable = None;
        assert!(check_allowed(&sys, true).is_err());
    }

    #[test]
    fn install_without_source_says_so() {
        let dir = std::env::temp_dir().join(format!("chud-install-none-{}", std::process::id()));
        let paths = AppPaths::at(&dir);
        let mut pandoc = crate::builtin_manifests().into_iter().find(|m| m.id == "pandoc").unwrap();
        pandoc.install.clear();
        let err = install(&paths, &pandoc, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(matches!(err, InstallError::Failed(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_and_uninstall_touch_only_managed_files() {
        let dir = std::env::temp_dir().join(format!("chud-install-rb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = AppPaths::at(&dir);
        let m = crate::builtin_manifests().into_iter().find(|m| m.id == "7zip").unwrap();
        // Rien d'installé : refus clair, aucun dossier créé.
        assert!(uninstall(&paths, &m).is_err());
        assert!(rollback(&paths, &m).is_err());
        assert!(!paths.engines_dir().join("7zip").join("versions").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
