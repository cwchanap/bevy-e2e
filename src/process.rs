use std::{
    io::Read,
    net::TcpListener,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{Error, Result, options::E2eLaunchOptions};

/// Tail cap for each captured child output stream. A fixture that logs
/// continuously during a wait cannot exhaust the test runner's memory; only the
/// most recent tail is retained and a truncation marker is recorded.
const MAX_CAPTURED_OUTPUT_BYTES: usize = 1024 * 1024;

/// Default drain-join grace used when no shutdown timeout is in scope (`Drop`).
/// The normal case (pipe EOFs when the child exits) completes in milliseconds;
/// this only bounds the pathological case where a helper process inherited the
/// pipe write-end and keeps it open after the game exited.
const DEFAULT_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// OS child process wrapper: port probe, spawn, stdout/stderr drain, wait/kill/reap.
pub struct ChildProcess {
    child: Option<Child>,
    pid: u32,
    port: u16,
    stdout: Arc<Mutex<CapturedOutput>>,
    stderr: Arc<Mutex<CapturedOutput>>,
    stdout_drain: Option<DrainHandle>,
    stderr_drain: Option<DrainHandle>,
    exit_status: Option<ExitStatus>,
}

/// A drain thread plus a completion flag so the join can be bounded.
struct DrainHandle {
    handle: JoinHandle<()>,
    done: Arc<AtomicBool>,
}

/// Bounded, tail-truncating capture of one child output stream.
struct CapturedOutput {
    data: Vec<u8>,
    truncated: bool,
}

impl CapturedOutput {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            truncated: false,
        }
    }

    /// Append bytes, retaining only the most recent `MAX_CAPTURED_OUTPUT_BYTES`
    /// tail and marking truncation when older bytes are discarded.
    fn push(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        if self.data.len() > MAX_CAPTURED_OUTPUT_BYTES {
            self.truncated = true;
            let excess = self.data.len() - MAX_CAPTURED_OUTPUT_BYTES;
            self.data.drain(..excess);
        }
    }

    fn snapshot(&self) -> String {
        let body = String::from_utf8_lossy(&self.data).into_owned();
        if self.truncated {
            format!(
                "... [output truncated, showing last {} bytes]\n{}",
                MAX_CAPTURED_OUTPUT_BYTES, body
            )
        } else {
            body
        }
    }
}

impl ChildProcess {
    /// Select a free loopback port, spawn the binary with framework env, and drain pipes.
    pub fn spawn(options: &E2eLaunchOptions) -> Result<Self> {
        let port = select_port()?;

        let mut command = Command::new(options.binary());
        command
            .args(options.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in options.env_vars() {
            command.env(key, value);
        }

        // Framework-owned: always win over caller env.
        command
            .env("BEVY_E2E", "1")
            .env("BRP_EXTRAS_PORT", port.to_string());

        let mut child = command.spawn().map_err(Error::Spawn)?;
        let pid = child.id();

        let stdout_buf = Arc::new(Mutex::new(CapturedOutput::new()));
        let stderr_buf = Arc::new(Mutex::new(CapturedOutput::new()));

        let stdout_drain = child
            .stdout
            .take()
            .map(|stdout| spawn_drain(stdout, Arc::clone(&stdout_buf)));
        let stderr_drain = child
            .stderr
            .take()
            .map(|stderr| spawn_drain(stderr, Arc::clone(&stderr_buf)));

        Ok(Self {
            child: Some(child),
            pid,
            port,
            stdout: stdout_buf,
            stderr: stderr_buf,
            stdout_drain,
            stderr_drain,
            exit_status: None,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// OS process id captured at spawn (stable after reap).
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.try_wait(), Ok(None))
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.exit_status = Some(status);
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(Error::Spawn(error)),
        }
    }

    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn kill(&mut self) -> Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        match child.kill() {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(Error::Spawn(error)),
        }
    }

    /// Block until the child exits and clear the live handle (idempotent).
    ///
    /// `drain_timeout` bounds how long `reap` waits for the stdout/stderr drain
    /// threads to finish after the child exits. The pipes normally hit EOF as
    /// soon as the child exits, so the join completes in milliseconds. A game
    /// that spawns a helper process inheriting the stdout/stderr pipe write-ends
    /// can keep the drain thread blocked in `read()` after the game itself
    /// exits; the bounded join detaches such a drain thread instead of hanging
    /// `reap` (and the whole test/CI process) past the grace period.
    pub fn reap(&mut self, drain_timeout: Duration) -> Result<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            self.child = None;
            self.join_drains_bounded(drain_timeout);
            return Ok(Some(status));
        }
        let Some(mut child) = self.child.take() else {
            self.join_drains_bounded(drain_timeout);
            return Ok(None);
        };
        let status = child.wait().map_err(Error::Spawn)?;
        self.exit_status = Some(status);
        // `child.wait()` only synchronizes with the child process; the drain
        // threads may still be copying buffered pipe data into the mutexes. Join
        // them (bounded) before returning so callers reading the buffers (e.g.
        // the final `refresh_failure_output_logs` after shutdown) see the
        // complete tail. The pipes hit EOF once the child exits, so the joins
        // complete promptly unless a helper inherited the pipe write-ends.
        self.join_drains_bounded(drain_timeout);
        Ok(Some(status))
    }

    /// Join the stdout/stderr drain threads if still live, waiting at most
    /// `timeout` (shared across both drains) for each to signal completion.
    /// Drain threads that have not finished by the deadline are detached
    /// (their handles are dropped) so `reap` cannot hang.
    fn join_drains_bounded(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        for drain in [self.stdout_drain.take(), self.stderr_drain.take()]
            .into_iter()
            .flatten()
        {
            while !drain.done.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            if drain.done.load(Ordering::Acquire) {
                let _ = drain.handle.join();
            }
            // else: drain thread still blocked (e.g. a helper process inherited
            // the pipe write-end and keeps it open after the game exited).
            // Detach by dropping the handle; the drain thread exits when the
            // pipe finally EOFs or the test process exits, and the bytes
            // captured so far remain in the buffer.
        }
    }

    pub fn stdout_snapshot(&self) -> String {
        self.stdout
            .lock()
            .map(|guard| guard.snapshot())
            .unwrap_or_else(|poisoned| poisoned.into_inner().snapshot())
    }

    pub fn stderr_snapshot(&self) -> String {
        self.stderr
            .lock()
            .map(|guard| guard.snapshot())
            .unwrap_or_else(|poisoned| poisoned.into_inner().snapshot())
    }

    /// Persist drained stdout/stderr into `dir` using lossy UTF-8 decoding.
    pub fn write_output_logs(&self, dir: &std::path::Path) -> Result<()> {
        std::fs::write(dir.join("stdout.log"), self.stdout_snapshot())
            .map_err(|error| Error::artifact_io("write stdout.log", error))?;
        std::fs::write(dir.join("stderr.log"), self.stderr_snapshot())
            .map_err(|error| Error::artifact_io("write stderr.log", error))?;
        Ok(())
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        // Same spirit as Game::Drop: kill/reap unreaped children so panic/early
        // return cannot hang on std::process::Child (e.g. --sleep-forever tests).
        if self.child.is_none() && self.stdout_drain.is_none() && self.stderr_drain.is_none() {
            return;
        }
        let _ = self.kill();
        let _ = self.reap(DEFAULT_DRAIN_GRACE);
    }
}

fn select_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(Error::Spawn)?;
    let port = listener.local_addr().map_err(Error::Spawn)?.port();
    drop(listener);
    Ok(port)
}

fn spawn_drain(
    mut reader: impl Read + Send + 'static,
    buffer: Arc<Mutex<CapturedOutput>>,
) -> DrainHandle {
    let done = Arc::new(AtomicBool::new(false));
    let done_flag = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = buffer.lock() {
                        guard.push(&chunk[..n]);
                    }
                }
                Err(_) => break,
            }
        }
        done_flag.store(true, Ordering::Release);
    });
    DrainHandle { handle, done }
}
