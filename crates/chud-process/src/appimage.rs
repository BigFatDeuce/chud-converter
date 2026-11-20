//! Programmes lancÃ©s depuis une AppImage.
//!
//! Le lanceur d'une AppImage met ses propres dossiers en tÃªte de `LD_LIBRARY_PATH`, `PATH`,
//! `PYTHONPATH`, `PERLLIB`, `XDG_DATA_DIRS`â€¦ et fait pointer `GTK_PATH`, `GIO_MODULE_DIR`â€¦
//! dans son point de montage. C'est voulu pour l'application seule : un programme du systÃ¨me
//! qui en hÃ©rite charge les bibliothÃ¨ques embarquÃ©es (construites pour la plus ancienne
//! distribution prise en charge) au lieu des siennes et s'arrÃªte dÃ¨s le dÃ©marrage
//! (Â« symbol lookup error: /lib64/libcurl.so.4: undefined symbol: nghttp2_â€¦ Â», code 127 ;
//! vu avec LibreOffice, libvips, Tesseract et MediaInfo sur Nobara/Fedora).
//!
//! Chaque programme lancÃ© reÃ§oit donc l'environnement d'avant l'AppImage : les chemins situÃ©s
//! sous `$APPDIR` sont retirÃ©s des variables, et une variable qui ne contenait qu'eux disparaÃ®t.
//! Ce que l'utilisateur avait mis lui-mÃªme dans ces variables est conservÃ©, dans l'ordre.

use std::ffi::OsString;
use std::process::Command;

/// Une correction : `Some(valeur)` remplace la valeur hÃ©ritÃ©e, `None` retire la variable.
pub type EnvFix = (OsString, Option<OsString>);

/// Donne Ã  `cmd` l'environnement du systÃ¨me quand ce programme tourne depuis une AppImage ;
/// sans effet sinon.
pub fn host_environment(cmd: &mut Command) {
    for (k, v) in fixes() {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
}

/// Ce programme tourne-t-il depuis une AppImage ?
pub fn active() -> bool {
    !mount_dirs().is_empty()
}

/// Lance le premier des lanceurs donnÃ©s (`xdg-open`, `gio open`â€¦) qui accepte, dÃ©tachÃ© de
/// l'application et avec l'environnement du systÃ¨me. Un lanceur qui Ã©choue dans la seconde
/// laisse la place au suivant ; un lanceur encore actif aprÃ¨s ce dÃ©lai est considÃ©rÃ© comme
/// parti (il est rÃ©cupÃ©rÃ© en arriÃ¨re-plan quand il se termine).
///
/// Ã€ part de [`crate::run`] : le programme est cherchÃ© dans le `PATH` du systÃ¨me et peut
/// survivre Ã  l'application (navigateur, gestionnaire de fichiers).
#[cfg(unix)]
pub fn launch_detached(launchers: Vec<Command>) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "aucun lanceur / no launcher");
    for mut cmd in launchers {
        host_environment(&mut cmd);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let start = Instant::now();
        loop {
            match child.try_wait()? {
                Some(st) if st.success() => return Ok(()),
                Some(st) => {
                    last = std::io::Error::other(format!("{} : {st}", cmd.get_program().to_string_lossy()));
                    break;
                }
                None if start.elapsed() >= Duration::from_secs(1) => {
                    std::thread::spawn(move || child.wait());
                    return Ok(());
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    Err(last)
}

/// Corrections Ã  apporter Ã  l'environnement hÃ©ritÃ© (vide hors AppImage).
pub fn fixes() -> Vec<EnvFix> {
    let dirs = mount_dirs();
    if dirs.is_empty() {
        return Vec::new();
    }
    fixes_for(std::env::vars_os(), &dirs)
}

/// Dossiers de l'AppImage en cours, sans `/` final : `$APPDIR` tel qu'Ã©crit et son chemin rÃ©el.
/// Vide si ce programme n'est pas lui-mÃªme dans `$APPDIR` (la variable peut traÃ®ner ailleurs).
#[cfg(unix)]
fn mount_dirs() -> Vec<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    let Some(appdir) = std::env::var_os("APPDIR").map(PathBuf::from) else {
        return Vec::new();
    };
    if !appdir.is_absolute() || appdir.parent().is_none() {
        return Vec::new();
    }
    let real = std::fs::canonicalize(&appdir).unwrap_or_else(|_| appdir.clone());
    let exe = std::env::current_exe().ok().map(|e| std::fs::canonicalize(&e).unwrap_or(e));
    if !exe.is_some_and(|e| e.starts_with(&real) || e.starts_with(&appdir)) {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    for d in [&appdir, &real] {
        let b = trim_slashes(d.as_os_str().as_bytes());
        if !b.is_empty() && !dirs.iter().any(|x: &Vec<u8>| x == b) {
            dirs.push(b.to_vec());
        }
    }
    dirs
}

#[cfg(not(unix))]
fn mount_dirs() -> Vec<Vec<u8>> {
    Vec::new()
}

#[cfg_attr(not(unix), allow(dead_code))]
fn trim_slashes(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 1 && b[end - 1] == b'/' {
        end -= 1;
    }
    &b[..end]
}

fn fixes_for(vars: impl IntoIterator<Item = (OsString, OsString)>, dirs: &[Vec<u8>]) -> Vec<EnvFix> {
    vars.into_iter()
        .filter_map(|(k, v)| {
            let new = without_dirs(v.as_encoded_bytes(), dirs)?;
            Some((k, new.map(os_string)))
        })
        .collect()
}

/// Valeur sans les chemins situÃ©s sous `dirs` : `None` s'il n'y en avait aucun (rien Ã  changer),
/// `Some(None)` s'il ne reste rien. Les Ã©lÃ©ments vides partent aussi : le lanceur en ajoute un
/// (`:` final) quand la variable n'existait pas, et un Ã©lÃ©ment vide dÃ©signe le dossier courant.
fn without_dirs(value: &[u8], dirs: &[Vec<u8>]) -> Option<Option<Vec<u8>>> {
    let inside = |p: &[u8]| dirs.iter().any(|d| p.starts_with(d) && (p.len() == d.len() || p[d.len()] == b'/'));
    if !value.split(|&b| b == b':').any(inside) {
        return None;
    }
    let kept: Vec<&[u8]> = value.split(|&b| b == b':').filter(|p| !p.is_empty() && !inside(p)).collect();
    Some((!kept.is_empty()).then(|| kept.join(&b':')))
}

#[cfg(unix)]
fn os_string(b: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b)
}

/// Hors Unix il n'y a pas d'AppImage : seuls les tests passent ici, avec de l'ASCII.
#[cfg(not(unix))]
fn os_string(b: Vec<u8>) -> OsString {
    String::from_utf8_lossy(&b).into_owned().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "/tmp/.mount_ChudCoAbC123";

    fn fix(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let vars = vars.iter().map(|(k, v)| (OsString::from(k), OsString::from(v)));
        fixes_for(vars, &[A.as_bytes().to_vec()])
            .into_iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect()
    }

    /// `LD_LIBRARY_PATH` tel que l'Ã©crit le lanceur (AppRun d'AppImageKit) quand la variable
    /// n'existait pas : elle disparaÃ®t, sans laisser l'Ã©lÃ©ment vide (dossier courant).
    #[test]
    fn launcher_library_path_is_removed() {
        let ld = format!("{A}/usr/lib/:{A}/usr/lib/i386-linux-gnu/:{A}/usr/lib/x86_64-linux-gnu/:{A}/lib64/:");
        assert_eq!(fix(&[("LD_LIBRARY_PATH", ld.as_str())]), vec![("LD_LIBRARY_PATH".into(), None)]);
    }

    #[test]
    fn user_entries_are_kept_in_order() {
        let ld = format!("{A}/usr/lib/:{A}/lib64/:/opt/cuda/lib64:/home/moi/lib");
        let path = format!("{A}/usr/bin/:{A}/usr/sbin/:/usr/local/bin:/usr/bin:/bin");
        let data = format!("{A}/usr/share/:{A}/usr/share:/usr/share:/usr/local/share:/usr/share");
        assert_eq!(
            fix(&[("LD_LIBRARY_PATH", ld.as_str()), ("PATH", path.as_str()), ("XDG_DATA_DIRS", data.as_str())]),
            vec![
                ("LD_LIBRARY_PATH".into(), Some("/opt/cuda/lib64:/home/moi/lib".into())),
                ("PATH".into(), Some("/usr/local/bin:/usr/bin:/bin".into())),
                ("XDG_DATA_DIRS".into(), Some("/usr/share:/usr/local/share:/usr/share".into())),
            ]
        );
    }

    /// Variables du crochet GTK (`$APPDIR//usr/â€¦`) et dossier lui-mÃªme : retirÃ©es.
    #[test]
    fn single_paths_inside_are_removed() {
        let gtk = format!("{A}//usr/lib/gtk-3.0");
        let r = fix(&[
            ("GTK_PATH", gtk.as_str()),
            ("GTK_DATA_PREFIX", A),
            ("PERLLIB", format!("{A}/usr/share/perl5/:{A}/usr/lib/perl5/:").as_str()),
        ]);
        assert_eq!(r, vec![("GTK_PATH".into(), None), ("GTK_DATA_PREFIX".into(), None), ("PERLLIB".into(), None)]);
    }

    #[test]
    fn unrelated_values_are_untouched() {
        let sibling = format!("{A}2/usr/lib:/usr/lib");
        let r = fix(&[
            ("DISPLAY", ":0"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("LD_LIBRARY_PATH", sibling.as_str()),
            ("APPIMAGE", "/home/moi/Applications/Chud.Converter_0.2.1_amd64.AppImage"),
            ("OPTION", format!("--dir={A}/usr").as_str()),
            ("EMPTY", ""),
        ]);
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn trailing_slashes_do_not_matter() {
        assert_eq!(trim_slashes(b"/tmp/.mount_x//"), b"/tmp/.mount_x");
        assert_eq!(trim_slashes(b"/"), b"/");
    }

    #[test]
    fn nothing_changes_outside_an_appimage() {
        // Les tests ne tournent jamais depuis une AppImage.
        assert!(fixes().is_empty());
        assert!(!active());
    }

    /// Un lanceur qui Ã©choue passe la main au suivant ; un lanceur introuvable aussi.
    #[cfg(unix)]
    #[test]
    fn launch_falls_back_to_next_launcher() {
        let dir = std::env::temp_dir().join(format!("chud-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mark = dir.join("ok");
        let mut ok = Command::new("sh");
        ok.arg("-c").arg("printf lance > \"$1\"").arg("sh").arg(&mark);
        launch_detached(vec![Command::new("chud-lanceur-introuvable"), Command::new("false"), ok]).unwrap();
        assert_eq!(std::fs::read_to_string(&mark).unwrap(), "lance");
        assert!(launch_detached(vec![Command::new("false")]).is_err());
        assert!(launch_detached(Vec::new()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un lanceur qui reste actif (navigateur lancÃ© au premier plan) ne bloque pas l'appel.
    #[cfg(unix)]
    #[test]
    fn long_running_launcher_counts_as_started() {
        let start = std::time::Instant::now();
        let mut slow = Command::new("sleep");
        slow.arg("5");
        launch_detached(vec![slow]).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(3), "{:?}", start.elapsed());
    }

    /// Le programme lancÃ© voit bien la valeur corrigÃ©e, et une variable retirÃ©e n'existe plus.
    #[cfg(unix)]
    #[test]
    fn child_receives_fixed_environment() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf '%s|%s' \"$CHUD_T_LIST\" \"${CHUD_T_GONE-absente}\""])
            .env("CHUD_T_LIST", format!("{A}/usr/lib:/opt/lib"))
            .env("CHUD_T_GONE", format!("{A}/usr/lib"));
        let vars = [("CHUD_T_LIST", format!("{A}/usr/lib:/opt/lib")), ("CHUD_T_GONE", format!("{A}/usr/lib"))];
        for (k, v) in fixes_for(vars.map(|(k, v)| (OsString::from(k), OsString::from(v))), &[A.as_bytes().to_vec()]) {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        let out = cmd.output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "/opt/lib|absente");
    }
}
