mod common;

use bevy_e2e::{E2eLaunchOptions, Game, cargo_bin};
use common::{FIXTURE_STATE_TYPE, HEALTH_TYPE};

fn fixture_options() -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
}

#[test]
fn component_and_resource_json_reads() {
    let game = Game::launch(fixture_options()).unwrap();

    let health = game.component_json("player", HEALTH_TYPE).unwrap();
    assert_eq!(health["current"], 100.0);

    let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
    assert_eq!(state["play_clicked"], false);
}
