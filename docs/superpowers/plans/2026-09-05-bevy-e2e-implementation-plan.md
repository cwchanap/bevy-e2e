# Bevy E2E v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the v0.1 `bevy_e2e` crate: synchronous Rust tests launch a rendered Bevy 0.19.x child process, control it over BRP, interact through keyboard/mouse/UI, inspect reflected ECS state, and capture failure diagnostics.

**Architecture:** Publish one crate with a parent-side synchronous BRP/process facade and a feature-gated `BevyE2EPlugin` embedded in the tested game. Reuse built-in BRP for query/component/resource/message operations and `bevy_brp_extras` for HTTP setup, screenshot, shutdown, and input behavior where useful; v0.1 adds no custom RPC methods. Every `run()` call owns one child process and one loopback BRP port.

**Tech Stack:** Rust 2024, Rust 1.95+, Bevy 0.19.1, `bevy_brp_extras` 0.22.3, BRP JSON-RPC/HTTP, `ureq` 3.0.8, Serde/serde_json, thiserror, GitHub Actions, Xvfb on Linux.

**Spec:** `docs/superpowers/specs/2026-09-04-bevy-e2e-design.md`

## Global Constraints

- Implement all work in one feature PR; task commits are checkpoints inside that PR, not separate PRs.
- Target Bevy 0.19.x only; do not add multi-version compatibility code.
- Keep one public crate: `bevy_e2e`.
- Use ordinary synchronous Rust `#[test]`; do not require Tokio or a custom test runner.
- One child process per test; no pooling or reset protocol.
- BRP is the only transport; do not add a second RPC protocol.
- v0.1 adds no custom BRP methods unless implementation proves built-in BRP plus `bevy_brp_extras` cannot satisfy an approved behavior.
- Bind remote control to `127.0.0.1` only and require `BEVY_E2E=1` runtime activation.
- `E2eId` is the stable selector. Raw Bevy `Entity` values are diagnostic/transport details only.
- Player-facing input is preferred; ECS convenience APIs remain read-oriented.
- Rendered desktop execution is the generic path; do not add a generic headless-mode switch.
- Linux/Xvfb and Windows are release gates; macOS CI is deferred.
- Keep public APIs explicit and small; raw `Game::brp` is the escape hatch instead of wrapper proliferation.

---

## File Structure

Create this structure during the tasks below:

```text
.
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          # public exports, run(), cargo_bin! macro
│   ├── error.rs        # framework error and Result alias
│   ├── options.rs      # E2eLaunchOptions
│   ├── client.rs       # synchronous BRP HTTP client
│   ├── process.rs      # spawn, pipe draining, exit/kill/reap
│   ├── game.rs         # public Game facade and lifecycle
│   ├── runtime.rs      # BevyE2EPlugin, E2eId, E2eFrame, E2eRuntimeInfo
│   ├── selector.rs     # E2eId resolution
│   ├── inspect.rs      # reflected component/resource reads
│   ├── wait.rs         # selector/frame/predicate waits
│   ├── input.rs        # keyboard/mouse/UI-click composition
│   └── artifacts.rs    # screenshot/world/failure/stdout/stderr bundles
├── tests/
│   ├── support/mod.rs
│   ├── brp_client.rs
│   ├── runtime.rs
│   ├── lifecycle.rs
│   ├── selectors.rs
│   ├── inspection.rs
│   ├── input.rs
│   ├── artifacts.rs
│   ├── failure_harness.rs
│   ├── public_api.rs
│   └── fixtures/minimal_game.rs
├── scripts/assert_no_fixture_processes.sh
├── docs/superpowers/specs/2026-09-04-bevy-e2e-design.md
├── docs/superpowers/plans/2026-09-05-bevy-e2e-implementation-plan.md
└── .github/workflows/ci.yml
```

Each source file has one responsibility. Do not collapse process management, BRP transport, input, and artifacts into a single large `Game` implementation.

---

### Task 1: Bootstrap the crate and public foundation

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/options.rs`
- Create: `tests/public_api.rs`
- Modify: `README.md`

**Interfaces:**
- Produces: `pub type Result<T>`, `pub enum Error`, `pub struct E2eLaunchOptions`, `cargo_bin!`, and module boundaries used by every later task.
- `E2eLaunchOptions::new(binary: impl Into<PathBuf>) -> Self`
- Builder methods: `arg`, `env`, `startup_timeout`, `operation_timeout`, `shutdown_timeout`, `artifact_root`, `artifact_label`.

- [ ] **Step 1: Add the crate manifest and exact dependency baseline**

Create `Cargo.toml`:

```toml
[package]
name = "bevy_e2e"
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "MIT OR Apache-2.0"
repository = "https://github.com/cwchanap/bevy-e2e"
description = "Out-of-process end-to-end testing for Bevy games"

[dependencies]
bevy = { version = "0.19.1", default-features = false, features = [
  "bevy_remote",
  "bevy_render",
  "bevy_ui",
  "bevy_window",
  "png",
  "serialize",
] }
bevy_brp_extras = "0.22.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
ureq = { version = "3.0.8", features = ["json"] }

[dev-dependencies]
bevy = { version = "0.19.1", features = ["bevy_remote", "png", "serialize"] }
tiny_http = "0.12"

[[bin]]
name = "bevy-e2e-fixture"
path = "tests/fixtures/minimal_game.rs"
```

Do not add a workspace, proc-macro crate, runtime crate, or CLI crate.

- [ ] **Step 2: Write the failing public API compile test**

Create `tests/public_api.rs`:

```rust
use std::{path::PathBuf, time::Duration};
use bevy_e2e::E2eLaunchOptions;

#[test]
fn launch_options_keep_the_explicit_binary_and_timeouts() {
    let options = E2eLaunchOptions::new("target/debug/game")
        .startup_timeout(Duration::from_secs(3))
        .operation_timeout(Duration::from_secs(4))
        .shutdown_timeout(Duration::from_secs(2))
        .artifact_root("tmp/e2e");

    assert_eq!(options.binary(), PathBuf::from("target/debug/game").as_path());
    assert_eq!(options.startup_timeout_value(), Duration::from_secs(3));
    assert_eq!(options.operation_timeout_value(), Duration::from_secs(4));
    assert_eq!(options.shutdown_timeout_value(), Duration::from_secs(2));
}
```

- [ ] **Step 3: Run the test and verify it fails because the crate API does not exist**

Run:

```bash
cargo test --test public_api
```

Expected: compile failure for unresolved `bevy_e2e::E2eLaunchOptions` or missing source files.

- [ ] **Step 4: Implement the minimal error/options/public-export foundation**

`src/error.rs` should define one framework error with concrete variants used by later tasks:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("BRP request `{method}` failed: {message}")]
    Brp { method: String, message: String },
    #[error("selector `{0}` was not found")]
    SelectorNotFound(String),
    #[error("selector `{0}` matched more than one entity")]
    AmbiguousSelector(String),
    #[error("operation `{operation}` timed out after {timeout:?}")]
    Timeout { operation: String, timeout: std::time::Duration },
    #[error("child process exited unexpectedly with status {0}")]
    ChildExited(std::process::ExitStatus),
    #[error("artifact operation failed: {0}")]
    Artifact(String),
    #[error("invalid E2E configuration: {0}")]
    Configuration(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`src/options.rs` should keep fields private, expose read-only accessors needed by internal modules/tests, and default to:

```rust
startup_timeout = Duration::from_secs(10)
operation_timeout = Duration::from_secs(5)
shutdown_timeout = Duration::from_secs(3)
artifact_root = PathBuf::from("test_output")
artifact_label = None
args = Vec::new()
env = Vec::new()
```

`src/lib.rs` exports `Error`, `Result`, and `E2eLaunchOptions`, and defines:

```rust
#[macro_export]
macro_rules! cargo_bin {
    ($name:literal) => {
        std::path::PathBuf::from(env!(concat!("CARGO_BIN_EXE_", $name)))
    };
}
```

- [ ] **Step 5: Run formatting and the public API test**

```bash
cargo fmt --check
cargo test --test public_api
```

Expected: PASS.

- [ ] **Step 6: Commit the checkpoint**

```bash
git add Cargo.toml README.md src tests/public_api.rs
git commit -m "feat: bootstrap bevy e2e crate"
```

---

### Task 2: Implement the synchronous BRP client

**Files:**
- Create: `src/client.rs`
- Create: `tests/brp_client.rs`
- Create: `tests/support/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`

**Interfaces:**
- Produces crate-private `BrpClient::new(port: u16) -> Self`.
- Produces `BrpClient::request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value>`.
- Later tasks rely on remote JSON-RPC errors being preserved in `Error::Brp`.

- [ ] **Step 1: Write a deterministic fake-HTTP BRP test**

`tests/support/mod.rs` should provide a `TestServer` backed by `tiny_http` that binds `127.0.0.1:0`, records one POST body, and answers with caller-provided JSON.

`tests/brp_client.rs`:

```rust
mod support;

use bevy_e2e::testing::BrpClient;
use serde_json::json;

#[test]
fn brp_client_posts_json_rpc_and_returns_result() {
    let server = support::TestServer::once(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"ready": true}
    }));

    let client = BrpClient::new(server.port());
    let value = client.request("world.get_resources", json!({"resource": "game::Ready"})).unwrap();

    assert_eq!(value, json!({"ready": true}));
    let request = server.received_json();
    assert_eq!(request["method"], "world.get_resources");
}
```

Add a second test where the server returns a JSON-RPC `error` and assert the method and remote message are present in `Error::Brp` display text.

Expose `BrpClient` only under `#[doc(hidden)] pub mod testing` while the crate is being integration-tested; do not make it part of the documented product API.

- [ ] **Step 2: Verify failure**

```bash
cargo test --test brp_client
```

Expected: compile failure because `BrpClient` is missing.

- [ ] **Step 3: Implement the minimal synchronous client with `ureq`**

Use Bevy's BRP request shape and a monotonic request ID. POST to:

```text
http://127.0.0.1:<port>/
```

with JSON-RPC 2.0. Parse `{ result }` or `{ error }`; reject malformed responses as `Error::Brp` with the method name and response context. Keep all HTTP/network details inside `client.rs`.

Do not add async runtime, retries, auth, or connection pooling abstractions.

- [ ] **Step 4: Run focused tests**

```bash
cargo test --test brp_client
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/client.rs src/error.rs src/lib.rs tests/brp_client.rs tests/support/mod.rs
git commit -m "feat: add synchronous BRP client"
```

---

### Task 3: Add the E2E runtime plugin and rendered fixture

**Files:**
- Create: `src/runtime.rs`
- Create: `tests/runtime.rs`
- Create: `tests/fixtures/minimal_game.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces public `BevyE2EPlugin` and `E2eId`.
- Produces reflected crate-private `E2eFrame { frame: u64 }` and `E2eRuntimeInfo { protocol: u32 }`.
- `BevyE2EPlugin` does nothing unless `BEVY_E2E=1`.
- When active, it registers/inserts the runtime types/resources, increments `E2eFrame` every update, and adds `bevy_brp_extras::BrpExtrasPlugin` configured by `BRP_EXTRAS_PORT`.

- [ ] **Step 1: Write runtime unit tests before the fixture launch test**

`tests/runtime.rs` should cover both gates:

```rust
#[test]
fn plugin_is_inert_without_runtime_activation() {
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy_e2e::BevyE2EPlugin);
    app.update();
    assert!(app.world().get_resource::<bevy_e2e::E2eRuntimeInfo>().is_none());
}
```

For the active path, avoid mutating global process environment in the same test process. Factor runtime activation parsing into a crate-private pure function and unit-test it with an explicit lookup closure/map:

```rust
assert!(runtime_enabled(|key| (key == "BEVY_E2E").then(|| "1".into())));
assert!(!runtime_enabled(|_| None));
```

- [ ] **Step 2: Verify runtime tests fail**

```bash
cargo test --test runtime
```

Expected: compile failure for missing runtime types/plugin.

- [ ] **Step 3: Implement runtime types and plugin**

Use a stable reflected selector representation:

```rust
#[derive(Component, Reflect, Clone, Debug, PartialEq, Eq)]
#[reflect(Component)]
pub struct E2eId {
    pub value: String,
}

impl E2eId {
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into() }
    }
}
```

Runtime resources:

```rust
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct E2eFrame { pub frame: u64 }

#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct E2eRuntimeInfo { pub protocol: u32 }
```

`E2eRuntimeInfo.protocol` is `1` for v0.1. The frame system increments with saturating addition.

Do not register custom remote methods.

- [ ] **Step 4: Build the minimal rendered fixture**

`tests/fixtures/minimal_game.rs` should create a small `DefaultPlugins` app containing:

- primary window with deterministic size;
- root UI;
- button `E2eId::new("main_menu.play")`;
- text/status entity `E2eId::new("gameplay.hud")` hidden until the button is pressed;
- player entity `E2eId::new("player")` with reflected `Health { current: 100 }`;
- reflected `FixtureState` resource recording key/mouse/UI actions;
- optional second `player` entity when `--duplicate-id` is passed;
- `--skip-e2e-plugin` fixture switch for startup-timeout testing;
- `--sleep-forever` switch that blocks before the Bevy app starts for force-kill testing.

Register `Health` and `FixtureState` for reflection. Add `BevyE2EPlugin` normally.

- [ ] **Step 5: Verify the fixture builds**

```bash
cargo build --bin bevy-e2e-fixture
cargo test --test runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/runtime.rs src/lib.rs tests/runtime.rs tests/fixtures/minimal_game.rs
git commit -m "feat: add bevy e2e runtime plugin"
```

---

### Task 4: Implement child-process lifecycle and `run()`

**Files:**
- Create: `src/process.rs`
- Create: `src/game.rs`
- Create: `tests/lifecycle.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`

**Interfaces:**
- Produces public `Game::launch(options) -> Result<Game>` and `Game::shutdown(&mut self) -> Result<()>`.
- Produces public `run(options, FnOnce(&mut Game) -> Result<()>) -> Result<()>`.
- `Game` owns `BrpClient`, child process, output buffers, options, session start time, and artifact session identity.
- Startup readiness is successful `world.get_resources` for `type_name::<E2eRuntimeInfo>()` with `protocol == 1`.

- [ ] **Step 1: Write launch/readiness/shutdown integration tests**

`tests/lifecycle.rs`:

```rust
use bevy_e2e::{cargo_bin, E2eLaunchOptions, Game};

#[test]
fn game_launches_waits_for_brp_and_shuts_down() {
    let mut game = Game::launch(E2eLaunchOptions::new(cargo_bin!("bevy-e2e-fixture"))).unwrap();
    assert!(game.is_running());
    game.shutdown().unwrap();
    assert!(!game.is_running());
}
```

Add a startup-timeout test using `.arg("--skip-e2e-plugin")` and a short startup timeout. Add a parallelism test launching two games in separate threads and assert both become ready.

- [ ] **Step 2: Verify tests fail**

```bash
cargo test --test lifecycle -- --test-threads=1
```

Expected: compile failure because `Game` is missing.

- [ ] **Step 3: Implement `ChildProcess`**

`process.rs` responsibilities:

1. Choose a candidate port by binding `TcpListener::bind((Ipv4Addr::LOCALHOST, 0))`, reading `local_addr().port()`, then dropping the listener.
2. Spawn the requested binary with caller args/env followed by framework-owned overrides:

```text
BEVY_E2E=1
BRP_EXTRAS_PORT=<candidate>
```

3. Pipe stdout/stderr and immediately drain each on a dedicated reader thread into shared byte buffers.
4. Detect early child exit during startup.
5. Expose `try_wait`, bounded `wait_for_exit`, `kill`, and `reap` primitives.

If bind/spawn races make a candidate unavailable, `Game::launch` may retry the whole spawn with a new port up to three times only for connection/bind-style startup failures. Other child failures return immediately.

- [ ] **Step 4: Implement readiness and graceful shutdown**

Poll every ~25 ms until `startup_timeout` for:

```text
world.get_resources
resource = type_name::<E2eRuntimeInfo>()
```

and require `protocol == 1`.

Graceful shutdown calls:

```text
brp_extras/shutdown
```

then waits `shutdown_timeout`, force-kills if still alive, and reaps. Calling `shutdown()` twice is safe.

- [ ] **Step 5: Implement `run()` with panic preservation**

Use `std::panic::catch_unwind(AssertUnwindSafe(...))` around the test closure. The success path shuts down. For `Err` and panic, call a crate-private `capture_failure_best_effort` hook (implemented fully in Task 7), then clean up and propagate the original error/panic.

Until Task 7, the hook may be an empty private function; do not expose incomplete public artifact behavior.

- [ ] **Step 6: Add force-kill coverage**

Create a crate-private process-level test that spawns the fixture with `--sleep-forever`, calls the same kill/reap primitive used after shutdown timeout, and asserts the PID is no longer running. This tests force-kill without inventing a Bevy-side refusal mode.

- [ ] **Step 7: Run focused verification**

```bash
cargo test --test lifecycle -- --test-threads=1
cargo test --test lifecycle -- --test-threads=4
```

Expected: PASS in both modes.

- [ ] **Step 8: Commit**

```bash
git add src/process.rs src/game.rs src/lib.rs src/error.rs tests/lifecycle.rs
git commit -m "feat: manage bevy child lifecycle"
```

---

### Task 5: Add selectors, reflected inspection, and waits

**Files:**
- Create: `src/selector.rs`
- Create: `src/inspect.rs`
- Create: `src/wait.rs`
- Create: `tests/selectors.rs`
- Create: `tests/inspection.rs`
- Modify: `src/game.rs`

**Interfaces:**
- `Game::exists(id) -> Result<bool>`
- `Game::find(id) -> Result<()>`
- crate-private `resolve_entity(id) -> Result<u64-or-BRP-entity-value>`; do not expose it publicly.
- `Game::component_json(id, type_path) -> Result<Value>`
- `Game::resource_json(type_path) -> Result<Value>`
- optional typed `component<T>` / `resource<T>` using `DeserializeOwned + TypePath` when cleanly expressible.
- `Game::wait_for`, `wait_for_gone`, `wait_frames`, `wait`, `wait_until`.

- [ ] **Step 1: Write strict selector tests**

`tests/selectors.rs` must assert:

```rust
assert!(game.exists("player").unwrap());
assert!(!game.exists("missing").unwrap());
game.find("player").unwrap();
assert!(matches!(game.find("missing"), Err(Error::SelectorNotFound(_))));
```

Launch a second fixture with `--duplicate-id` and assert `find("player")` returns `AmbiguousSelector`.

- [ ] **Step 2: Write reflected inspection tests**

Use the fixture's stable type paths and assert:

```rust
let health = game.component_json("player", "bevy_e2e_fixture::Health").unwrap();
assert_eq!(health["current"], 100);

let state = game.resource_json("bevy_e2e_fixture::FixtureState").unwrap();
assert_eq!(state["play_clicked"], false);
```

If the binary module's actual reflected type path differs, pin that exact path once by inspecting BRP registry/query output and use it consistently in the fixture tests and README.

- [ ] **Step 3: Verify selector/inspection tests fail**

```bash
cargo test --test selectors --test inspection
```

Expected: compile failure for missing methods.

- [ ] **Step 4: Implement selector resolution with `world.query`**

Query only `E2eId` data using `type_name::<E2eId>()`, filter results client-side by `value`, and enforce 0/1/2+ semantics. Do not cache raw entities across calls; resolving again is cheap and avoids stale-entity contracts.

`exists()` returns false only for zero matches; ambiguity is still an error.

- [ ] **Step 5: Implement reflected reads**

After resolving the current entity, call built-in `world.get_components` with `strict: true`. Resources use `world.get_resources`. Preserve BRP remote errors rather than converting them to null/default values.

- [ ] **Step 6: Write and implement wait tests**

Add tests that:

1. `wait_for("player")` succeeds immediately.
2. `wait_for("missing")` times out and names the selector in the error context.
3. `wait_frames(2)` observes the reflected `E2eFrame.frame` increase by at least two.
4. `wait_until` succeeds when a predicate becomes true and returns timeout otherwise.

Implementation uses a short polling interval (~10–25 ms). `wait_frames` polls `E2eFrame`; it never converts frames to milliseconds.

- [ ] **Step 7: Run focused tests**

```bash
cargo test --test selectors --test inspection
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/selector.rs src/inspect.rs src/wait.rs src/game.rs tests/selectors.rs tests/inspection.rs
git commit -m "feat: add selectors inspection and waits"
```

---

### Task 6: Add keyboard, mouse, and selector-based Bevy UI clicks

**Files:**
- Create: `src/input.rs`
- Create: `tests/input.rs`
- Modify: `src/game.rs`
- Modify: `tests/fixtures/minimal_game.rs`

**Interfaces:**
- `Game::key_down(KeyCode)`, `key_up`, `press_key`
- `Game::move_mouse(Vec2)`, `mouse_down(MouseButton)`, `mouse_up`, `click_at`
- `Game::click(id)` for Bevy UI entities only
- Input serialization uses Bevy's actual reflected event/message types rather than hand-maintained JSON field names.

- [ ] **Step 1: Write keyboard behavior tests**

The fixture records whether `KeyCode::Space` was observed pressed and released. Test:

```rust
game.press_key(KeyCode::Space).unwrap();
game.wait_until(Duration::from_secs(2), |game| {
    Ok(game.resource_json(FIXTURE_STATE)?["space_press_count"] == 1)
}).unwrap();
```

Also test explicit `key_down` leaves the key pressed for a frame before `key_up` clears it.

- [ ] **Step 2: Write mouse and UI-click tests**

Test `click("main_menu.play")` and wait for `gameplay.hud` to become present/visible according to the fixture's chosen marker lifecycle. Assert `FixtureState.play_clicked == true`.

Add a negative test targeting `E2eId("player")` when `player` is not a Bevy UI node; return a clear unsupported-target/configuration error rather than clicking an arbitrary coordinate.

- [ ] **Step 3: Verify input tests fail**

```bash
cargo test --test input -- --test-threads=1
```

Expected: compile failure for missing input methods.

- [ ] **Step 4: Implement exact key down/up through built-in BRP messages**

Construct and serialize Bevy `WindowEvent::KeyboardInput(KeyboardInput { ... })` with:

- primary window entity;
- requested `KeyCode`;
- `ButtonState::Pressed` / `Released`;
- `logical_key: Key::Unidentified(NativeKey::Unidentified)`;
- `text: None`;
- `repeat: false`.

Send it with built-in `world.write_message`. Do not directly mutate `ButtonInput<KeyCode>`.

`press_key` is:

```text
key_down
→ wait_frames(1)
→ key_up
→ wait_frames(1)
```

- [ ] **Step 5: Implement mouse primitives**

Resolve the primary window entity through BRP. Serialize Bevy cursor/button window events and send them via `world.write_message`.

For cursor movement, use `brp_extras/move_mouse` if testing shows it is required to keep window cursor state/picking correct on unfocused CI windows; otherwise keep the built-in message path. This is a choice between two already-approved reusable mechanisms, not a new RPC method.

Mouse `click_at` is:

```text
move_mouse
→ mouse_down(Left)
→ wait_frames(1)
→ mouse_up(Left)
→ wait_frames(1)
```

- [ ] **Step 6: Implement `click(id)` using actual Bevy UI geometry**

Resolve the target entity, then read reflected `UiGlobalTransform` and `ComputedNode` plus primary-window scale factor. Calculate the node's center in physical space and convert to the logical coordinate expected by the cursor event/helper.

Reject targets lacking the required UI components.

Never insert or modify `Interaction` directly.

- [ ] **Step 7: Run rendered input tests**

Local desktop:

```bash
cargo test --test input -- --test-threads=1
```

Linux CI equivalent:

```bash
xvfb-run -a cargo test --test input -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/input.rs src/game.rs tests/input.rs tests/fixtures/minimal_game.rs
git commit -m "feat: drive bevy keyboard mouse and ui"
```

---

### Task 7: Add screenshots and diagnostic artifact bundles

**Files:**
- Create: `src/artifacts.rs`
- Create: `tests/artifacts.rs`
- Modify: `src/game.rs`
- Modify: `src/process.rs`
- Modify: `src/error.rs`

**Interfaces:**
- `Game::screenshot(label) -> Result<PathBuf>`
- `Game::capture_artifacts(label) -> Result<PathBuf>`
- crate-private `capture_failure_best_effort(error_text)` used by `run()`.
- Failure directory contains screenshot, world.json, failure.json, stdout.log, stderr.log when each source is available.

- [ ] **Step 1: Write an explicit screenshot test**

`tests/artifacts.rs`:

```rust
#[test]
fn screenshot_writes_a_non_empty_png() {
    let game = launch_fixture();
    let path = game.screenshot("main_menu").unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(bytes.len() > 100);
}
```

Use a per-test temporary artifact root under `target/e2e-test-output/<unique>` so repository tests do not collide.

- [ ] **Step 2: Write artifact-bundle tests**

After `capture_artifacts("checkpoint")`, assert the directory contains:

```text
screenshot.png
world.json
failure.json (only when capturing failure)
stdout.log
stderr.log
```

For explicit non-failure capture, `failure.json` may be omitted; document this distinction in the test.

Parse `world.json` and assert it contains the `player` and `main_menu.play` `E2eId` values.

- [ ] **Step 3: Verify tests fail**

```bash
cargo test --test artifacts -- --test-threads=1
```

Expected: compile failure for missing artifact methods.

- [ ] **Step 4: Implement screenshots through `bevy_brp_extras`**

Create the artifact directory in the parent, canonicalize/absolutize the PNG destination, and request `brp_extras/screenshot` with that path. Poll for the file until `operation_timeout`; validate that it exists and is non-empty.

Do not implement custom GPU readback.

- [ ] **Step 5: Implement the marked-world snapshot**

Use built-in `world.query` for entities carrying `E2eId` with reflected data requested via BRP. Serialize a stable diagnostic JSON object containing:

```json
{
  "frame": 123,
  "entities": [
    {
      "entity": "diagnostic raw id",
      "e2e_id": "player",
      "components": {}
    }
  ]
}
```

Do not attempt an unbounded full-world dump.

- [ ] **Step 6: Persist process output safely**

Expose snapshots of the continuously drained stdout/stderr buffers from `ChildProcess`. Artifact capture writes those bytes even if UTF-8 is lossy; use `String::from_utf8_lossy` rather than failing diagnostics because of one invalid byte.

- [ ] **Step 7: Implement artifact session naming**

If `artifact_label` is provided, sanitize it to a filesystem-safe component. Otherwise generate a session component from binary stem + parent PID + process-local atomic counter. Do not attempt to infer the Rust test function name.

- [ ] **Step 8: Run artifact tests**

```bash
cargo test --test artifacts -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/artifacts.rs src/game.rs src/process.rs src/error.rs tests/artifacts.rs
git commit -m "feat: capture bevy e2e diagnostics"
```

---

### Task 8: Guarantee failure diagnostics for returned errors and panics

**Files:**
- Create: `tests/failure_harness.rs`
- Modify: `src/lib.rs`
- Modify: `src/game.rs`
- Modify: `src/artifacts.rs`

**Interfaces:**
- `run()` captures diagnostics before teardown for both closure `Err` and panic.
- Cleanup failure never replaces the original error/panic.
- Panic payload is resumed with `resume_unwind` after cleanup.

- [ ] **Step 1: Add ignored helper tests that intentionally fail inside `run()`**

In `tests/failure_harness.rs`, define ignored helpers selected by exact test filter:

```rust
#[test]
#[ignore]
fn helper_returns_error() {
    let result = bevy_e2e::run(fixture_options("returned-error"), |_game| {
        Err(bevy_e2e::Error::Configuration("intentional returned error".into()))
    });
    result.unwrap();
}

#[test]
#[ignore]
fn helper_panics() {
    bevy_e2e::run(fixture_options("panic"), |_game| -> bevy_e2e::Result<()> {
        panic!("intentional panic");
    }).unwrap();
}
```

The outer non-ignored harness tests spawn the current test binary with `--ignored --exact <helper-name>`, expect non-zero exit, then inspect the dedicated artifact root.

- [ ] **Step 2: Verify the harness fails before wiring automatic capture**

```bash
cargo test --test failure_harness
```

Expected: outer harness fails because the expected failure bundle is absent/incomplete.

- [ ] **Step 3: Complete `run()` failure capture**

For returned `Err`:

```text
format original error
→ game.capture_failure_best_effort
→ game.shutdown best effort
→ return the original Error value unchanged
```

For panic:

```text
catch payload
→ render a diagnostic message if payload is &str/String
→ capture failure best effort
→ shutdown best effort
→ resume_unwind(original_payload)
```

`failure.json` contains at least:

```json
{
  "error": "...",
  "last_operation": "...",
  "pid": 12345,
  "elapsed_ms": 456
}
```

Track `last_operation` on the `Game`/client boundary before each high-level BRP action. It is diagnostic metadata only.

- [ ] **Step 4: Assert child cleanup in the failure harness**

Have the helper write its child PID into the artifact metadata. The outer harness verifies that PID is no longer alive after helper exit. Use a small cross-platform process check in Rust rather than a Unix-only shell command inside the Rust suite.

- [ ] **Step 5: Run the failure harness and normal suite**

```bash
cargo test --test failure_harness
cargo test
```

Expected: PASS; the intentionally failing helpers are only run by subprocesses controlled by the harness.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/game.rs src/artifacts.rs tests/failure_harness.rs
git commit -m "test: guarantee e2e failure diagnostics"
```

---

### Task 9: Lock CI, package boundary, and consumer documentation

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `scripts/assert_no_fixture_processes.sh`
- Modify: `README.md`
- Modify: `Cargo.toml`

**Interfaces:**
- Linux and Windows run the same rendered behavioral suite.
- Linux uses Xvfb.
- Package validation confirms only reusable crate/docs/license material is published.

- [ ] **Step 1: Add README consumer setup and minimal test**

README must document exactly:

1. Bevy 0.19.x / Rust 1.95+ compatibility.
2. Optional `bevy_e2e` dependency and `e2e` feature.
3. Feature-gated `BevyE2EPlugin` registration.
4. `E2eId::new(...)` usage.
5. reflected component/resource registration.
6. a minimal `bevy_e2e::run(...)` test.
7. `cargo test --features e2e --test e2e` example.
8. failure artifact directory.
9. raw `game.brp(...)` escape hatch.
10. explicit v0.1 deferrals: no headless switch, process pool, assertion DSL, world picking, other-language client, or MCP integration.

Keep README focused on using the crate; do not duplicate the entire design spec.

- [ ] **Step 2: Add package exclusions**

Set Cargo package metadata so `tests/fixtures`, `test_output`, `.github`, and `docs/superpowers/plans` are not unintentionally included in a release archive while source/docs/license required for the crate remain present.

Verify with:

```bash
cargo package --allow-dirty --list
```

Expected: no fixture binary source, generated test output, or CI-only scripts unless Cargo requires a file for package verification.

- [ ] **Step 3: Add Linux/Windows CI**

`.github/workflows/ci.yml` uses a two-OS matrix:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest]
```

Install Rust 1.95.0. Linux installs X11/Xvfb plus Bevy's required native development packages. Run:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test (under xvfb-run -a on Linux for rendered tests)
cargo package
```

Upload `test_output/**` on failure.

Do not add macOS or a Bevy-version matrix.

- [ ] **Step 4: Add the survivor scan**

`scripts/assert_no_fixture_processes.sh` on Linux checks for remaining `bevy-e2e-fixture` processes and exits non-zero if any survive. Windows CI performs the equivalent PowerShell `Get-Process` check inline after tests.

The survivor step must run with `if: always()` so it executes after test failures too.

- [ ] **Step 5: Run the complete local gate**

On macOS/Windows desktop:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo package --allow-dirty
```

On Linux:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
xvfb-run -a cargo test
cargo package --allow-dirty
./scripts/assert_no_fixture_processes.sh
```

Expected: all commands succeed and no fixture process remains.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml README.md .github/workflows/ci.yml scripts/assert_no_fixture_processes.sh
git commit -m "ci: validate bevy e2e on linux and windows"
```

---

## Final Verification Before Marking the Feature PR Ready

- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo clippy --all-targets -- -D warnings`.
- [ ] Run the full rendered test suite locally.
- [ ] Run `cargo package --allow-dirty --list` and inspect the package boundary.
- [ ] Confirm selector tests cover zero, one, and multiple `E2eId` matches.
- [ ] Confirm input tests prove Bevy observed keyboard/mouse/UI behavior rather than direct state mutation.
- [ ] Confirm `wait_frames` reads the child `E2eFrame` resource.
- [ ] Confirm component/resource inspection uses built-in BRP reflection operations.
- [ ] Confirm raw `Game::brp` can invoke at least one unwrapped BRP operation.
- [ ] Confirm panic and returned-`Err` helpers both produce diagnostics and preserve the primary failure.
- [ ] Confirm shutdown and force-kill paths both reap their child.
- [ ] Confirm Linux/Xvfb and Windows CI pass.
- [ ] Confirm survivor checks report zero leaked fixture processes.
- [ ] Confirm no custom BRP method was added unless the implementation PR documents a concrete blocker in built-in BRP/extras and updates the design accordingly.
- [ ] Confirm no deferred scope (pooling, async API, assertion DSL, world picking, gamepad/touch/IME, headless switch, MCP, multi-Bevy support) slipped into the PR.

## Self-review

- **Spec coverage:** Every v0.1 acceptance criterion maps to Tasks 3–9; process isolation and lifecycle are Task 4, selectors/inspection/waits Task 5, input/UI Task 6, artifacts Task 7, failure semantics Task 8, and release gates Task 9.
- **Scope:** The work is one coherent framework release and stays in one implementation PR. No independent product subsystem needs a separate PR.
- **Reuse:** BRP handles transport/reflection/message writes; `bevy_brp_extras` handles its proven screenshot/shutdown/input seams. The plan does not create a second protocol or generic automation server.
- **Type consistency:** `E2eId { value: String }`, `E2eFrame { frame: u64 }`, and `E2eRuntimeInfo { protocol: u32 }` are the canonical runtime types throughout the plan.
- **Lifecycle consistency:** `run()` is the only API that guarantees automatic failure diagnostics; manual `Game::launch()` callers own their lifecycle.
- **Artifact naming:** No step assumes Rust test-name introspection; caller labels or generated session IDs are used.
- **Headless boundary:** No generic headless option is planned.
- **Placeholders:** No TBD/TODO implementation placeholders remain; choices that depend on observed Bevy behavior (built-in cursor message versus existing BRP-extra cursor helper) are explicitly bounded to two existing implementations and cannot expand the RPC surface.
