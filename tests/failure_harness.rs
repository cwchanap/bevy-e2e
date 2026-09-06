use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use bevy_e2e::{E2eLaunchOptions, cargo_bin};

fn fixture_options(label: &str) -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
        .artifact_root("test_output/failure_harness")
        .artifact_label(label)
}

fn artifact_dir(label: &str) -> PathBuf {
    PathBuf::from("test_output/failure_harness").join(label)
}

fn clear_artifact_dir(label: &str) {
    let dir = artifact_dir(label);
    let _ = fs::remove_dir_all(&dir);
}

fn run_ignored_helper(name: &str) -> std::process::Output {
    let exe = std::env::current_exe().expect("current test binary");
    Command::new(exe)
        .args(["--exact", "--ignored", name])
        .output()
        .expect("spawn ignored helper")
}

/// Cross-platform "is this PID still alive?" check (no shell).
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(unix)]
    {
        // Signal 0 tests existence without delivering a signal.
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // SAFETY: kill(pid, 0) is a pure existence probe.
        unsafe { kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
            fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
            fn GetExitCodeProcess(handle: *mut core::ffi::c_void, code: *mut u32) -> i32;
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        // SAFETY: Win32 process query APIs with a borrowed handle closed below.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

fn read_failure_json(label: &str) -> serde_json::Value {
    let path = artifact_dir(label).join("failure.json");
    assert!(path.is_file(), "missing failure.json at {}", path.display());
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read {}: {e}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("parse {}: {e}\n{text}", path.display());
    })
}

fn assert_child_pid_reaped(failure: &serde_json::Value) {
    let pid = failure
        .get("child_pid")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("failure.json missing numeric child_pid: {failure}"));
    let pid = u32::try_from(pid).expect("child_pid fits u32");
    assert!(
        !process_is_alive(pid),
        "child pid {pid} still alive after helper exit"
    );
}

fn assert_log_contains(dir: &Path, name: &str, marker: &str) {
    let path = dir.join(name);
    assert!(path.is_file(), "missing {name} at {}", path.display());
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(marker),
        "{name} missing marker {marker:?}; contents:\n{text}"
    );
}

#[test]
#[ignore]
fn helper_returns_error() {
    bevy_e2e::run(fixture_options("returned-error"), |_game| {
        Err(bevy_e2e::Error::Configuration(
            "intentional returned error".into(),
        ))
    })
    .unwrap();
}

#[test]
#[ignore]
fn helper_panics() {
    bevy_e2e::run(fixture_options("panic"), |_game| -> bevy_e2e::Result<()> {
        panic!("intentional panic");
    })
    .unwrap();
}

#[test]
#[ignore]
fn helper_child_exits_mid_test() {
    let options = fixture_options("dead-child").arg("--exit-after-ready-ms=250");

    bevy_e2e::run(options, |game| {
        std::thread::sleep(Duration::from_millis(400));
        game.exists("player")?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn returned_error_writes_failure_bundle() {
    clear_artifact_dir("returned-error");
    let output = run_ignored_helper("helper_returns_error");
    assert!(
        !output.status.success(),
        "helper should fail; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dir = artifact_dir("returned-error");
    let failure = read_failure_json("returned-error");
    let error = failure
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        error.contains("intentional returned error"),
        "failure.json error missing original Err text: {failure}"
    );
    assert_child_pid_reaped(&failure);
    assert!(dir.join("stdout.log").is_file());
    assert!(dir.join("stderr.log").is_file());
}

#[test]
fn panic_writes_failure_bundle() {
    clear_artifact_dir("panic");
    let output = run_ignored_helper("helper_panics");
    assert!(
        !output.status.success(),
        "helper should fail; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dir = artifact_dir("panic");
    let failure = read_failure_json("panic");
    let error = failure
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        error.contains("intentional panic"),
        "failure.json error missing panic text: {failure}"
    );
    assert_child_pid_reaped(&failure);
    assert!(dir.join("stdout.log").is_file());
    assert!(dir.join("stderr.log").is_file());
}

#[test]
fn dead_child_writes_local_failure_artifacts() {
    clear_artifact_dir("dead-child");
    let output = run_ignored_helper("helper_child_exits_mid_test");
    assert!(
        !output.status.success(),
        "helper should fail; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dir = artifact_dir("dead-child");
    let failure = read_failure_json("dead-child");
    assert!(
        failure.get("error").and_then(|v| v.as_str()).is_some(),
        "failure.json missing error: {failure}"
    );
    // Remote screenshot/world are optional when BRP is dead; local metadata/output required.
    assert_log_contains(&dir, "stdout.log", "fixture stdout before crash");
    assert_log_contains(&dir, "stderr.log", "fixture stderr before crash");
    assert_child_pid_reaped(&failure);

    // Primary failure must remain the BRP/child-exit failure, not a remote artifact error.
    let error = failure["error"].as_str().unwrap_or_default();
    assert!(
        !error.to_lowercase().contains("screenshot")
            && !error.to_lowercase().contains("world.json"),
        "remote artifact failure must not replace primary failure: {failure}"
    );
}
