//! Menus des gestionnaires de fichiers Linux, dans le dossier de l'utilisateur
//! (`$XDG_DATA_HOME`, par défaut `~/.local/share`) :
//!
//! - **Dolphin** (KDE) : `kio/servicemenus/chud-converter-<clé>.desktop`, un sous-menu
//!   « Chud Converter » par groupe d'extensions, filtré par type MIME ;
//! - **Nemo** (Cinnamon) : `nemo/actions/chud-converter-<préréglage>.nemo_action`, filtré par
//!   extension ;
//! - **Nautilus** (GNOME) : `nautilus/scripts/Chud Converter/<nom>`. Nautilus ne sait pas filtrer
//!   ses scripts par type : tous les préréglages y figurent, et un fichier qui ne convient pas est
//!   signalé dans la fenêtre de conversion au lieu d'être converti.
//!
//! Thunar (XFCE) range ses actions dans un seul fichier partagé (`uca.xml`) : il n'est pas
//! modifié automatiquement.
//!
//! Chaque fichier créé porte une ligne de marque et figure dans une liste
//! (`chud-converter/shell-files.txt`) : la désinstallation ne retire que ces fichiers-là.
//! Le code est compilé partout pour être testé partout ; il n'est appelé que sous Linux.

use crate::{Menu, ShellError, Status};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MARKER: &str = "# Créé par Chud Converter (menu clic droit) ; retiré par « chud shell uninstall ».";
const PREFIX: &str = "chud-converter-";
const NAUTILUS_DIR: &str = "Chud Converter";

pub struct Desktop {
    data_home: PathBuf,
}

/// Fichier à écrire : chemin relatif à `data_home`, contenu, exécutable ou non.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub rel: PathBuf,
    pub content: String,
    pub executable: bool,
}

fn check_path(exe: &Path) -> Result<String, ShellError> {
    let s = exe.to_str().ok_or_else(|| ShellError::MissingExe(exe.to_path_buf()))?.to_owned();
    // Caractères qui exigeraient un échappement à plusieurs niveaux dans les fichiers .desktop :
    // refusés plutôt que risqués.
    // (La barre oblique inverse n'est refusée que sous Unix, où elle n'est jamais un séparateur.)
    if s.chars().any(|c| matches!(c, '"' | '`' | '$' | '%' | '\n' | '\r') || (cfg!(unix) && c == '\\')) {
        return Err(ShellError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("chemin du programme non pris en charge pour un menu : {s}"),
        )));
    }
    Ok(s)
}

/// Nom de fichier lisible et sûr tiré d'un nom de préréglage.
fn file_name(name: &str) -> String {
    name.chars().map(|c| if c == '/' || c == '\0' || c.is_control() { '-' } else { c }).collect::<String>().trim().to_owned()
}

/// Contenus à écrire pour ces menus.
pub fn plan_files(exe: &Path, menus: &[Menu]) -> Result<Vec<Planned>, ShellError> {
    let exe = check_path(exe)?;
    let mut out = Vec::new();

    // Dolphin : un fichier par sous-menu.
    for menu in menus {
        let actions: Vec<String> = menu.items.iter().enumerate().map(|(i, _)| format!("p{i}")).chain(["open".to_owned()]).collect();
        let mut s = format!(
            "{MARKER}\n[Desktop Entry]\nType=Service\nMimeType={};\nActions={};\nX-KDE-Submenu=Chud Converter\nX-KDE-Priority=TopLevel\n",
            menu.mime.join(";"),
            actions.join(";")
        );
        for (i, item) in menu.items.iter().enumerate() {
            s.push_str(&format!("\n[Desktop Action p{i}]\nName={}\nExec=\"{exe}\" --preset {} -- %F\n", item.name, item.preset));
        }
        s.push_str(&format!("\n[Desktop Action open]\nName=Ouvrir dans Chud Converter…\nExec=\"{exe}\" -- %F\n"));
        out.push(Planned {
            rel: Path::new("kio/servicemenus").join(format!("{PREFIX}{}.desktop", menu.key)),
            content: s,
            executable: true,
        });
    }

    // Nemo : une action par préréglage, avec toutes les extensions où il a un sens.
    let mut per_preset: BTreeMap<(usize, String), (String, Vec<String>)> = BTreeMap::new();
    let order: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for m in menus {
            for it in &m.items {
                if !v.contains(&it.preset) {
                    v.push(it.preset.clone());
                }
            }
        }
        v
    };
    for m in menus {
        for it in &m.items {
            let idx = order.iter().position(|p| *p == it.preset).unwrap_or(0);
            let e = per_preset.entry((idx, it.preset.clone())).or_insert_with(|| (it.name.clone(), Vec::new()));
            if m.folder {
                e.1.push("dir".into());
            } else {
                e.1.extend(m.extensions.iter().cloned());
            }
        }
    }
    for ((_, id), (name, exts)) in &per_preset {
        out.push(Planned {
            rel: Path::new("nemo/actions").join(format!("{PREFIX}{id}.nemo_action")),
            content: format!(
                "{MARKER}\n[Nemo Action]\nName=Chud Converter : {name}\nComment=Convertir avec le préréglage « {name} »\nExec=\"{exe}\" --preset {id} -- %F\nIcon-Name=document-save-as\nSelection=NotNone\nExtensions={};\nQuote=double\n",
                exts.join(";")
            ),
            executable: false,
        });
    }

    // Nautilus : scripts, sans filtre possible par type.
    let quoted = format!("'{}'", exe.replace('\'', "'\\''"));
    for (i, id) in order.iter().enumerate() {
        let name = per_preset.iter().find(|((_, p), _)| p == id).map(|(_, (n, _))| n.clone()).unwrap_or_else(|| id.clone());
        out.push(Planned {
            rel: Path::new("nautilus/scripts").join(NAUTILUS_DIR).join(format!("{:02} {}", i + 1, file_name(&name))),
            content: format!("#!/bin/sh\n{MARKER}\nexec {quoted} --preset {id} -- \"$@\"\n"),
            executable: true,
        });
    }
    out.push(Planned {
        rel: Path::new("nautilus/scripts").join(NAUTILUS_DIR).join("99 Ouvrir dans Chud Converter"),
        content: format!("#!/bin/sh\n{MARKER}\nexec {quoted} -- \"$@\"\n"),
        executable: true,
    });
    Ok(out)
}

impl Desktop {
    pub fn user() -> Result<Self, ShellError> {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or(ShellError::Unsupported)?;
        Ok(Self { data_home })
    }

    pub fn at(data_home: &Path) -> Self {
        Self { data_home: data_home.to_path_buf() }
    }

    fn manifest(&self) -> PathBuf {
        self.data_home.join("chud-converter/shell-files.txt")
    }

    /// Un chemin de la liste n'est retiré que s'il est bien l'un des nôtres.
    fn is_ours(&self, p: &Path) -> bool {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let parent = p.parent().unwrap_or(Path::new(""));
        let ok_place = (parent == self.data_home.join("kio/servicemenus") && name.starts_with(PREFIX) && name.ends_with(".desktop"))
            || (parent == self.data_home.join("nemo/actions") && name.starts_with(PREFIX) && name.ends_with(".nemo_action"))
            || parent == self.data_home.join("nautilus/scripts").join(NAUTILUS_DIR);
        ok_place && std::fs::read_to_string(p).is_ok_and(|t| t.lines().take(2).any(|l| l == MARKER))
    }

    pub fn install(&self, exe: &Path, menus: &[Menu]) -> Result<Status, ShellError> {
        let files = plan_files(exe, menus)?;
        self.uninstall()?;
        let mut written = Vec::new();
        for f in &files {
            let path = self.data_home.join(&f.rel);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, &f.content)?;
            #[cfg(unix)]
            if f.executable {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
            }
            written.push(path);
        }
        let manifest = self.manifest();
        if let Some(dir) = manifest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = format!("exe={}\nentries={}\n", exe.display(), menus.iter().map(|m| m.extensions.len().max(1)).sum::<usize>());
        for p in &written {
            text.push_str(&format!("file={}\n", p.display()));
        }
        std::fs::write(&manifest, text)?;
        self.status()
    }

    pub fn uninstall(&self) -> Result<usize, ShellError> {
        let manifest = self.manifest();
        let text = match std::fs::read_to_string(&manifest) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let files: Vec<PathBuf> = text.lines().filter_map(|l| l.strip_prefix("file=")).map(PathBuf::from).collect();
        if let Some(bad) = files.iter().find(|p| p.exists() && !self.is_ours(p)) {
            return Err(ShellError::Suspicious(bad.display().to_string()));
        }
        let mut removed = 0;
        for p in &files {
            match std::fs::remove_file(p) {
                Ok(()) => removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        // Le dossier des scripts Nautilus, s'il est vide.
        let _ = std::fs::remove_dir(self.data_home.join("nautilus/scripts").join(NAUTILUS_DIR));
        std::fs::remove_file(&manifest)?;
        let _ = std::fs::remove_dir(manifest.parent().unwrap_or(Path::new("")));
        Ok(removed)
    }

    pub fn status(&self) -> Result<Status, ShellError> {
        let location = format!("{} (Dolphin, Nemo, Nautilus)", self.data_home.display());
        let Ok(text) = std::fs::read_to_string(self.manifest()) else {
            return Ok(Status { location, ..Status::default() });
        };
        let exe = text.lines().find_map(|l| l.strip_prefix("exe=")).map(PathBuf::from);
        let entries = text.lines().find_map(|l| l.strip_prefix("entries=")).and_then(|n| n.parse().ok()).unwrap_or(0);
        Ok(Status { installed: true, exe_missing: exe.as_ref().is_none_or(|e| !e.is_file()), exe, entries, location })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_menus;
    use chud_core::FormatCatalog;
    use chud_jobs::PresetStore;

    #[test]
    fn writes_and_removes_only_its_files() {
        let dir = std::env::temp_dir().join(format!("chud-shell-linux-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let exe = dir.join("bin/chud converter");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        // Un menu d'un autre programme, au même endroit.
        std::fs::create_dir_all(dir.join("kio/servicemenus")).unwrap();
        std::fs::write(dir.join("kio/servicemenus/autre.desktop"), "[Desktop Entry]\n").unwrap();

        let menus = plan_menus(&PresetStore::builtin(), FormatCatalog::builtin());
        let d = Desktop::at(&dir);
        let st = d.install(&exe, &menus).unwrap();
        assert!(st.installed && !st.exe_missing && st.entries > 20);

        let mkv = menus.iter().find(|m| m.extensions.iter().any(|e| e == "mkv")).unwrap();
        let dolphin = std::fs::read_to_string(dir.join(format!("kio/servicemenus/chud-converter-{}.desktop", mkv.key))).unwrap();
        assert!(dolphin.contains("X-KDE-Submenu=Chud Converter"));
        assert!(dolphin.contains(&format!("Exec=\"{}\" --preset mp4 -- %F", exe.display())));
        assert!(dolphin.contains("video/x-matroska"), "{dolphin}");
        let nemo = std::fs::read_to_string(dir.join("nemo/actions/chud-converter-zip.nemo_action")).unwrap();
        assert!(nemo.contains(";dir;") && nemo.contains("7z;"), "{nemo}");
        let script = std::fs::read_dir(dir.join("nautilus/scripts/Chud Converter")).unwrap().count();
        assert!(script > 10);
        let first = std::fs::read_to_string(dir.join("nautilus/scripts/Chud Converter/01 MP4 (vidéo)"));
        if let Ok(s) = first {
            assert!(s.contains("'/") || s.contains("'C:"), "{s}");
            assert!(s.ends_with("-- \"$@\"\n"));
        }

        let removed = d.uninstall().unwrap();
        assert!(removed > 30);
        assert!(!d.status().unwrap().installed);
        assert!(dir.join("kio/servicemenus/autre.desktop").is_file());
        assert!(!dir.join("nautilus/scripts/Chud Converter").exists());

        // Liste trafiquée : rien n'est supprimé.
        std::fs::create_dir_all(dir.join("chud-converter")).unwrap();
        std::fs::write(
            dir.join("chud-converter/shell-files.txt"),
            format!("file={}\n", dir.join("kio/servicemenus/autre.desktop").display()),
        )
        .unwrap();
        assert!(matches!(d.uninstall(), Err(ShellError::Suspicious(_))));
        assert!(dir.join("kio/servicemenus/autre.desktop").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_hard_to_quote_program_paths() {
        assert!(plan_files(Path::new("/opt/a$b/chud-converter"), &[]).is_err());
        assert!(plan_files(Path::new("/opt/ok dir/chud-converter"), &[]).is_ok());
    }
}
