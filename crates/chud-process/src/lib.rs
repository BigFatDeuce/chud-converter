//! Exécution durcie de processus externes.
//!
//! Garanties :
//! - jamais de shell : programme **absolu** + vecteur d'arguments ;
//! - confinement de l'arbre de processus (Job Object Windows avec `KILL_ON_JOB_CLOSE`,
//!   groupe de processus Unix) : l'annulation et le délai tuent aussi les sous-processus ;
//! - limite mémoire optionnelle (Job Object / `RLIMIT_AS`) ;
//! - lecture ligne à ligne de stdout/stderr (séparateurs `\n`, `\r` et retour arrière),
//!   capture bornée de stdout et conservation de la fin de stderr pour les rapports d'erreur ;
//! - aucune fenêtre console ne s'ouvre sous Windows ;
//! - depuis une AppImage, le programme lancé reçoit l'environnement du système, pas celui de
//!   l'AppImage (voir [`appimage`]).

pub mod appimage;

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Arguments transmis tels quels, sans guillemets ajoutés (Windows : pour `msiexec`, qui
    /// analyse lui-même `PROPRIÉTÉ="valeur"`). Jamais construits à partir de données utilisateur.
    pub raw_args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    pub timeout: Option<Duration>,
    /// Plafond mémoire du processus (et de ses enfants sous Windows), en octets.
    pub memory_limit: Option<u64>,
    /// Capture de stdout (octets bruts) jusqu'à cette taille ; 0 = pas de capture.
    pub capture_stdout: usize,
    /// Taille de la fin de stderr conservée.
    pub stderr_tail: usize,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            raw_args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            timeout: None,
            memory_limit: None,
            capture_stdout: 0,
            stderr_tail: 16 * 1024,
        }
    }

    pub fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn raw_arg(mut self, a: impl Into<OsString>) -> Self {
        self.raw_args.push(a.into());
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, k: impl Into<OsString>, v: impl Into<OsString>) -> Self {
        self.env.push((k.into(), v.into()));
        self
    }

    pub fn timeout(mut self, t: Option<Duration>) -> Self {
        self.timeout = t;
        self
    }

    pub fn memory_limit(mut self, bytes: Option<u64>) -> Self {
        self.memory_limit = bytes;
        self
    }

    pub fn capture_stdout(mut self, max_bytes: usize) -> Self {
        self.capture_stdout = max_bytes;
        self
    }
}

#[derive(Debug)]
pub struct ProcessOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_tail: String,
    pub duration: Duration,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("{}", chud_core::trf!("le chemin du programme doit être absolu : {}", "the program path must be absolute: {}", .0.display()))]
    NotAbsolute(PathBuf),
    #[error("{}", chud_core::trf!("impossible de lancer {} : {source}", "cannot start {}: {source}", program.display()))]
    Spawn { program: PathBuf, source: std::io::Error },
    #[error("{}", chud_core::trf!("processus annulé", "process cancelled"))]
    Cancelled { stderr_tail: String },
    #[error("{}", chud_core::trf!("délai maximal dépassé ({:?})", "time limit exceeded ({:?})", .0))]
    Timeout(Duration, String),
    #[error("{}", chud_core::trf!("erreur d'entrée/sortie : {}", "input/output error: {}", .0))]
    Io(#[from] std::io::Error),
}

enum Msg {
    Out(Vec<u8>),
    Err(Vec<u8>),
    OutEof,
    ErrEof,
}

/// Découpe un flux d'octets en lignes (`\n`, `\r`, retour arrière `\x08`).
#[derive(Default)]
struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    fn push(&mut self, data: &[u8], emit: &mut dyn FnMut(&str)) {
        for &b in data {
            if b == b'\n' || b == b'\r' || b == 0x08 {
                self.flush(emit);
            } else {
                self.buf.push(b);
                if self.buf.len() > 64 * 1024 {
                    self.flush(emit);
                }
            }
        }
    }

    fn flush(&mut self, emit: &mut dyn FnMut(&str)) {
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&self.buf);
            let line = line.trim_end();
            if !line.trim().is_empty() {
                emit(line);
            }
            self.buf.clear();
        }
    }
}

/// Lance le processus et attend sa fin, en appelant `on_stdout`/`on_stderr` pour chaque ligne.
///
/// Un code de sortie non nul n'est pas une erreur ici : c'est à l'appelant d'interpréter
/// le résultat (certains outils renvoient 1 pour des avertissements).
pub fn run(
    spec: &ProcessSpec,
    should_cancel: &dyn Fn() -> bool,
    on_stdout: &mut dyn FnMut(&str),
    on_stderr: &mut dyn FnMut(&str),
) -> Result<ProcessOutput, ProcessError> {
    if !spec.program.is_absolute() {
        return Err(ProcessError::NotAbsolute(spec.program.clone()));
    }
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for a in &spec.raw_args {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(a);
        }
        #[cfg(not(windows))]
        cmd.arg(a);
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    appimage::host_environment(&mut cmd);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    platform::configure(&mut cmd, spec.memory_limit);

    let start = Instant::now();
    let mut child = cmd.spawn().map_err(|source| ProcessError::Spawn { program: spec.program.clone(), source })?;
    let guard = match platform::contain(&child, spec.memory_limit) {
        Ok(g) => g,
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::Spawn { program: spec.program.clone(), source });
        }
    };

    // Canal borné (au plus ~1 Mo en attente) : si l'interface traite moins vite que le programme
    // n'écrit, le programme attend au lieu de remplir la mémoire.
    let (tx, rx) = mpsc::sync_channel::<Msg>(64);
    let mut stdout = child.stdout.take().expect("stdout est capturé");
    let mut stderr = child.stderr.take().expect("stderr est capturé");
    let tx_out = tx.clone();
    let t_out = thread::spawn(move || pump(&mut stdout, &tx_out, Msg::Out, Msg::OutEof));
    let t_err = thread::spawn(move || pump(&mut stderr, &tx, Msg::Err, Msg::ErrEof));

    let mut out_split = LineSplitter::default();
    let mut err_split = LineSplitter::default();
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut tail: Vec<u8> = Vec::new();
    let mut eofs = 0;
    let mut exit_code: Option<Option<i32>> = None;
    let mut exited_at: Option<Instant> = None;
    let mut failure: Option<ProcessError> = None;

    let mut handle_msg = |m: Msg,
                          eofs: &mut i32,
                          captured: &mut Vec<u8>,
                          truncated: &mut bool,
                          tail: &mut Vec<u8>,
                          on_stdout: &mut dyn FnMut(&str),
                          on_stderr: &mut dyn FnMut(&str)| match m {
        Msg::Out(d) => {
            if spec.capture_stdout > 0 {
                let room = spec.capture_stdout.saturating_sub(captured.len());
                captured.extend_from_slice(&d[..d.len().min(room)]);
                *truncated |= d.len() > room;
            }
            out_split.push(&d, on_stdout);
        }
        Msg::Err(d) => {
            tail.extend_from_slice(&d);
            if tail.len() > spec.stderr_tail * 2 {
                let cut = tail.len() - spec.stderr_tail;
                tail.drain(..cut);
            }
            err_split.push(&d, on_stderr);
        }
        Msg::OutEof | Msg::ErrEof => *eofs += 1,
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(m) => handle_msg(m, &mut eofs, &mut captured, &mut truncated, &mut tail, on_stdout, on_stderr),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => eofs = 2,
        }
        if exit_code.is_none() {
            if let Some(status) = child.try_wait()? {
                exit_code = Some(status.code());
                exited_at = Some(Instant::now());
                // Le processus principal est terminé : on élimine d'éventuels descendants
                // qui garderaient les tubes ouverts.
                guard.kill_tree(&mut child);
            } else if should_cancel() {
                guard.kill_tree(&mut child);
                let _ = child.wait();
                failure = Some(ProcessError::Cancelled { stderr_tail: String::new() });
                exit_code = Some(None);
                exited_at = Some(Instant::now());
            } else if let Some(t) = spec.timeout
                && start.elapsed() > t
            {
                guard.kill_tree(&mut child);
                let _ = child.wait();
                failure = Some(ProcessError::Timeout(t, String::new()));
                exit_code = Some(None);
                exited_at = Some(Instant::now());
            }
        }
        if eofs >= 2 && exit_code.is_some() {
            break;
        }
        // Garde-fou : si les tubes restent ouverts longtemps après la fin, on n'attend plus.
        if exited_at.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
            break;
        }
    }
    while let Ok(m) = rx.try_recv() {
        handle_msg(m, &mut eofs, &mut captured, &mut truncated, &mut tail, on_stdout, on_stderr);
    }
    out_split.flush(on_stdout);
    err_split.flush(on_stderr);
    drop(guard);
    // Un descendant sorti du confinement peut garder les tubes ouverts : on n'attend que les
    // lecteurs déjà terminés. Les autres s'arrêteront seuls à la fermeture des tubes (le canal
    // fermé les empêche de bloquer en écriture), sans retenir la conversion.
    drop(rx);
    for t in [t_out, t_err] {
        if t.is_finished() {
            let _ = t.join();
        }
    }

    let stderr_tail = String::from_utf8_lossy(&tail[tail.len().saturating_sub(spec.stderr_tail)..]).into_owned();
    match failure {
        Some(ProcessError::Cancelled { .. }) => Err(ProcessError::Cancelled { stderr_tail }),
        Some(ProcessError::Timeout(t, _)) => Err(ProcessError::Timeout(t, stderr_tail)),
        Some(other) => Err(other),
        None => Ok(ProcessOutput {
            code: exit_code.flatten(),
            stdout: captured,
            stdout_truncated: truncated,
            stderr_tail,
            duration: start.elapsed(),
        }),
    }
}

/// Le processus `pid` existe-t-il encore ? Sert à ne pas toucher aux fichiers de travail d'une
/// autre instance de Chud en cours. Un numéro réutilisé répond « oui » : on garde alors un peu trop.
pub fn process_alive(pid: u32) -> bool {
    platform::alive(pid)
}

/// Variante simple : capture stdout (jusqu'à `max_stdout`) sans rappel ligne à ligne.
pub fn run_capture(spec: &ProcessSpec, max_stdout: usize) -> Result<ProcessOutput, ProcessError> {
    let spec = spec.clone().capture_stdout(max_stdout);
    run(&spec, &|| false, &mut |_| {}, &mut |_| {})
}

fn pump(r: &mut dyn Read, tx: &mpsc::SyncSender<Msg>, wrap: fn(Vec<u8>) -> Msg, eof: Msg) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(wrap(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = tx.send(eof);
}

#[cfg(windows)]
mod platform {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Le programme est placé dans son Job Object dès sa création, avant d'avoir pu charger ses
    /// bibliothèques ; ses descendants y naissent ensuite et ne peuvent pas en sortir (le job
    /// n'autorise pas `CREATE_BREAKAWAY_FROM_JOB`). Un démarrage suspendu fermerait la fenêtre de
    /// quelques microsecondes restante, mais coûtait 0,1 à 0,2 s par lancement (relevé des fils
    /// d'exécution) : écarté.
    pub fn configure(cmd: &mut Command, _memory_limit: Option<u64>) {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    /// Le processus existe-t-il encore ? (Un numéro réutilisé répond « oui » : sans danger.)
    pub fn alive(pid: u32) -> bool {
        // SAFETY : handle refermé ; `code` valide pendant l'appel.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                // Accès refusé : le processus existe mais appartient à un autre compte.
                return std::io::Error::last_os_error().raw_os_error() == Some(5);
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE as u32
        }
    }

    /// Job Object possédant l'arbre de processus ; sa fermeture tue tous les membres.
    pub struct Guard {
        job: HANDLE,
    }

    // SAFETY : un HANDLE de Job Object peut être utilisé depuis n'importe quel thread.
    unsafe impl Send for Guard {}

    pub fn contain(child: &Child, memory_limit: Option<u64>) -> std::io::Result<Guard> {
        Ok(make_job(child, memory_limit))
    }

    fn make_job(child: &Child, memory_limit: Option<u64>) -> Guard {
        // SAFETY : appels Win32 documentés ; les pointeurs passés sont valides pendant l'appel.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Guard { job };
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Some(limit) = memory_limit {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = limit as usize;
            }
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0;
            // Sans limites ou sans le processus, le job ne protège rien : on le referme et
            // l'arrêt se replie sur `Child::kill` (le programme principal seulement).
            if !configured || AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                CloseHandle(job);
                return Guard { job: std::ptr::null_mut() };
            }
            Guard { job }
        }
    }

    impl Guard {
        pub fn kill_tree(&self, child: &mut Child) {
            if !self.job.is_null() {
                // SAFETY : handle de job valide tant que `self` existe.
                unsafe {
                    TerminateJobObject(self.job, 1);
                }
            }
            let _ = child.kill();
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.job.is_null() {
                // SAFETY : fermeture unique du handle ; KILL_ON_JOB_CLOSE tue les survivants.
                unsafe {
                    CloseHandle(self.job);
                }
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub fn configure(cmd: &mut Command, memory_limit: Option<u64>) {
        cmd.process_group(0);
        if let Some(limit) = memory_limit {
            // SAFETY : setrlimit est async-signal-safe ; aucune allocation dans la closure.
            unsafe {
                cmd.pre_exec(move || {
                    let rl = libc::rlimit { rlim_cur: limit as libc::rlim_t, rlim_max: limit as libc::rlim_t };
                    if libc::setrlimit(libc::RLIMIT_AS, &rl) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }

    pub struct Guard {
        pgid: i32,
    }

    pub fn contain(child: &Child, _memory_limit: Option<u64>) -> std::io::Result<Guard> {
        Ok(Guard { pgid: child.id() as i32 })
    }

    pub fn alive(pid: u32) -> bool {
        // SAFETY : le signal 0 ne fait que tester l'existence. EPERM : il existe (autre compte).
        unsafe { libc::kill(pid as i32, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
    }

    /// Descendants de `root` d'après /proc (Linux), y compris ceux sortis du groupe de processus
    /// (`setsid`). Vide là où /proc n'existe pas.
    fn descendants(root: i32) -> Vec<i32> {
        let mut parent_of = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/proc") {
            for e in rd.flatten() {
                let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
                // « pid (nom) état ppid … » : le nom peut contenir des espaces et des parenthèses.
                let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
                let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else { continue };
                if let Some(ppid) = rest.split_whitespace().nth(1).and_then(|v| v.parse::<i32>().ok()) {
                    parent_of.push((pid, ppid));
                }
            }
        }
        let mut found = vec![root];
        let mut i = 0;
        while i < found.len() {
            let p = found[i];
            let children: Vec<i32> = parent_of.iter().filter(|(c, pp)| *pp == p && !found.contains(c)).map(|(c, _)| *c).collect();
            found.extend(children);
            i += 1;
        }
        found.remove(0);
        found
    }

    impl Guard {
        pub fn kill_tree(&self, child: &mut Child) {
            // Les descendants d'abord, tant que le lien de parenté existe encore.
            let tree = descendants(self.pgid);
            // SAFETY : signaux envoyés au groupe créé pour ce lancement et à ses descendants.
            unsafe {
                for pid in tree {
                    libc::kill(pid, libc::SIGKILL);
                }
                libc::kill(-self.pgid, libc::SIGKILL);
            }
            let _ = child.kill();
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY : idem ; sans effet si le groupe n'existe plus.
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[cfg(windows)]
    fn shell(script: &str) -> ProcessSpec {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        ProcessSpec::new(format!(r"{sys}\System32\cmd.exe")).args(["/d", "/c", script])
    }

    #[cfg(unix)]
    fn shell(script: &str) -> ProcessSpec {
        ProcessSpec::new("/bin/sh").args(["-c", script])
    }

    #[cfg(windows)]
    const LONG: &str = "ping -n 30 127.0.0.1 >nul";
    #[cfg(unix)]
    const LONG: &str = "sleep 30";

    #[cfg(windows)]
    const TWO_LINES_EXIT3: &str = "echo bonjour& echo monde& exit 3";
    #[cfg(unix)]
    const TWO_LINES_EXIT3: &str = "echo bonjour; echo monde; exit 3";

    #[test]
    fn captures_stdout_lines_and_exit_code() {
        let lines = RefCell::new(Vec::new());
        let out = run(&shell(TWO_LINES_EXIT3).capture_stdout(1024), &|| false, &mut |l| lines.borrow_mut().push(l.to_owned()), &mut |_| {})
            .unwrap();
        assert_eq!(out.code, Some(3));
        let lines = lines.into_inner();
        assert_eq!(lines, vec!["bonjour".to_string(), "monde".to_string()]);
        assert!(out.stdout_text().contains("bonjour"));
    }

    #[test]
    fn keeps_stderr_tail() {
        let out = run_capture(&shell("echo probleme 1>&2"), 0).unwrap();
        assert!(out.stderr_tail.contains("probleme"));
    }

    #[test]
    fn rejects_relative_program() {
        let e = run_capture(&ProcessSpec::new("ffmpeg"), 0).unwrap_err();
        assert!(matches!(e, ProcessError::NotAbsolute(_)));
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_keeping_the_pipes_open_does_not_block() {
        // `setsid` sort du groupe de processus : le descendant survit et garde stdout ouvert.
        let start = Instant::now();
        let out = run_capture(&shell("setsid sleep 30 & echo fini"), 1024).unwrap();
        assert!(out.stdout_text().contains("fini"));
        assert!(start.elapsed() < Duration::from_secs(15), "{:?}", start.elapsed());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancelling_also_kills_descendants_outside_the_group() {
        // Le descendant sort du groupe (`setsid`) puis écrit un fichier 2 s plus tard : il ne
        // doit jamais y arriver.
        let marker = std::env::temp_dir().join(format!("chud-escape-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("setsid sh -c 'sleep 2; touch {}' & sleep 30", marker.display());
        let start = Instant::now();
        let e = run(&shell(&script), &|| start.elapsed() > Duration::from_millis(500), &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, ProcessError::Cancelled { .. }));
        std::thread::sleep(Duration::from_secs(3));
        assert!(!marker.exists(), "le descendant a survécu à l'annulation");
    }

    #[cfg(windows)]
    #[test]
    fn descendants_started_at_once_are_contained() {
        // Le programme lance aussitôt un descendant détaché (`start /b`) : il est confiné aussi.
        let marker = std::env::temp_dir().join(format!("chud-escape-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("start /b cmd /d /c \"ping -n 3 127.0.0.1 >nul & echo x> {}\" & ping -n 30 127.0.0.1 >nul", marker.display());
        let start = Instant::now();
        let e = run(&shell(&script), &|| start.elapsed() > Duration::from_millis(500), &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, ProcessError::Cancelled { .. }));
        std::thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "le descendant a survécu à l'annulation");
    }

    #[test]
    fn the_current_process_is_alive() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(u32::MAX - 1));
    }

    #[test]
    fn a_fast_writer_is_fully_read_through_the_bounded_channel() {
        #[cfg(windows)]
        let script = "for /l %i in (1,1,20000) do @echo ligne %i";
        #[cfg(unix)]
        let script = "i=1; while [ $i -le 20000 ]; do echo ligne $i; i=$((i+1)); done";
        let mut n = 0;
        let out = run(&shell(script), &|| false, &mut |_| n += 1, &mut |_| {}).unwrap();
        assert_eq!(out.code, Some(0));
        assert_eq!(n, 20000);
    }

    #[test]
    fn timeout_kills_process_tree() {
        let start = Instant::now();
        let e = run(&shell(LONG).timeout(Some(Duration::from_millis(400))), &|| false, &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, ProcessError::Timeout(..)));
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn cancellation_is_prompt() {
        let start = Instant::now();
        let e = run(&shell(LONG), &|| start.elapsed() > Duration::from_millis(300), &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, ProcessError::Cancelled { .. }));
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn splitter_handles_cr_and_backspace() {
        let mut s = LineSplitter::default();
        let mut got = Vec::new();
        s.push(b"10%\r20%\x08\x08\x0830%\nfin", &mut |l| got.push(l.to_owned()));
        s.flush(&mut |l| got.push(l.to_owned()));
        assert_eq!(got, vec!["10%", "20%", "30%", "fin"]);
    }
}
