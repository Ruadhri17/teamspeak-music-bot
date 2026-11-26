//! Stopping child processes: SIGTERM first, SIGKILL only if the process is still running
//! after a grace period.
//!
//! `yt-dlp` built with PyInstaller unpacks itself into `/tmp/_MEI*` and removes the directory
//! only when it exits on its own or on SIGTERM. SIGKILL leaves about 85 MB behind every time.
//!
//! - `stop`: stop one `std` child and reap it
//! - `Stopper`: stop the processes of one playback from another thread
//! - `TermOnDrop`: a tokio child that is stopped when it is dropped

use std::{
    io,
    os::fd::OwnedFd,
    process::Output,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

use rustix::process::{Pid, PidfdFlags, Signal, kill_process, pidfd_open, pidfd_send_signal};
use tokio::{io::AsyncReadExt, process::Child};
use tracing::warn;

/// How long a process gets to exit after SIGTERM.
pub const STOP_GRACE: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(2)
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Stops a process and reaps it, blocking the thread for at most `STOP_GRACE`.
/// Does nothing if the process has already exited.
pub fn stop(child: &mut std::process::Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    // Not reaped yet, so the PID still belongs to this process.
    let _ = kill_process(Pid::from_child(child), Signal::TERM);
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        thread::sleep(POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Stops one playback from any thread: sets a flag its threads check, and signals its
/// processes at once. A thread blocked reading from a stalled process is freed too, which
/// a flag alone cannot do. Clones share the same state.
#[derive(Clone, Debug, Default)]
pub struct Stopper(Arc<Mutex<Stopping>>);

#[derive(Debug, Default)]
struct Stopping {
    stopped: bool,
    /// Each process with the signal it gets. A pidfd keeps pointing at the same process,
    /// so a signal sent after the process was reaped cannot hit another one with the
    /// reused PID: it fails instead.
    processes: Vec<(OwnedFd, Signal)>,
}

impl Stopper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the flag and signals every watched process. Processes that already exited
    /// are skipped. Only short system calls under the lock, safe to call from async code.
    pub fn stop(&self) {
        let mut stopping = self.lock();
        stopping.stopped = true;
        for (pidfd, signal) in &stopping.processes {
            let _ = pidfd_send_signal(pidfd, *signal);
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.lock().stopped
    }

    /// Signals `child` with `signal` when the playback stops, at once if it already has.
    /// Call right after spawning, while the child cannot have been reaped yet.
    pub fn watch(&self, child: &std::process::Child, signal: Signal) {
        // Fails only on kernels older than 5.3, the process then dies with the end of the
        // track instead of at once.
        let pidfd = match pidfd_open(Pid::from_child(child), PidfdFlags::empty()) {
            Ok(pidfd) => pidfd,
            Err(err) => {
                warn!("cannot watch process {}: {err}", child.id());
                return;
            }
        };
        let mut stopping = self.lock();
        if stopping.stopped {
            let _ = pidfd_send_signal(&pidfd, signal);
        } else {
            stopping.processes.push((pidfd, signal));
        }
    }

    /// A panic while holding the lock cannot leave the state half changed, so a poisoned
    /// lock is used as it is.
    fn lock(&self) -> MutexGuard<'_, Stopping> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A tokio child process that is stopped with `stop` semantics when dropped, for example
/// when the future that owns it times out or its task is aborted.
pub struct TermOnDrop(Option<Child>);

impl TermOnDrop {
    /// `child` must have piped stdout and stderr and must not use `kill_on_drop`.
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }

    /// Waits for the process and collects its output, like `Command::output`.
    pub async fn output(&mut self) -> io::Result<Output> {
        let child = self.0.as_mut().expect("the child is only taken in drop");
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (status, _, _) = tokio::try_join!(
            child.wait(),
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        )?;
        Ok(Output {
            status,
            stdout: out,
            stderr: err,
        })
    }
}

impl Drop for TermOnDrop {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        if let Some(pid) = child.id().and_then(|id| Pid::from_raw(id as i32)) {
            let _ = kill_process(pid, Signal::TERM);
        }
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if tokio::time::timeout(STOP_GRACE, child.wait())
                        .await
                        .is_err()
                    {
                        let _ = child.kill().await;
                    }
                });
            }
            // Outside a runtime nothing can wait, tokio reaps the orphan when it can.
            Err(_) => {
                let _ = child.start_kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, process::ExitStatusExt},
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };

    use tempfile::TempDir;

    use super::*;

    const TRAPS_TERM: &str = r#"trap 'echo term > "$1"; exit 0' TERM
echo ready > "$2"
while :; do sleep 0.05; done"#;

    const IGNORES_TERM: &str = r#"trap '' TERM
echo ready > "$2"
while :; do sleep 0.05; done"#;

    struct Script {
        dir: TempDir,
        path: PathBuf,
    }

    impl Script {
        fn new(body: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("script");
            let source = dir.path().join("script.src");
            fs::write(&source, format!("#!/bin/sh\n{body}\n")).unwrap();
            let copied = Command::new("cp").arg(&source).arg(&path).status().unwrap();
            assert!(copied.success());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            Self { dir, path }
        }

        fn command(&self) -> Command {
            let mut cmd = Command::new(&self.path);
            cmd.arg(self.term_file())
                .arg(self.ready_file())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            cmd
        }

        fn term_file(&self) -> PathBuf {
            self.dir.path().join("term")
        }

        fn ready_file(&self) -> PathBuf {
            self.dir.path().join("ready")
        }

        fn wait_until_ready(&self) {
            wait_for(|| self.ready_file().exists());
        }
    }

    fn wait_for(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(start.elapsed() < Duration::from_secs(5), "timed out");
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn is_running(pid: u32) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rsplit(") ")
                .next()
                .is_some_and(|rest| !rest.starts_with('Z')),
            Err(_) => false,
        }
    }

    fn exists(path: &Path) -> bool {
        path.exists()
    }

    #[test]
    fn stop_sends_term_first() {
        let script = Script::new(TRAPS_TERM);
        let mut child = script.command().spawn().unwrap();
        script.wait_until_ready();

        let start = Instant::now();
        stop(&mut child);
        assert!(start.elapsed() < STOP_GRACE);
        assert!(exists(&script.term_file()));
        assert_eq!(child.try_wait().unwrap().unwrap().code(), Some(0));
    }

    #[test]
    fn stop_kills_a_process_that_ignores_term() {
        let script = Script::new(IGNORES_TERM);
        let mut child = script.command().spawn().unwrap();
        script.wait_until_ready();

        let start = Instant::now();
        stop(&mut child);
        assert!(start.elapsed() >= STOP_GRACE);
        assert_eq!(child.try_wait().unwrap().unwrap().signal(), Some(9));
    }

    #[test]
    fn stop_leaves_an_exited_process_alone() {
        let script = Script::new("exit 3");
        let mut child = script.command().spawn().unwrap();
        child.wait().unwrap();
        stop(&mut child);
        assert_eq!(child.try_wait().unwrap().unwrap().code(), Some(3));
    }

    #[tokio::test]
    async fn output_collects_status_and_both_streams() {
        let script = Script::new("echo out; echo err >&2; exit 4");
        let mut child = TermOnDrop::new(
            tokio::process::Command::from(script.command())
                .spawn()
                .unwrap(),
        );
        let output = child.output().await.unwrap();
        assert_eq!(output.status.code(), Some(4));
        assert_eq!(output.stdout, b"out\n");
        assert_eq!(output.stderr, b"err\n");
    }

    #[tokio::test]
    async fn dropping_sends_term() {
        let script = Script::new(TRAPS_TERM);
        let child = tokio::process::Command::from(script.command())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        script.wait_until_ready();

        drop(TermOnDrop::new(child));
        let term_file = script.term_file();
        tokio::task::spawn_blocking(move || wait_for(|| exists(&term_file) && !is_running(pid)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn dropping_kills_a_process_that_ignores_term() {
        let script = Script::new(IGNORES_TERM);
        let child = tokio::process::Command::from(script.command())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        script.wait_until_ready();

        drop(TermOnDrop::new(child));
        tokio::time::sleep(STOP_GRACE / 2).await;
        assert!(is_running(pid));
        tokio::task::spawn_blocking(move || wait_for(|| !is_running(pid)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn timeout_stops_the_process() {
        let script = Script::new(TRAPS_TERM);
        let mut child = TermOnDrop::new(
            tokio::process::Command::from(script.command())
                .spawn()
                .unwrap(),
        );
        let ready_file = script.ready_file();
        tokio::task::spawn_blocking(move || wait_for(|| exists(&ready_file)))
            .await
            .unwrap();

        let result = tokio::time::timeout(Duration::from_millis(50), child.output()).await;
        assert!(result.is_err());
        drop(child);
        let term_file = script.term_file();
        tokio::task::spawn_blocking(move || wait_for(|| exists(&term_file)))
            .await
            .unwrap();
    }

    fn sleeper() -> std::process::Child {
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn ended_by(child: &mut std::process::Child, signal: Signal) -> bool {
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status.signal() == Some(signal.as_raw());
            }
            if start.elapsed() > Duration::from_secs(5) {
                return false;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    #[test]
    fn stopper_signals_watched_processes() {
        let stopper = Stopper::new();
        let mut killed = sleeper();
        let mut terminated = sleeper();
        stopper.watch(&killed, Signal::KILL);
        stopper.watch(&terminated, Signal::TERM);
        assert!(!stopper.is_stopped());
        assert!(is_running(killed.id()));

        stopper.clone().stop();
        assert!(stopper.is_stopped());
        assert!(ended_by(&mut killed, Signal::KILL));
        assert!(ended_by(&mut terminated, Signal::TERM));
    }

    #[test]
    fn a_process_watched_after_the_stop_is_signalled_at_once() {
        let stopper = Stopper::new();
        stopper.stop();
        let mut child = sleeper();
        stopper.watch(&child, Signal::KILL);
        assert!(ended_by(&mut child, Signal::KILL));
    }

    #[test]
    fn stopping_after_the_process_was_reaped_is_harmless() {
        let stopper = Stopper::new();
        let mut child = Command::new("true").spawn().unwrap();
        stopper.watch(&child, Signal::KILL);
        assert!(child.wait().unwrap().success());
        stopper.stop();
        stopper.stop();
        assert!(stopper.is_stopped());
    }
}
