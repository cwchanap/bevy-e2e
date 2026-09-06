use bevy::prelude::*;

use crate::E2eId;

/// In-game plugin that activates remote E2E support when `BEVY_E2E=1`.
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
