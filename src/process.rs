use std::{
    collections::VecDeque,
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
///
/// Backed by a `VecDeque` so trimming the oldest bytes (`drain` from the front)
/// is O(excess) instead of shifting the whole retained MiB on every read — a
/// `Vec` front-`drain` would copy ~1 MiB per 4 KiB chunk once at the cap.
struct CapturedOutput {
    data: VecDeque<u8>,
    truncated: bool,
}

impl CapturedOutput {
    fn new() -> Self {
        Self {
            data: VecDeque::with_capacity(MAX_CAPTURED_OUTPUT_BYTES),
            truncated: false,
        }
    }

    /// Append bytes, retaining only the most recent `MAX_CAPTURED_OUTPUT_BYTES`
    /// tail and marking truncation when older bytes are discarded. Front
    /// `drain` on a `VecDeque` is O(excess), not O(len), so a chatty fixture
    /// cannot amplify memory copies or back-pressure the drain thread.
    fn push(&mut self, bytes: &[u8]) {
        self.data.extend(bytes);
        if self.data.len() > MAX_CAPTURED_OUTPUT_BYTES {
            self.truncated = true;
            let excess = self.data.len() - MAX_CAPTURED_OUTPUT_BYTES;
            self.data.drain(..excess);
        }
    }

    fn snapshot(&self) -> String {
        // `VecDeque` may be split across two contiguous slices; collect into
        // one before lossy UTF-8 decoding so multi-byte sequences straddling
        // the ring boundary decode correctly.
        let body: Vec<u8> = self.data.iter().copied().collect();
        let body = String::from_utf8_lossy(&body).into_owned();
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

#[cfg(test)]
mod tests {
    use super::{CapturedOutput, MAX_CAPTURED_OUTPUT_BYTES};

    #[test]
    fn push_retains_only_the_tail_when_over_cap() {
        let mut out = CapturedOutput::new();
        // Fill exactly to the cap: no truncation yet.
        out.push(&vec![b'a'; MAX_CAPTURED_OUTPUT_BYTES]);
        assert!(!out.truncated, "at-cap buffer is not truncated");
        assert_eq!(out.data.len(), MAX_CAPTURED_OUTPUT_BYTES);

        // Push a full cap of `b`; the entire `a` tail is displaced, leaving
        // only the newest `cap` bytes (all `b`).
        out.push(&vec![b'b'; MAX_CAPTURED_OUTPUT_BYTES]);
        assert!(out.truncated, "overflowing the cap marks truncation");
        assert_eq!(
            out.data.len(),
            MAX_CAPTURED_OUTPUT_BYTES,
            "stays at the cap"
        );
        assert!(
            out.data.iter().all(|&b| b == b'b'),
            "retains the newest tail"
        );
    }

    #[test]
    fn push_keeps_partial_old_tail_on_small_overflow() {
        let mut out = CapturedOutput::new();
        out.push(&vec![b'a'; MAX_CAPTURED_OUTPUT_BYTES]);
        // A small overflow evicts only the oldest bytes; the retained tail is
        // (cap - 4096) `a`s followed by 4096 `b`s — the last `cap` bytes.
        out.push(&vec![b'b'; 4096]);
        assert!(out.truncated);
        assert_eq!(out.data.len(), MAX_CAPTURED_OUTPUT_BYTES);
        let actual: Vec<u8> = out.data.iter().copied().collect();
        assert!(
            actual[..MAX_CAPTURED_OUTPUT_BYTES - 4096]
                .iter()
                .all(|&b| b == b'a')
        );
        assert!(
            actual[MAX_CAPTURED_OUTPUT_BYTES - 4096..]
                .iter()
                .all(|&b| b == b'b')
        );
    }

    #[test]
    fn push_drops_oldest_in_one_shot_for_a_large_chunk() {
        let mut out = CapturedOutput::new();
        // A single push larger than the cap keeps only the last `cap` bytes.
        let big: Vec<u8> = (0..(MAX_CAPTURED_OUTPUT_BYTES * 2) as u32)
            .map(|i| (i % 251) as u8)
            .collect();
        out.push(&big);
        assert!(out.truncated);
        assert_eq!(out.data.len(), MAX_CAPTURED_OUTPUT_BYTES);
        // Last `cap` bytes of `big` == big[cap..].
        let expected: Vec<u8> = big[MAX_CAPTURED_OUTPUT_BYTES..].to_vec();
        let actual: Vec<u8> = out.data.iter().copied().collect();
        assert_eq!(actual, expected, "retains the exact newest tail");
    }

    #[test]
    fn snapshot_marks_truncation_and_keeps_tail_body() {
        let mut out = CapturedOutput::new();
        out.push(b"header\n");
        out.push(&vec![b'x'; MAX_CAPTURED_OUTPUT_BYTES + 10]);
        let snap = out.snapshot();
        assert!(
            snap.starts_with("... [output truncated, showing last "),
            "truncation marker is prepended"
        );
        // The original "header\n" was evicted; only the newest tail remains.
        assert!(!snap.contains("header"));
        assert!(snap.contains('x'));
    }

    #[test]
    fn snapshot_is_unmarked_when_not_truncated() {
        let mut out = CapturedOutput::new();
        out.push(b"hello\n");
        let snap = out.snapshot();
        assert_eq!(snap, "hello\n");
        assert!(!snap.contains("truncated"));
    }

    #[test]
    fn snapshot_decodes_multibyte_utf8_across_ring_boundary() {
        // Force the ring to wrap by pushing, overflowing, then checking that a
        // multi-byte UTF-8 sequence near the tail decodes lossily without panic.
        let mut out = CapturedOutput::new();
        out.push(&vec![b'a'; MAX_CAPTURED_OUTPUT_BYTES]);
        // '€' is 3 bytes in UTF-8; place it at the very end so it straddles or
        // follows the wrap point. snapshot must not panic and must contain it.
        out.push("€tail".as_bytes());
        let snap = out.snapshot();
        assert!(snap.contains("€tail"));
    }
}
