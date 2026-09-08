use bevy::prelude::*;

use crate::E2eId;

/// In-game plugin that activates remote E2E support when `BEVY_E2E=1`.
///
/// Adds [`bevy_brp_extras::BrpExtrasPlugin`], which installs the loopback HTTP
/// BRP transport on the port chosen by the harness via `BRP_EXTRAS_PORT`.
///
/// # Do not add `RemoteHttpPlugin` / `RemotePlugin` separately
///
/// `bevy_brp_extras` 0.22.x composes with an existing
/// [`bevy::remote::http::RemoteHttpPlugin`]: if one is already registered when
/// `BrpExtrasPlugin` builds, `BrpExtrasPlugin` skips its own HTTP transport and
/// **ignores `BRP_EXTRAS_PORT`** (it logs a warning and uses the existing
/// transport as-is). The harness polls the port it selected with
/// `BRP_EXTRAS_PORT`, so a game that registers `RemoteHttpPlugin` before
/// `BevyE2EPlugin` makes the harness poll the wrong endpoint until startup times
/// out. Let `BevyE2EPlugin` own the BRP HTTP transport and do not add
/// `RemoteHttpPlugin` or `RemotePlugin` yourself.
pub struct BevyE2EPlugin;

fn runtime_enabled(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup("BEVY_E2E").as_deref() == Some("1")
}

impl Plugin for BevyE2EPlugin {
    fn build(&self, app: &mut App) {
        if !runtime_enabled(|key| std::env::var(key).ok()) {
            return;
        }

        app.register_type::<E2eId>();
        app.add_plugins(bevy_brp_extras::BrpExtrasPlugin);
    }
}

#[cfg(test)]
mod tests {
    use super::runtime_enabled;

    #[test]
    fn activation_requires_exact_one() {
        assert!(runtime_enabled(
            |key| (key == "BEVY_E2E").then(|| "1".into())
        ));
        assert!(!runtime_enabled(|_| None));
        assert!(!runtime_enabled(|_| Some("0".into())));
    }
}
