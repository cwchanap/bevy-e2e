use std::time::Duration;

use bevy_e2e::{E2eLaunchOptions, Error, Game, Result, cargo_bin};
use serde_json::json;

fn fixture_options() -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
}

#[test]
fn launch_reaches_diagnostics_and_shutdown_reaps() {
    let mut game = Game::launch(fixture_options()).unwrap();
    assert!(game.is_running());
    game.shutdown().unwrap();
    assert!(!game.is_running());
}

#[test]
fn missing_runtime_returns_startup_timeout_and_reaps() {
    let options = fixture_options()
        .arg("--skip-e2e-plugin")
        .startup_timeout(Duration::from_millis(500));
    assert!(matches!(Game::launch(options), Err(Error::Timeout { .. })));
}

#[test]
fn two_children_can_answer_distinct_main_brp_sessions() {
    let a = std::thread::spawn(|| Game::launch(fixture_options()));
    let b = std::thread::spawn(|| Game::launch(fixture_options()));

    let mut a = a.join().unwrap().unwrap();
    let mut b = b.join().unwrap().unwrap();

    assert!(a.brp("brp_extras/get_diagnostics", json!({})).is_ok());
    assert!(b.brp("brp_extras/get_diagnostics", json!({})).is_ok());

    a.shutdown().unwrap();
    b.shutdown().unwrap();
}

#[test]
fn force_kill_reaps_sleep_forever_child() {
    let mut child =
        bevy_e2e::ChildProcess::spawn(&fixture_options().arg("--sleep-forever")).unwrap();
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    let status = child.reap(std::time::Duration::from_secs(2)).unwrap();
    assert!(status.is_some(), "force-kill must reap an exit status");
    assert!(!child.is_running());
}

#[test]
fn run_shuts_down_after_closure() {
    bevy_e2e::run(fixture_options(), |game| -> Result<()> {
        assert!(game.is_running());
        let _ = game.brp("brp_extras/get_diagnostics", json!({}))?;
        Ok(())
    })
    .unwrap();
}
