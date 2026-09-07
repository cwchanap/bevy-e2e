use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use bevy_e2e::{E2eLaunchOptions, Game, cargo_bin};

fn fixture_options(artifact_root: impl Into<PathBuf>) -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture")).artifact_root(artifact_root)
}

/// Upper bound for waiting on the first *presented* render frame.
///
/// GPU-backed hosts present within a couple of frames, but CI runners without a
/// GPU fall back to a software adapter (e.g. Windows "Microsoft Basic Render
/// Driver" / WARP via DX12) that is far slower to surface its first frame. A
/// fixed `wait_frames` count that works on a fast GPU can capture a
/// zero-initialized (pure-black) texture on a slow one. Polling until the
/// capture is non-uniform expresses the actual requirement directly and is
/// independent of adapter speed.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(20);

/// True when the decoded screenshot contains more than one distinct pixel
/// color (i.e. the renderer has presented real content, not a blank texture).
fn screenshot_has_visible_content(path: &std::path::Path) -> bool {
    let Ok(image) = image::open(path) else {
        return false;
    };
    let image = image.to_rgb8();
    if image.width() == 0 || image.height() == 0 {
        return false;
    }
    let first = image.get_pixel(0, 0);
    image.pixels().any(|pixel| pixel != first)
}

#[test]
fn screenshot_captures_non_uniform_rendered_content() {
    let root = PathBuf::from("test_output/artifacts_screenshot");
    let game = Game::launch(fixture_options(&root)).unwrap();

    game.wait_for("main_menu.play").unwrap();

    // Poll for a non-uniform capture rather than assuming a fixed frame count
    // is enough for the first frame to be presented on every adapter.
    let deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
    let path = loop {
        game.wait_frames(2).unwrap();
        let candidate = game.screenshot("menu").unwrap();
        if screenshot_has_visible_content(&candidate) || Instant::now() >= deadline {
            // On timeout, keep the last capture so the assertions below report
            // the real failure (uniform/black pixels) instead of a generic
            // timeout message.
            break candidate;
        }
        thread::sleep(Duration::from_millis(50));
    };

    assert!(path.exists(), "screenshot path missing: {}", path.display());
    assert!(
        path.to_string_lossy().contains("menu"),
        "expected session label in path: {}",
        path.display()
    );

    let image = image::open(&path).unwrap().to_rgb8();
    assert!(image.width() > 0 && image.height() > 0);

    let first = image.get_pixel(0, 0);
    assert!(image.pixels().any(|pixel| pixel != first));

    let average = image
        .pixels()
        .map(|p| (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3)
        .sum::<u32>() as f64
        / f64::from(image.width() * image.height());
    assert!(
        average > 2.0,
        "average luminance {average} too dark/uniform"
    );
}

#[test]
fn capture_artifacts_writes_expected_bundle_without_failure_json() {
    let root = PathBuf::from("test_output/artifacts_checkpoint");
    let game = Game::launch(fixture_options(&root)).unwrap();

    game.wait_for("main_menu.play").unwrap();
    game.wait_frames(2).unwrap();

    let dir = game.capture_artifacts("checkpoint").unwrap();
    assert!(dir.is_dir(), "artifact dir missing: {}", dir.display());

    for name in ["screenshot.png", "world.json", "stdout.log", "stderr.log"] {
        let path = dir.join(name);
        assert!(path.is_file(), "missing {name} at {}", path.display());
    }
    assert!(
        std::fs::metadata(dir.join("screenshot.png")).unwrap().len() > 0,
        "screenshot.png should be non-empty"
    );
    assert!(
        std::fs::metadata(dir.join("world.json")).unwrap().len() > 0,
        "world.json should be non-empty"
    );

    assert!(
        !dir.join("failure.json").exists(),
        "explicit non-failure capture must omit failure.json"
    );

    let world: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("world.json")).unwrap()).unwrap();
    assert!(world.get("frame_count").is_some());
    assert!(world.get("entities").and_then(|v| v.as_array()).is_some());
}
