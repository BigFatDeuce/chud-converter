//! Menu clic droit de l'explorateur de fichiers : « Chud Converter ▸ MP4, MP3… ».
//!
//! Le contenu du menu se calcule ici, sans dépendre du système ([`plan_menus`]) : pour chaque
//! extension connue, les préréglages actifs qui s'y appliquent (un préréglage qui ne ferait rien,
//! comme « PNG » sur un `.png`, n'est pas proposé). Ensuite :
//!
//! - **Windows** ([`windows`]) : clés du registre de l'utilisateur (`HKCU`), sans droits
//!   d'administrateur. Rien n'est remplacé : seules nos propres clés `ChudConverter` sont créées,
//!   et la liste exacte est gardée pour les retirer toutes, et rien qu'elles.
//! - **Linux** ([`linux`]) : menus de service de Dolphin, actions de Nemo, scripts de Nautilus,
//!   dans le dossier de l'utilisateur, avec la même liste de ce qui a été créé.
//!
//! Les commandes lancées par le menu passent les chemins comme arguments séparés
//! (`"%1"`, `%F`, `"$@"`) : un nom de fichier n'est jamais interprété comme une commande.

use chud_core::{FormatCatalog, FormatId};
use chud_jobs::{PresetStore, presets::FOLDER};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod linux;
#[cfg(windows)]
pub mod windows;

/// Une entrée du sous-menu.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MenuItem {
    pub preset: String,
    pub name: String,
}

/// Un sous-menu et les extensions (sans point, en minuscules) qui le montrent. Pour les
/// dossiers, `folder` est vrai et `extensions` est vide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// Nom stable, utilisable dans un nom de clé ou de fichier (`mkv`, `folder`…).
    pub key: String,
    pub folder: bool,
    pub extensions: Vec<String>,
    /// Types MIME correspondants (pour Dolphin).
    pub mime: Vec<String>,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("{}", chud_core::trf!("programme introuvable : {}", "program not found: {}", .0.display()))]
    MissingExe(PathBuf),
    #[error("{}", chud_core::trf!("aucun préréglage actif : rien à mettre dans le menu", "no active preset: nothing to put in the menu"))]
    Empty,
    #[error("{}", chud_core::trf!("entrée inattendue dans la liste des éléments installés, ignorée par sécurité : {}", "unexpected entry in the list of installed items, ignored for safety: {}", .0))]
    Suspicious(String),
    #[error("{}", chud_core::trf!("menu contextuel non pris en charge sur ce système", "context menu not supported on this system"))]
    Unsupported,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// État de l'intégration, tel qu'affiché par `chud shell status` et par l'application.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub installed: bool,
    /// Programme lancé par le menu.
    pub exe: Option<PathBuf>,
    /// Nombre d'extensions (et dossiers) qui affichent le menu.
    pub entries: usize,
    /// Le programme enregistré n'existe plus (dossier portable déplacé, par exemple).
    pub exe_missing: bool,
    /// Où c'est rangé, en clair.
    pub location: String,
}

/// Menus à créer d'après les préréglages, sans tenir compte des moteurs installés.
pub fn plan_menus(store: &PresetStore, catalog: &FormatCatalog) -> Vec<Menu> {
    plan_menus_with(store, catalog, |_, _| true)
}

/// Menus à créer d'après les préréglages, en ne gardant que les conversions que `can(source,
/// cible)` déclare réalisables (moteurs présents) : le menu ne promet que ce qui marche. Les
/// extensions qui donnent exactement le même sous-menu le partagent.
pub fn plan_menus_with(store: &PresetStore, catalog: &FormatCatalog, mut can: impl FnMut(&FormatId, &FormatId) -> bool) -> Vec<Menu> {
    // extension → (préréglages, mime)
    let mut by_items: BTreeMap<Vec<MenuItem>, (Vec<String>, Vec<String>)> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for def in catalog.all() {
        for ext in &def.extensions {
            let ext = ext.trim_start_matches('.').to_ascii_lowercase();
            if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || !seen.insert(ext.clone()) {
                continue;
            }
            // Le format retenu pour une extension est celui qu'on devinerait d'après le nom.
            let Some(guess) = catalog.guess_from_name(&format!("x.{ext}")) else { continue };
            let kind = vec![guess.category.as_str().to_owned()];
            let items: Vec<MenuItem> = store
                .for_kinds(&kind)
                .filter(|p| !p.is_pointless_for(&guess.id) && can(&guess.id, &p.target))
                .map(|p| MenuItem { preset: p.id.clone(), name: p.display_name().to_owned() })
                .collect();
            if items.is_empty() {
                continue;
            }
            let entry = by_items.entry(items).or_default();
            entry.0.push(ext);
            for m in &guess.mime {
                if !entry.1.contains(m) {
                    entry.1.push(m.clone());
                }
            }
        }
    }
    let mut menus: Vec<Menu> = by_items
        .into_iter()
        .map(|(items, (mut extensions, mime))| {
            extensions.sort();
            Menu { key: extensions[0].clone(), folder: false, extensions, mime, items }
        })
        .collect();
    let folder = vec![FOLDER.to_owned()];
    let dir = FormatId::new("directory");
    let items: Vec<MenuItem> = store
        .for_kinds(&folder)
        .filter(|p| can(&dir, &p.target))
        .map(|p| MenuItem { preset: p.id.clone(), name: p.display_name().to_owned() })
        .collect();
    if !items.is_empty() {
        menus.push(Menu { key: FOLDER.into(), folder: true, extensions: Vec::new(), mime: vec!["inode/directory".into()], items });
    }
    menus.sort_by(|a, b| a.key.cmp(&b.key));
    menus
}

/// Programme graphique à lancer depuis le menu : `chud-converter` à côté de `chud`, ou le
/// programme courant s'il s'agit déjà de lui. Sous Linux, une AppImage est désignée par
/// `$APPIMAGE` (le chemin courant est un montage temporaire).
pub fn default_gui_exe() -> Result<PathBuf, ShellError> {
    if let Some(app) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(app);
    }
    let me = std::env::current_exe()?;
    let name = format!("chud-converter{}", std::env::consts::EXE_SUFFIX);
    if me.file_name().is_some_and(|n| n == name.as_str()) {
        return Ok(me);
    }
    let sibling = me.with_file_name(&name);
    if sibling.is_file() { Ok(sibling) } else { Err(ShellError::MissingExe(sibling)) }
}

fn check_exe(exe: &Path) -> Result<(), ShellError> {
    if exe.is_file() { Ok(()) } else { Err(ShellError::MissingExe(exe.to_path_buf())) }
}

/// Installe (ou met à jour) le menu de ce système pour l'utilisateur courant.
pub fn install(exe: &Path, menus: &[Menu]) -> Result<Status, ShellError> {
    check_exe(exe)?;
    if menus.is_empty() {
        return Err(ShellError::Empty);
    }
    #[cfg(windows)]
    {
        windows::Registry::user().install(exe, menus)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::Desktop::user()?.install(exe, menus)
    }
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    {
        Err(ShellError::Unsupported)
    }
}

/// Retire tout ce qui a été installé (et rien d'autre). Renvoie le nombre d'éléments retirés.
pub fn uninstall() -> Result<usize, ShellError> {
    #[cfg(windows)]
    {
        windows::Registry::user().uninstall()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::Desktop::user()?.uninstall()
    }
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    {
        Err(ShellError::Unsupported)
    }
}

pub fn status() -> Result<Status, ShellError> {
    #[cfg(windows)]
    {
        windows::Registry::user().status()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::Desktop::user()?.status()
    }
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    {
        Err(ShellError::Unsupported)
    }
}

/// Après une modification des préréglages : met le menu à jour s'il est installé (et que le
/// programme enregistré est bien `exe`). Ne l'installe jamais de lui-même.
pub fn refresh_if_installed(exe: &Path, menus: &[Menu]) -> Result<bool, ShellError> {
    let s = status()?;
    if !s.installed || s.exe.as_deref() != Some(exe) {
        return Ok(false);
    }
    if menus.is_empty() {
        uninstall()?;
    } else {
        install(exe, menus)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menus() -> Vec<Menu> {
        plan_menus(&PresetStore::builtin(), FormatCatalog::builtin())
    }

    #[test]
    fn menus_follow_presets_and_skip_pointless_entries() {
        let m = menus();
        let find =
            |ext: &str| m.iter().find(|m| m.extensions.iter().any(|e| e == ext)).unwrap_or_else(|| panic!("pas de menu pour .{ext}"));
        let ids = |ext: &str| find(ext).items.iter().map(|i| i.preset.clone()).collect::<Vec<_>>();
        assert!(ids("mkv").contains(&"mp4".to_string()) && !ids("mkv").contains(&"mkv".to_string()));
        assert!(ids("mp4").contains(&"mkv".to_string()) && !ids("mp4").contains(&"mp4".to_string()));
        // MP4 léger a des options : il reste proposé sur un MP4.
        assert!(ids("mp4").contains(&"mp4-leger".to_string()));
        assert!(ids("png").contains(&"jpeg".to_string()) && !ids("png").contains(&"png".to_string()));
        assert!(ids("wav").contains(&"mp3".to_string()));
        assert!(find("mkv").mime.iter().any(|m| m.contains("matroska")), "{:?}", find("mkv").mime);
        let folder = m.iter().find(|m| m.folder).expect("menu des dossiers");
        assert!(folder.items.iter().any(|i| i.preset == "zip"));
        // Extensions sûres pour un nom de clé ou de fichier.
        for menu in &m {
            assert!(menu.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{}", menu.key);
        }
        // Aucun menu pour une extension sans préréglage (une police, par exemple).
        assert!(!m.iter().any(|m| m.extensions.iter().any(|e| e == "ttf")));
    }

    #[test]
    fn only_feasible_conversions_are_offered() {
        // Sans moteur vidéo : plus de menu pour .mkv, les images restent.
        let m = plan_menus_with(&PresetStore::builtin(), FormatCatalog::builtin(), |src, _| {
            FormatCatalog::builtin().get(src.as_str()).is_some_and(|d| d.category != chud_core::Category::Video)
        });
        assert!(!m.iter().any(|m| m.extensions.iter().any(|e| e == "mkv")));
        assert!(m.iter().any(|m| m.extensions.iter().any(|e| e == "png")));
        // .tsv est un tableau de données, pas une vidéo.
        let all = menus();
        let tsv = all.iter().find(|m| m.extensions.iter().any(|e| e == "tsv")).unwrap();
        assert!(tsv.items.iter().any(|i| i.preset == "json"), "{:?}", tsv.items);
    }

    #[test]
    fn disabled_presets_disappear() {
        let mut store = PresetStore::builtin();
        for p in &mut store.presets {
            p.enabled = p.id == "zip";
        }
        let m = plan_menus(&store, FormatCatalog::builtin());
        // ZIP vise les dossiers et les archives (sauf les .zip eux-mêmes).
        assert!(m.iter().any(|m| m.folder));
        assert!(m.iter().all(|m| m.items.iter().all(|i| i.preset == "zip")));
        assert!(!m.iter().any(|m| m.extensions.iter().any(|e| e == "zip")));
        for p in &mut store.presets {
            p.enabled = false;
        }
        assert!(plan_menus(&store, FormatCatalog::builtin()).is_empty());
    }
}
