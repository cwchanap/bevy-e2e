use bevy::prelude::*;

use crate::E2eId;

/// In-game plugin that activates remote E2E support when `BEVY_E2E=1`.
///
/// Adds [`bevy_brp_extras::BrpExtrasPlugin`], which installs the loopback HTTP
/// BRP transport on the port chosen by the harness via `BRP_EXTRAS_PORT`.
///
/// # A pre-existing `RemoteHttpPlugin` is rejected at startup
///
/// `bevy_brp_extras` 0.22.x composes with an existing
/// [`bevy::remote::http::RemoteHttpPlugin`]: if one is already registered when
/// `BrpExtrasPlugin` builds, `BrpExtrasPlugin` skips its own HTTP transport and
/// **ignores `BRP_EXTRAS_PORT`** (it logs a warning and uses the existing
/// transport as-is). The harness polls the port it selected with
/// `BRP_EXTRAS_PORT`, so a game that registers `RemoteHttpPlugin` before
/// `BevyE2EPlugin` would make the harness poll the wrong endpoint until
/// startup times out. To make this misconfiguration fail fast instead of
/// silently timing out, [`BevyE2EPlugin::build`] panics if it detects an
/// already-added `RemoteHttpPlugin` (the message is captured on the child's
/// stderr, which the harness surfaces on the resulting child-exit failure).
/// Let `BevyE2EPlugin` own the BRP HTTP transport and do not add
/// `RemoteHttpPlugin` yourself.
///
/// # A pre-existing `RemotePlugin` is supported
///
/// Only the HTTP transport (`RemoteHttpPlugin`) is the hazard above. A
/// separately-added [`bevy::remote::RemotePlugin`] is fine: `BrpExtrasPlugin`
/// adds `RemotePlugin` only when it is absent and registers its extras methods
/// into the existing [`bevy::remote::RemoteMethods`] resource either way. A
/// game that needs custom remote methods may therefore register `RemotePlugin`
/// before `BevyE2EPlugin` and still reach diagnostics on the harness-selected
/// port.
pub struct BevyE2EPlugin;

fn runtime_enabled(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup("BEVY_E2E").as_deref() == Some("1")
}

impl Plugin for BevyE2EPlugin {
    fn build(&self, app: &mut App) {
        if !runtime_enabled(|key| std::env::var(key).ok()) {
            return;
        }

        // Reject a separately-added `RemoteHttpPlugin` before it can silently
        // desync the harness from the BRP transport. `BrpExtrasPlugin` composes
        // with an existing `RemoteHttpPlugin` by skipping its own transport and
        // ignoring `BRP_EXTRAS_PORT` (it logs a warning and uses the existing
        // transport as-is). The harness polls the port it selected with
        // `BRP_EXTRAS_PORT`, so a game that registers `RemoteHttpPlugin` first
        // would make the harness poll the wrong endpoint until startup times
        // out. Fail fast here so the misconfiguration surfaces as an immediate
        // child exit (with this message on stderr) instead of a silent startup
        // timeout.
        if app.is_plugin_added::<bevy::remote::http::RemoteHttpPlugin>() {
            panic!(
                "BevyE2EPlugin: `bevy::remote::http::RemoteHttpPlugin` is already registered. \
                 BrpExtrasPlugin would skip its own HTTP transport and ignore BRP_EXTRAS_PORT, \
                 so the harness would poll the wrong port and time out at startup. \
                 Remove the separately-added RemoteHttpPlugin and let BevyE2EPlugin own the \
                 BRP HTTP transport."
            );
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
