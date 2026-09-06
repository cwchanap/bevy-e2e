mod id;
pub use id::E2eId;

#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::BevyE2EPlugin;

#[cfg(feature = "client")]
mod artifacts;
#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
mod error;
#[cfg(feature = "client")]
mod game;
#[cfg(feature = "client")]
mod input;
#[cfg(feature = "client")]
mod inspect;
#[cfg(feature = "client")]
mod options;
#[cfg(feature = "client")]
mod process;
#[cfg(feature = "client")]
mod selector;
#[cfg(feature = "client")]
mod wait;

#[cfg(feature = "client")]
pub use error::{Error, Result};
#[cfg(feature = "client")]
pub use game::{Game, run};
#[cfg(feature = "client")]
pub use options::E2eLaunchOptions;
#[cfg(feature = "client")]
pub use process::ChildProcess;

#[macro_export]
macro_rules! cargo_bin {
    ($name:literal) => {
        std::path::PathBuf::from(env!(concat!("CARGO_BIN_EXE_", $name)))
    };
}
