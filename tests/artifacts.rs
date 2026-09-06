use std::path::PathBuf;

use bevy_e2e::{E2eLaunchOptions, Game, cargo_bin};

fn fixture_options(artifact_root: impl Into<PathBuf>) -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture")).artifact_root(artifact_root)
}

#[test]
fn screenshot_captures_non_uniform_rendered_content() {
    let root = PathBuf::from("test_output/artifacts_screenshot");
    let game = Game::launch(fixture_options(&root)).unwrap();

    game.wait_for("main_menu.play").unwrap();
    game.wait_frames(2).unwrap();

    let path = game.screenshot("menu").unwrap();
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
