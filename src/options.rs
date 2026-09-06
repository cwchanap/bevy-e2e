use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Launch and timeout configuration for an out-of-process Bevy E2E session.
#[derive(Clone, Debug)]
pub struct E2eLaunchOptions {
    binary: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    startup_timeout: Duration,
    operation_timeout: Duration,
    shutdown_timeout: Duration,
    artifact_root: PathBuf,
    artifact_label: Option<String>,
}

impl E2eLaunchOptions {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            args: Vec::new(),
            env: Vec::new(),
            startup_timeout: Duration::from_secs(10),
            operation_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(3),
            artifact_root: PathBuf::from("test_output"),
            artifact_label: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    pub fn operation_timeout(mut self, timeout: Duration) -> Self {
        self.operation_timeout = timeout;
        self
    }

    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    pub fn artifact_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.artifact_root = root.into();
        self
    }

    pub fn artifact_label(mut self, label: impl Into<String>) -> Self {
        self.artifact_label = Some(label.into());
        self
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn env_vars(&self) -> &[(String, String)] {
        &self.env
    }

    pub fn startup_timeout_value(&self) -> Duration {
        self.startup_timeout
    }

    pub fn operation_timeout_value(&self) -> Duration {
        self.operation_timeout
    }

    pub fn shutdown_timeout_value(&self) -> Duration {
        self.shutdown_timeout
    }

    pub fn artifact_root_path(&self) -> &Path {
        &self.artifact_root
    }

    pub fn artifact_label_value(&self) -> Option<&str> {
        self.artifact_label.as_deref()
    }
}
