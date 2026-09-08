use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::{Error, Result, client::BrpClient, options::E2eLaunchOptions, process::ChildProcess};

const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const READINESS_METHOD: &str = "brp_extras/get_diagnostics";
const SHUTDOWN_METHOD: &str = "brp_extras/shutdown";

/// Out-of-process Bevy E2E session: one child, one main BRP port.
pub struct Game {
    child: ChildProcess,
    client: BrpClient,
    operation_timeout: Duration,
    shutdown_timeout: Duration,
    shut_down: bool,
    artifact_root: PathBuf,
    artifact_label: Option<String>,
    binary_stem: String,
    failure_dir: Option<PathBuf>,
}

impl Game {
    /// Spawn the game binary and wait until `brp_extras/get_diagnostics` answers.
    ///
    /// Does **not** silently relaunch on startup timeout. If the child exits
    /// while the readiness probe is in flight (e.g. a fixture run with
    /// `--exit-after-ready-ms` on a slow renderer such as Windows WARP), returns
    /// a [`Game`] holding the exited child so its stdout/stderr remain available
    /// for failure capture; detect this with [`Game::is_running`]. [`run`]
    /// handles this automatically and propagates [`Error::ChildExited`] after
    /// capturing artifacts.
    pub fn launch(options: E2eLaunchOptions) -> Result<Self> {
        let startup_timeout = options.startup_timeout_value();
        let shutdown_timeout = options.shutdown_timeout_value();
        // Keep readiness probes snappy; connection-refused fails fast anyway.
        let probe_timeout = options
            .operation_timeout_value()
            .min(Duration::from_millis(200));

        let binary_stem = options
            .binary()
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "game".to_owned());

        let mut child = ChildProcess::spawn(&options)?;
        let client = BrpClient::new(child.port(), probe_timeout);

        let deadline = Instant::now() + startup_timeout;
        loop {
            // If the child exited before readiness was observed, break out and
            // return a `Game` holding the dead child. Dropping it here (the old
            // `Err(ChildExited)` path) lost its stdout/stderr buffers, so `run`
            // could not capture failure artifacts -- which made the dead-child
            // fixture flaky on slow renderers where the child's
            // `--exit-after-ready-ms` fired before the readiness probe answered.
            if child.try_wait()?.is_some() {
                break;
            }

            match client.request(READINESS_METHOD, json!({})) {
                Ok(_result) => {
                    // Null frame_count is acceptable; any successful JSON result means ready.
                    let client = BrpClient::new(child.port(), options.operation_timeout_value());
                    return Ok(Self {
                        child,
                        client,
                        operation_timeout: options.operation_timeout_value(),
                        shutdown_timeout,
                        shut_down: false,
                        artifact_root: options.artifact_root_path().to_path_buf(),
                        artifact_label: options.artifact_label_value().map(str::to_owned),
                        binary_stem,
                        failure_dir: None,
                    });
                }
                Err(_) => {
                    if Instant::now() >= deadline {
                        let stdout = child.stdout_snapshot();
                        let stderr = child.stderr_snapshot();
                        let _ = child.kill();
                        let _ = child.reap();
                        return Err(Error::Timeout {
                            operation: format!(
                                "startup (stdout_len={}, stderr_len={})\n--- stdout ---\n{}\n--- stderr ---\n{}",
                                stdout.len(),
                                stderr.len(),
                                truncate_output(&stdout),
                                truncate_output(&stderr),
                            ),
                            timeout: startup_timeout,
                        });
                    }
                    thread::sleep(READINESS_POLL_INTERVAL);
                }
            }
        }

        // Child exited while the readiness probe was in flight. Return a `Game`
        // holding the exited child (with its drained stdout/stderr) so callers
        // can still capture failure artifacts. `run` detects this via
        // `is_running()` and propagates `Error::ChildExited` after capturing.
        let client = BrpClient::new(child.port(), options.operation_timeout_value());
        Ok(Self {
            child,
            client,
            operation_timeout: options.operation_timeout_value(),
            shutdown_timeout,
            shut_down: false,
            artifact_root: options.artifact_root_path().to_path_buf(),
            artifact_label: options.artifact_label_value().map(str::to_owned),
            binary_stem,
            failure_dir: None,
        })
    }

    pub fn is_running(&mut self) -> bool {
        !self.shut_down && self.child.is_running()
    }

    /// Raw one-response BRP call on the child's main loopback port.
    pub fn brp(&self, method: &str, params: Value) -> Result<Value> {
        self.client.request(method, params)
    }

    pub(crate) fn operation_timeout(&self) -> Duration {
        self.operation_timeout
    }

    pub(crate) fn artifact_root_path(&self) -> &Path {
        &self.artifact_root
    }

    pub(crate) fn artifact_label_value(&self) -> Option<&str> {
        self.artifact_label.as_deref()
    }

    pub(crate) fn binary_stem(&self) -> &str {
        &self.binary_stem
    }

    pub(crate) fn failure_dir(&self) -> Option<&Path> {
        self.failure_dir.as_deref()
    }

    pub(crate) fn set_failure_dir(&mut self, dir: PathBuf) {
        self.failure_dir = Some(dir);
    }

    pub(crate) fn child_write_output_logs(&self, dir: &Path) -> Result<()> {
        self.child.write_output_logs(dir)
    }

    pub(crate) fn child_pid(&self) -> u32 {
        self.child.pid()
    }

    /// Cached exit status of the child, if it has already exited.
    /// `is_running()` returning `false` implies `Some(status)` here.
    pub(crate) fn child_exit_status(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// Graceful shutdown: BRP shutdown → wait → kill → reap. Idempotent.
    pub fn shutdown(&mut self) -> Result<()> {
        if self.shut_down {
            return Ok(());
        }

        if self.child.try_wait()?.is_none() {
            let _ = self.client.request(SHUTDOWN_METHOD, json!({}));
            if self.child.wait_for_exit(self.shutdown_timeout)?.is_none() {
                self.child.kill()?;
            }
        }

        let _ = self.child.reap()?;
        self.shut_down = true;
        Ok(())
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        if self.shut_down {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.reap();
        self.shut_down = true;
    }
}

/// Launch one child, run `test`, capture failure best-effort, then shutdown/reap.
///
/// Panics are preserved after cleanup via `resume_unwind`.
pub fn run<F>(options: E2eLaunchOptions, test: F) -> Result<()>
where
    F: FnOnce(&mut Game) -> Result<()>,
{
    let mut game = Game::launch(options)?;

    // `Game::launch` returns a `Game` even if the child exited while the
    // readiness probe was in flight (notably the dead-child fixture with
    // `--exit-after-ready-ms` on slow Windows WARP runners, where the child can
    // exit before the harness observes BRP readiness). Detect that here and
    // capture failure artifacts before propagating `ChildExited`, so a child
    // that dies during startup still produces `failure.json` + output logs
    // instead of bypassing capture (which `?` on `Game::launch` used to do).
    if !game.is_running() {
        let status = game.child_exit_status()?.ok_or_else(|| {
            Error::Configuration("child reported not running but no exit status".into())
        })?;
        let error = Error::ChildExited(status);
        game.capture_failure_best_effort(&error.to_string());
        let _ = game.shutdown();
        game.refresh_failure_output_logs();
        return Err(error);
    }

    let outcome = catch_unwind(AssertUnwindSafe(|| test(&mut game)));

    match outcome {
        Ok(Ok(())) => {
            game.shutdown()?;
            Ok(())
        }
        Ok(Err(error)) => {
            game.capture_failure_best_effort(&error.to_string());
            let _ = game.shutdown();
            game.refresh_failure_output_logs();
            Err(error)
        }
        Err(payload) => {
            let message = panic_message(&payload);
            game.capture_failure_best_effort(&message);
            let _ = game.shutdown();
            game.refresh_failure_output_logs();
            resume_unwind(payload);
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "box test panicked".to_owned()
    }
}

fn truncate_output(text: &str) -> String {
    const MAX: usize = 4096;
    if text.len() <= MAX {
        return text.to_owned();
    }
    let mut end = MAX;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... [truncated]", &text[..end])
}
