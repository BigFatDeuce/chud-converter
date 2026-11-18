//! Menu de l'Explorateur Windows, par clés du registre de l'utilisateur (HKCU).
//!
//! Pour chaque extension : `Software\Classes\SystemFileAssociations\.<ext>\shell\ChudConverter`
//! (un menu en cascade, `ExtendedSubCommandsKey`) qui renvoie vers
//! `Software\Classes\ChudConverter.Menu.<clé>\shell\<nn>-<préréglage>\command`. Les dossiers
//! passent par `Directory\shell\ChudConverter`. Aucune association de fichiers n'est modifiée :
//! le programme par défaut de chaque type reste celui choisi par l'utilisateur.
//!
//! `MultiSelectModel=Player` : le menu reste visible au-delà de 15 fichiers ; l'Explorateur lance
//! alors un processus par fichier, qui rejoint la première instance de l'application.
//!
//! Sous Windows 11, ces entrées apparaissent dans « Afficher plus d'options » (Maj+F10) : le menu
//! raccourci de Windows 11 n'accepte que des extensions empaquetées (prévu plus tard).

use crate::{Menu, ShellError, Status};
use std::path::{Path, PathBuf};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const VERB: &str = "ChudConverter";
const MENU_PREFIX: &str = "ChudConverter.Menu.";
/// `ECF_SEPARATORBEFORE` : trait de séparation avant l'entrée.
const SEPARATOR_BEFORE: u32 = 0x20;

pub struct Registry {
    /// Équivalent de HKEY_CLASSES_ROOT pour l'utilisateur.
    classes: String,
    /// Liste de ce qui a été créé.
    manifest: String,
}

/// Ligne de commande d'une entrée. Le chemin du fichier choisi arrive par `"%1"` : un argument
/// séparé, jamais interprété. L'identifiant du préréglage est restreint à `[a-z0-9-]`.
pub fn command(exe: &Path, preset: Option<&str>) -> String {
    match preset {
        Some(id) => format!("\"{}\" --preset {id} -- \"%1\"", exe.display()),
        None => format!("\"{}\" -- \"%1\"", exe.display()),
    }
}

fn is_ours(key: &str) -> bool {
    let ext_ok = |e: &str| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if let Some(rest) = key.strip_prefix(MENU_PREFIX) {
        return ext_ok(rest);
    }
    if key == format!("Directory\\shell\\{VERB}") {
        return true;
    }
    key.strip_prefix("SystemFileAssociations\\.").and_then(|r| r.strip_suffix(&format!("\\shell\\{VERB}"))).is_some_and(ext_ok)
}

fn is_our_parent(key: &str) -> bool {
    let ext_ok = |e: &str| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    key.strip_prefix("SystemFileAssociations\\.").map(|r| r.strip_suffix("\\shell").unwrap_or(r)).is_some_and(ext_ok)
}

impl Registry {
    pub fn user() -> Self {
        Self { classes: "Software\\Classes".into(), manifest: "Software\\ChudConverter\\Shell".into() }
    }

    /// Emplacements choisis (tests).
    pub fn at(classes: &str, manifest: &str) -> Self {
        Self { classes: classes.into(), manifest: manifest.into() }
    }

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn path(&self, rel: &str) -> String {
        format!("{}\\{rel}", self.classes)
    }

    fn exists(&self, rel: &str) -> bool {
        Self::hkcu().open_subkey_with_flags(self.path(rel), KEY_READ).is_ok()
    }

    pub fn install(&self, exe: &Path, menus: &[Menu]) -> Result<Status, ShellError> {
        self.uninstall()?;
        let hkcu = Self::hkcu();
        let mut keys: Vec<String> = Vec::new();
        let mut parents: Vec<String> = Vec::new();
        let icon = format!("\"{}\",0", exe.display());
        let mut entries = 0usize;

        for menu in menus {
            let menu_key = format!("{MENU_PREFIX}{}", menu.key);
            let (k, _) = hkcu.create_subkey(self.path(&menu_key))?;
            keys.push(menu_key.clone());
            k.set_value("MUIVerb", &"Chud Converter")?;
            for (i, item) in menu.items.iter().enumerate() {
                let (v, _) = hkcu.create_subkey(self.path(&format!("{menu_key}\\shell\\{:02}-{}", i + 1, item.preset)))?;
                v.set_value("MUIVerb", &item.name)?;
                v.set_value("Icon", &icon)?;
                let (c, _) = v.create_subkey("command")?;
                c.set_value("", &command(exe, Some(&item.preset)))?;
            }
            let (open, _) = hkcu.create_subkey(self.path(&format!("{menu_key}\\shell\\99-ouvrir")))?;
            open.set_value("MUIVerb", &chud_core::tr!("Ouvrir dans Chud Converter…", "Open in Chud Converter…"))?;
            open.set_value("CommandFlags", &SEPARATOR_BEFORE)?;
            let (c, _) = open.create_subkey("command")?;
            c.set_value("", &command(exe, None))?;

            let targets: Vec<String> = if menu.folder {
                vec![format!("Directory\\shell\\{VERB}")]
            } else {
                menu.extensions.iter().map(|e| format!("SystemFileAssociations\\.{e}\\shell\\{VERB}")).collect()
            };
            for t in targets {
                // Clés parentes créées par nous : retirées à la désinstallation si elles sont vides.
                if let Some(ext_key) = t.strip_suffix(&format!("\\shell\\{VERB}")).filter(|_| !menu.folder) {
                    for parent in [ext_key.to_owned(), format!("{ext_key}\\shell")] {
                        if !self.exists(&parent) {
                            parents.push(parent);
                        }
                    }
                }
                let (v, _) = hkcu.create_subkey(self.path(&t))?;
                v.set_value("MUIVerb", &"Chud Converter")?;
                v.set_value("Icon", &icon)?;
                v.set_value("ExtendedSubCommandsKey", &menu_key)?;
                v.set_value("MultiSelectModel", &"Player")?;
                keys.push(t);
                entries += 1;
            }
        }

        let (m, _) = hkcu.create_subkey(&self.manifest)?;
        m.set_value("Exe", &exe.display().to_string())?;
        m.set_value("Keys", &keys)?;
        m.set_value("Parents", &parents)?;
        m.set_value("Entries", &(entries as u32))?;
        m.set_value("Version", &env!("CARGO_PKG_VERSION"))?;
        self.status()
    }

    /// Retire exactement ce que la liste mentionne. Si une entrée de la liste n'a pas la forme
    /// de nos clés, rien n'est retiré (la liste a pu être modifiée par autre chose).
    pub fn uninstall(&self) -> Result<usize, ShellError> {
        let hkcu = Self::hkcu();
        let Ok(m) = hkcu.open_subkey_with_flags(&self.manifest, KEY_READ) else { return Ok(0) };
        let keys: Vec<String> = m.get_value("Keys").unwrap_or_default();
        let parents: Vec<String> = m.get_value("Parents").unwrap_or_default();
        if let Some(bad) = keys.iter().find(|k| !is_ours(k)).or_else(|| parents.iter().find(|k| !is_our_parent(k))) {
            return Err(ShellError::Suspicious(bad.clone()));
        }
        let mut removed = 0;
        for k in &keys {
            match hkcu.delete_subkey_all(self.path(k)) {
                Ok(()) => removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        // Les plus profondes d'abord ; seulement si elles sont restées vides.
        let mut parents = parents;
        parents.sort_by_key(|p| std::cmp::Reverse(p.len()));
        for p in &parents {
            if let Ok(k) = hkcu.open_subkey_with_flags(self.path(p), KEY_READ) {
                let info = k.query_info()?;
                if info.sub_keys == 0 && info.values == 0 {
                    drop(k);
                    let _ = hkcu.delete_subkey(self.path(p));
                }
            }
        }
        drop(m);
        hkcu.delete_subkey_all(&self.manifest)?;
        // Le dossier parent de la liste (Software\ChudConverter) s'il ne contient plus rien.
        if let Some((parent, _)) = self.manifest.rsplit_once('\\')
            && let Ok(k) = hkcu.open_subkey_with_flags(parent, KEY_READ | KEY_WRITE)
            && k.query_info().is_ok_and(|i| i.sub_keys == 0 && i.values == 0)
        {
            drop(k);
            let _ = hkcu.delete_subkey(parent);
        }
        Ok(removed)
    }

    pub fn status(&self) -> Result<Status, ShellError> {
        let location = chud_core::trf!("registre de l'utilisateur (HKCU\\{})", "user registry (HKCU\\{})", self.classes);
        let Ok(m) = Self::hkcu().open_subkey_with_flags(&self.manifest, KEY_READ) else {
            return Ok(Status { location, ..Status::default() });
        };
        let exe: Option<PathBuf> = m.get_value::<String, _>("Exe").ok().map(PathBuf::from);
        let entries: u32 = m.get_value("Entries").unwrap_or(0);
        Ok(Status { installed: true, exe_missing: exe.as_ref().is_none_or(|e| !e.is_file()), exe, entries: entries as usize, location })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_menus;
    use chud_core::FormatCatalog;
    use chud_jobs::PresetStore;

    #[test]
    fn recognizes_only_our_keys() {
        assert!(is_ours("SystemFileAssociations\\.mkv\\shell\\ChudConverter"));
        assert!(is_ours("ChudConverter.Menu.mkv"));
        assert!(is_ours("Directory\\shell\\ChudConverter"));
        assert!(!is_ours("SystemFileAssociations\\.mkv\\shell\\open"));
        assert!(!is_ours("SystemFileAssociations\\.mkv"));
        assert!(!is_ours("ChudConverter.Menu.a\\..\\b"));
        assert!(!is_ours("Directory\\shell"));
        assert!(is_our_parent("SystemFileAssociations\\.mkv\\shell"));
        assert!(!is_our_parent("Directory\\shell"));
        assert_eq!(
            command(Path::new("C:\\A B\\chud-converter.exe"), Some("mp4")),
            "\"C:\\A B\\chud-converter.exe\" --preset mp4 -- \"%1\""
        );
    }

    #[test]
    fn installs_and_removes_exactly_its_keys() {
        let base = format!("Software\\ChudConverterTest-{}", std::process::id());
        let reg = Registry::at(&format!("{base}\\Classes"), &format!("{base}\\Manifest"));
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        // Une clé existante d'un autre programme ne doit pas être touchée.
        let (other, _) = hkcu.create_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.mkv\\shell\\open")).unwrap();
        other.set_value("", &"autre programme").unwrap();

        let exe = std::env::current_exe().unwrap();
        let menus = plan_menus(&PresetStore::builtin(), FormatCatalog::builtin());
        let st = reg.install(&exe, &menus).unwrap();
        assert!(st.installed && !st.exe_missing && st.entries > 20, "{st:?}");

        let verb = hkcu.open_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.mkv\\shell\\ChudConverter")).unwrap();
        let sub: String = verb.get_value("ExtendedSubCommandsKey").unwrap();
        assert!(sub.starts_with(MENU_PREFIX));
        let shell = hkcu.open_subkey(format!("{base}\\Classes\\{sub}\\shell")).unwrap();
        let names: Vec<String> = shell.enum_keys().map(|k| k.unwrap()).collect();
        assert!(names.iter().any(|n| n.ends_with("-mp4")), "{names:?}");
        assert!(!names.iter().any(|n| n.ends_with("-mkv")), "{names:?}");
        let mp4 = names.iter().find(|n| n.ends_with("-mp4")).unwrap();
        let cmd: String = shell.open_subkey(format!("{mp4}\\command")).unwrap().get_value("").unwrap();
        assert_eq!(cmd, command(&exe, Some("mp4")));
        assert!(hkcu.open_subkey(format!("{base}\\Classes\\Directory\\shell\\ChudConverter")).is_ok());

        // Réinstaller remplace sans dupliquer.
        let again = reg.install(&exe, &menus).unwrap();
        assert_eq!(again.entries, st.entries);

        let removed = reg.uninstall().unwrap();
        assert!(removed > 20);
        assert!(!reg.status().unwrap().installed);
        assert!(hkcu.open_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.mkv\\shell\\ChudConverter")).is_err());
        assert!(hkcu.open_subkey(format!("{base}\\Classes\\{sub}")).is_err());
        // L'autre programme est intact ; les clés parentes que nous avions créées ont disparu.
        let kept: String =
            hkcu.open_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.mkv\\shell\\open")).unwrap().get_value("").unwrap();
        assert_eq!(kept, "autre programme");
        assert!(hkcu.open_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.webm")).is_err());

        // Une liste trafiquée n'entraîne aucune suppression.
        let (m, _) = hkcu.create_subkey(format!("{base}\\Manifest")).unwrap();
        m.set_value("Keys", &vec!["SystemFileAssociations\\.mkv\\shell\\open".to_string()]).unwrap();
        assert!(matches!(reg.uninstall(), Err(ShellError::Suspicious(_))));
        assert!(hkcu.open_subkey(format!("{base}\\Classes\\SystemFileAssociations\\.mkv\\shell\\open")).is_ok());

        hkcu.delete_subkey_all(&base).unwrap();
    }
}
