use bevy_e2e::{E2eId, E2eLaunchOptions};
use std::{path::PathBuf, time::Duration};

#[test]
fn shared_id_and_client_options_are_stable() {
    assert_eq!(E2eId::new("player").value, "player");

    let options = E2eLaunchOptions::new("target/debug/game")
        .startup_timeout(Duration::from_secs(3))
        .operation_timeout(Duration::from_secs(4))
        .shutdown_timeout(Duration::from_secs(2))
        .artifact_root("target/e2e");

    assert_eq!(
        options.binary(),
        PathBuf::from("target/debug/game").as_path()
    );
    assert_eq!(options.startup_timeout_value(), Duration::from_secs(3));
    assert_eq!(options.operation_timeout_value(), Duration::from_secs(4));
    assert_eq!(options.shutdown_timeout_value(), Duration::from_secs(2));
}
