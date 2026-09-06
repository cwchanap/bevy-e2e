use std::time::{Duration, Instant};

use bevy_e2e::{E2eLaunchOptions, Error, Game, Result, cargo_bin};
use serde_json::json;

fn fixture_options() -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
}

fn frame_count(game: &Game) -> Result<Option<u64>> {
    let diagnostics = game.brp("brp_extras/get_diagnostics", json!({}))?;
    Ok(diagnostics
        .get("frame_count")
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))))
}

#[test]
fn wait_for_player_is_immediate() {
    let game = Game::launch(fixture_options()).unwrap();
    game.wait_for("player").unwrap();
}

#[test]
fn wait_for_missing_times_out() {
    let game =
        Game::launch(fixture_options().operation_timeout(Duration::from_millis(300))).unwrap();
    let err = game.wait_for("missing").expect_err("must time out");
    assert!(matches!(err, Error::Timeout { .. }), "got {err:?}");
}

#[test]
fn wait_frames_advances_diagnostics_count() {
    let game = Game::launch(fixture_options()).unwrap();
    // Ensure we have a numeric baseline before measuring (mirrors wait_frames).
    let before = {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(n) = frame_count(&game).unwrap() {
                break n;
            }
            assert!(Instant::now() < deadline, "frame_count stayed null");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    game.wait_frames(2).unwrap();
    let after = frame_count(&game).unwrap().expect("numeric after wait");
    assert!(
        after >= before + 2,
        "expected frame delta >= 2, before={before} after={after}"
    );
}

#[test]
fn wait_until_success_and_timeout() {
    let game = Game::launch(fixture_options()).unwrap();
    game.wait_until(Duration::from_secs(2), |g| g.exists("player"))
        .unwrap();

    let err = game
        .wait_until(Duration::from_millis(200), |g| g.exists("missing"))
        .expect_err("predicate never true");
    assert!(matches!(err, Error::Timeout { .. }), "got {err:?}");
}
