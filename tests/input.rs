mod common;

use bevy::{input::mouse::MouseButton, math::Vec2, prelude::KeyCode};
use bevy_e2e::{E2eLaunchOptions, Game, cargo_bin};
use common::FIXTURE_STATE_TYPE;

fn fixture_options() -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
}

#[test]
fn keyboard_reaches_button_input() {
    let game = Game::launch(fixture_options()).unwrap();

    game.key_down(KeyCode::Space).unwrap();
    game.wait_frames(1).unwrap();

    let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
    assert!(state["key_is_down"].as_bool().unwrap());
    assert_eq!(state["space_press_count"], 1);

    game.key_up(KeyCode::Space).unwrap();
    game.wait_frames(1).unwrap();
    assert!(
        !game.resource_json(FIXTURE_STATE_TYPE).unwrap()["key_is_down"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn mouse_button_reaches_button_input() {
    let game = Game::launch(fixture_options()).unwrap();

    game.mouse_down(MouseButton::Left).unwrap();
    game.wait_frames(1).unwrap();
    assert!(
        game.resource_json(FIXTURE_STATE_TYPE).unwrap()["mouse_left_is_down"]
            .as_bool()
            .unwrap()
    );

    game.mouse_up(MouseButton::Left).unwrap();
    game.wait_frames(1).unwrap();
}

#[test]
fn click_selector_drives_ui_interaction() {
    let game = Game::launch(fixture_options()).unwrap();

    game.click("main_menu.play").unwrap();
    game.wait_for("gameplay.hud").unwrap();
    assert!(
        game.resource_json(FIXTURE_STATE_TYPE).unwrap()["play_clicked"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn move_mouse_updates_window_cursor_position() {
    let game = Game::launch(fixture_options()).unwrap();

    let pos = Vec2::new(120.0, 80.0);
    game.move_mouse(pos).unwrap();
    game.wait_frames(1).unwrap();

    let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
    assert_eq!(state["cursor_position"][0], 120.0);
    assert_eq!(state["cursor_position"][1], 80.0);
}

#[test]
fn ui_global_transform_brp_shape_is_pinned() {
    let game = Game::launch(fixture_options()).unwrap();

    let transform = game
        .component_json("main_menu.play", "bevy_ui::ui_transform::UiGlobalTransform")
        .unwrap();
    let arr = transform
        .as_array()
        .expect("UiGlobalTransform must be an array");
    assert!(
        arr.len() >= 6,
        "expected flat Affine2 length >= 6, got {}",
        arr.len()
    );
    assert!(arr[4].as_f64().is_some(), "index 4 must be numeric");
    assert!(arr[5].as_f64().is_some(), "index 5 must be numeric");
}
