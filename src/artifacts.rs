use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use bevy::{
    reflect::TypePath,
    remote::builtin_methods::{
        BRP_QUERY_METHOD, BrpQuery, BrpQueryFilter, BrpQueryParams, ComponentSelector,
    },
};
use serde_json::{Value, json};

use crate::{E2eId, Error, Result, game::Game};

const SCREENSHOT_METHOD: &str = "brp_extras/screenshot";
const DIAGNOSTICS_METHOD: &str = "brp_extras/get_diagnostics";
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Game {
    /// Capture a PNG via terminal `brp_extras/screenshot` into a labeled session directory.
    pub fn screenshot(&self, label: &str) -> Result<PathBuf> {
        let dir = self.create_session_dir(Some(label))?;
        self.write_screenshot(&dir)
    }

    /// Write an explicit non-failure artifact bundle (no `failure.json`).
    pub fn capture_artifacts(&self, label: &str) -> Result<PathBuf> {
        let dir = self.create_session_dir(Some(label))?;
        self.write_screenshot(&dir)?;
        self.write_world_snapshot(&dir)?;
        self.write_output_logs(&dir)?;
        Ok(dir)
    }

    pub(crate) fn capture_failure_best_effort(&mut self, error_text: &str) {
        let Ok(dir) = self.create_session_dir(None) else {
            return;
        };
        self.set_failure_dir(dir.clone());

        let child_pid = self.child_pid();
        let mut remote_errors = Vec::new();

        let failure = json!({
            "error": error_text,
            "child_pid": child_pid,
            "remote_errors": remote_errors,
        });
        if let Err(error) = write_json(&dir.join("failure.json"), &failure) {
            let _ = error;
        }

        if let Err(error) = self.write_output_logs(&dir) {
            remote_errors.push(format!("stdout/stderr: {error}"));
        }

        if let Err(error) = self.write_screenshot(&dir) {
            remote_errors.push(format!("screenshot: {error}"));
        }

        if let Err(error) = self.write_world_snapshot(&dir) {
            remote_errors.push(format!("world: {error}"));
        }

        let failure = json!({
            "error": error_text,
            "child_pid": child_pid,
            "remote_errors": remote_errors,
        });
        let _ = write_json(&dir.join("failure.json"), &failure);
    }

    pub(crate) fn refresh_failure_output_logs(&self) {
        let Some(dir) = self.failure_dir() else {
            return;
        };
        let _ = self.write_output_logs(dir);
    }

    fn create_session_dir(&self, label: Option<&str>) -> Result<PathBuf> {
        let name = session_name(label, self.artifact_label_value(), self.binary_stem());
        let dir = self.artifact_root_path().join(name);
        fs::create_dir_all(&dir)
            .map_err(|error| Error::artifact_io("create artifact directory", error))?;
        // `create_dir_all` reuses an existing directory verbatim, so a rerun with
        // the same `artifact_label` would retain stale optional bundle files (e.g.
        // a previous dead-child failure leaving `screenshot.png`/`world.json`, or an
        // old `failure.json` surfacing in a non-failure `capture_artifacts`). Clear
        // the known bundle files before writing a fresh bundle.
        clear_bundle_files(&dir);
        Ok(dir)
    }

    fn write_screenshot(&self, dir: &Path) -> Result<PathBuf> {
        let abs_dir = fs::canonicalize(dir)
            .map_err(|error| Error::artifact_io("canonicalize artifact directory", error))?;
        let png_path = abs_dir.join("screenshot.png");
        let path_param = png_path.to_string_lossy().into_owned();

        let _result = self.brp(
            SCREENSHOT_METHOD,
            json!({
                "path": path_param,
            }),
        )?;

        let meta = fs::metadata(&png_path).map_err(|error| {
            Error::Artifact(format!(
                "screenshot completed but file missing at {}: {error}",
                png_path.display()
            ))
        })?;
        if meta.len() == 0 {
            return Err(Error::Artifact(format!(
                "screenshot file is empty: {}",
                png_path.display()
            )));
        }
        Ok(png_path)
    }

    fn write_world_snapshot(&self, dir: &Path) -> Result<()> {
        let type_path = E2eId::type_path();
        let params = BrpQueryParams {
            data: BrpQuery {
                components: vec![type_path.to_owned()],
                option: ComponentSelector::All,
                has: Vec::new(),
            },
            filter: BrpQueryFilter {
                with: vec![type_path.to_owned()],
                without: Vec::new(),
            },
            strict: false,
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_QUERY_METHOD.to_owned(),
            message: format!("failed to serialize world.query params: {error}"),
        })?;

        let entities = self.brp(BRP_QUERY_METHOD, params)?;
        let frame_count = self.diagnostics_frame_count_value()?;
        let snapshot = json!({
            "frame_count": frame_count,
            "entities": entities,
        });
        write_json(&dir.join("world.json"), &snapshot)
    }

    fn write_output_logs(&self, dir: &Path) -> Result<()> {
        self.child_write_output_logs(dir)
    }

    fn diagnostics_frame_count_value(&self) -> Result<Value> {
        let diagnostics = self.brp(DIAGNOSTICS_METHOD, json!({}))?;
        Ok(diagnostics
            .get("frame_count")
            .cloned()
            .unwrap_or(Value::Null))
    }
}

fn session_name(label: Option<&str>, fallback_label: Option<&str>, binary_stem: &str) -> String {
    if let Some(label) = label {
        return sanitize_label(label);
    }
    if let Some(label) = fallback_label {
        return sanitize_label(label);
    }
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{}",
        sanitize_label(binary_stem),
        std::process::id(),
        counter
    )
}

fn sanitize_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "session".to_owned()
    } else {
        out
    }
}

/// Remove the known artifact bundle files from `dir` so a rerun with the same
/// `artifact_label` cannot retain stale optional files from a prior capture.
fn clear_bundle_files(dir: &Path) {
    for name in [
        "screenshot.png",
        "world.json",
        "stdout.log",
        "stderr.log",
        "failure.json",
    ] {
        let _ = fs::remove_file(dir.join(name));
    }
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| Error::Artifact(format!("serialize {}: {error}", path.display())))?;
    fs::write(path, text)
        .map_err(|error| Error::artifact_io(&format!("write {}", path.display()), error))
}
