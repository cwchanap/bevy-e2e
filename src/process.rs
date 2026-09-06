use std::{
    io::Read,
    net::TcpListener,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::{Error, Result, options::E2eLaunchOptions};

/// OS child process wrapper: port probe, spawn, stdout/stderr drain, wait/kill/reap.
pub struct ChildProcess {
    child: Option<Child>,
    pid: u32,
    port: u16,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    exit_status: Option<ExitStatus>,
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

        let stdout_buf = Arc::new(Mutex::new(Vec::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));

        if let Some(stdout) = child.stdout.take() {
            spawn_drain(stdout, Arc::clone(&stdout_buf));
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_drain(stderr, Arc::clone(&stderr_buf));
        }

        Ok(Self {
            child: Some(child),
            pid,
            port,
            stdout: stdout_buf,
            stderr: stderr_buf,
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
    pub fn reap(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            self.child = None;
            return Ok(Some(status));
        }
        let Some(mut child) = self.child.take() else {
            return Ok(None);
        };
        let status = child.wait().map_err(Error::Spawn)?;
        self.exit_status = Some(status);
        Ok(Some(status))
    }

    pub fn stdout_snapshot(&self) -> String {
        snapshot(&self.stdout)
    }

    pub fn stderr_snapshot(&self) -> String {
        snapshot(&self.stderr)
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
        if self.child.is_none() {
            return;
        }
        let _ = self.kill();
        let _ = self.reap();
    }
}

fn select_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(Error::Spawn)?;
    let port = listener.local_addr().map_err(Error::Spawn)?.port();
    drop(listener);
    Ok(port)
}

fn spawn_drain(mut reader: impl Read + Send + 'static, buffer: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = buffer.lock() {
                        guard.extend_from_slice(&chunk[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn snapshot(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    let guard = buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    String::from_utf8_lossy(&guard).into_owned()
}
