use bevy_e2e::{E2eLaunchOptions, Error, Game, cargo_bin};

fn fixture_options() -> E2eLaunchOptions {
    E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))
}

#[test]
fn find_and_exists_one_match() {
    let game = Game::launch(fixture_options()).unwrap();
    assert!(game.exists("player").unwrap());
    game.find("player").unwrap();
    assert!(game.exists("main_menu.play").unwrap());
    game.find("main_menu.play").unwrap();
}

#[test]
fn exists_missing_is_false_find_errors() {
    let game = Game::launch(fixture_options()).unwrap();
    assert!(!game.exists("missing").unwrap());
    assert!(matches!(
        game.find("missing"),
        Err(Error::SelectorNotFound(_))
    ));
}

#[test]
fn duplicate_id_is_ambiguous() {
    let game = Game::launch(fixture_options().arg("--duplicate-id")).unwrap();
    assert!(matches!(
        game.exists("player"),
        Err(Error::AmbiguousSelector(_))
    ));
    assert!(matches!(
        game.find("player"),
        Err(Error::AmbiguousSelector(_))
    ));
}
