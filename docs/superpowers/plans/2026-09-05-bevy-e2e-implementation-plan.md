# Bevy E2E v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the v0.1 `bevy_e2e` package so synchronous Rust tests can launch a rendered Bevy 0.19.x child, drive it over BRP, interact through keyboard/mouse/UI, inspect reflected ECS state, and capture resilient failure diagnostics.

**Architecture:** One package has `client` and `runtime` feature-separated sides. The parent uses Bevy `BrpRequest` with synchronous `ureq`; the child registers `E2eId` and reuses `bevy_brp_extras` for BRP/HTTP, diagnostics, screenshots, shutdown, and cursor movement. Exact held keyboard/mouse primitives dual-write Bevy's typed input message and aggregate `WindowEvent`, matching Bevy winit's forwarding behavior.

**Tech Stack:** Rust 2024, Rust 1.95+, Bevy 0.19.1, `bevy_brp_extras` 0.22.3, BRP JSON-RPC/HTTP, `ureq` 3.x, Serde/serde_json, thiserror, image 0.25 for screenshot verification, GitHub Actions, Xvfb.

**Spec:** `docs/superpowers/specs/2026-09-04-bevy-e2e-design.md`

## Global Constraints

- Keep all v0.1 implementation in one feature PR.
- Target Bevy 0.19.x only; no compatibility shims.
- Publish one package: `bevy_e2e`.
- Default Cargo feature is `client`; game binaries use `default-features = false, features = ["runtime"]`.
- `fixture` is repository-only and enables a rendered Bevy UI profile.
- Use ordinary synchronous `#[test]`; no Tokio/custom runner/proc macro.
- One child process per `run()`; no pooling/reset protocol.
- BRP is the only transport; add no custom BRP methods.
- Use Bevy `BrpRequest`; do not hand-build JSON-RPC envelopes.
- v0.1 is one-response BRP only; reject `text/event-stream`.
- Main BRP binds loopback only and activates only with `BEVY_E2E=1`.
- `E2eId` is stable identity; raw `Entity` is transport/diagnostic only.
- Use `brp_extras/get_diagnostics.frame_count`; no framework frame/protocol resource.
- Input must reach both typed input messages and aggregate `WindowEvent` consumers.
- Reuse `brp_extras/move_mouse` so `Window::cursor_position()` is also updated.
- `click(id)` uses Bevy 0.19.1's `UiGlobalTransform` translation + window scale-factor algorithm.
- Screenshot tests decode pixels and reject uniform/black captures.
- Framework rendered CI is serialized for GPU/compositor stability; this is not a main-BRP protocol requirement.
- Do not silently relaunch a consumer game after startup timeout.
- No generic headless mode.
- Linux/Xvfb and Windows are release gates; macOS CI deferred.

---

## File Structure

```text
.
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── options.rs
│   ├── id.rs
│   ├── client.rs
│   ├── process.rs
│   ├── game.rs
│   ├── runtime.rs
│   ├── selector.rs
│   ├── inspect.rs
│   ├── wait.rs
│   ├── input.rs
│   └── artifacts.rs
├── tests/
│   ├── lifecycle.rs
│   ├── selectors.rs
│   ├── inspection.rs
│   ├── waits.rs
│   ├── input.rs
│   ├── artifacts.rs
│   ├── failure_harness.rs
│   ├── public_api.rs
│   └── fixtures/minimal_game.rs
├── scripts/assert_no_fixture_processes.sh
├── .github/workflows/ci.yml
└── docs/superpowers/
    ├── specs/2026-09-04-bevy-e2e-design.md
    └── plans/2026-09-05-bevy-e2e-implementation-plan.md
```

`BrpClient` and runtime activation helpers stay crate-private. Unit-test them inside their source modules instead of exporting hidden testing APIs.

---

### Task 1: Bootstrap the package, Cargo feature split, and shared API

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/options.rs`
- Create: `src/id.rs`
- Create: `tests/public_api.rs`
- Modify: `README.md`

**Interfaces:**
- Produces `E2eId`.
- Produces client-only `Error`, `Result`, `E2eLaunchOptions`, `cargo_bin!`.
- Cargo features: `client`, `runtime`, `fixture`.

- [ ] **Step 1: Create the package manifest**

Use this starting manifest:

```toml
[package]
name = "bevy_e2e"
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "MIT OR Apache-2.0"
repository = "https://github.com/cwchanap/bevy-e2e"
description = "Out-of-process end-to-end testing for Bevy games"

[features]
default = ["client"]
client = ["dep:ureq"]
runtime = ["dep:bevy_brp_extras"]
fixture = ["runtime", "bevy/ui"]

[dependencies]
bevy = { version = "0.19.1", default-features = false, features = [
  "bevy_remote",
  "serialize",
] }
bevy_brp_extras = { version = "0.22.3", optional = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
ureq = { version = "3", features = ["json"], optional = true }

[dev-dependencies]
image = "0.25"
tiny_http = "0.12"

[[bin]]
name = "bevy-e2e-fixture"
path = "tests/fixtures/minimal_game.rs"
required-features = ["fixture"]
```

Do not add a workspace or extra package.

- [ ] **Step 2: Write the failing shared/public API test**

`tests/public_api.rs`:

```rust
use std::{path::PathBuf, time::Duration};
use bevy_e2e::{E2eId, E2eLaunchOptions};

#[test]
fn shared_id_and_client_options_are_stable() {
    assert_eq!(E2eId::new("player").value, "player");

    let options = E2eLaunchOptions::new("target/debug/game")
        .startup_timeout(Duration::from_secs(3))
        .operation_timeout(Duration::from_secs(4))
        .shutdown_timeout(Duration::from_secs(2))
        .artifact_root("target/e2e");

    assert_eq!(options.binary(), PathBuf::from("target/debug/game").as_path());
    assert_eq!(options.startup_timeout_value(), Duration::from_secs(3));
    assert_eq!(options.operation_timeout_value(), Duration::from_secs(4));
    assert_eq!(options.shutdown_timeout_value(), Duration::from_secs(2));
}
```

- [ ] **Step 3: Verify the test fails**

```bash
cargo test --test public_api
```

Expected: compile failure for missing crate API.

- [ ] **Step 4: Implement `E2eId`**

`src/id.rs`:

```rust
use bevy::prelude::*;

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

- [ ] **Step 5: Implement initial client-side errors/options**

`src/error.rs` starts with:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("BRP request `{method}` failed: {message}")]
    Brp { method: String, message: String },

    #[error("BRP watch/SSE response is not supported by v0.1: `{0}`")]
    UnsupportedWatch(String),

    #[error("selector `{0}` was not found")]
    SelectorNotFound(String),

    #[error("selector `{0}` matched more than one entity")]
    AmbiguousSelector(String),

    #[error("operation `{operation}` timed out after {timeout:?}")]
    Timeout {
        operation: String,
        timeout: std::time::Duration,
    },

    #[error("child process exited unexpectedly with status {0}")]
    ChildExited(std::process::ExitStatus),

    #[error("artifact operation failed: {0}")]
    Artifact(String),

    #[error("invalid E2E configuration: {0}")]
    Configuration(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`E2eLaunchOptions` defaults:

```text
startup_timeout   = 10s
operation_timeout = 5s
shutdown_timeout  = 3s
artifact_root     = test_output
artifact_label    = None
args              = []
env               = []
```

Add builders/accessors used in `tests/public_api.rs` plus `arg`, `env`, and `artifact_label`.

- [ ] **Step 6: Wire feature-gated exports**

`src/lib.rs`:

```rust
mod id;
pub use id::E2eId;

#[cfg(feature = "client")]
mod error;
#[cfg(feature = "client")]
mod options;

#[cfg(feature = "client")]
pub use error::{Error, Result};
#[cfg(feature = "client")]
pub use options::E2eLaunchOptions;

#[macro_export]
macro_rules! cargo_bin {
    ($name:literal) => {
        std::path::PathBuf::from(env!(concat!("CARGO_BIN_EXE_", $name)))
    };
}
```

Later tasks add `Game`/`run` under `client` and `BevyE2EPlugin` under `runtime`.

- [ ] **Step 7: Verify client-only and runtime-only dependency shapes**

```bash
cargo fmt --check
cargo test --test public_api
cargo check --no-default-features --features runtime
cargo tree --no-default-features --features runtime | grep -v ureq
```

Expected:
- test/check pass;
- `ureq` is absent from runtime-only tree.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml README.md src tests/public_api.rs
git commit -m "feat: bootstrap bevy e2e crate"
```

---

### Task 2: Implement the private synchronous one-response BRP client

**Files:**
- Create: `src/client.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`

**Interfaces:**
- `BrpClient::new(port: u16, timeout: Duration)`.
- `BrpClient::request(method: &str, params: Value) -> Result<Value>`.
- Uses `bevy::remote::BrpRequest`.
- Rejects SSE.

- [ ] **Step 1: Add private fake-HTTP unit tests**

Inside `src/client.rs` under `#[cfg(test)]`, build `tiny_http::Server` on `127.0.0.1:0`.

Cover:

```rust
#[test]
fn returns_instant_result() { /* JSON result */ }

#[test]
fn preserves_remote_method_and_error_message() { /* JSON-RPC error */ }

#[test]
fn rejects_event_stream_response() { /* Content-Type: text/event-stream */ }
```

The success test must inspect the POST body and assert:

```rust
assert_eq!(request["method"], "world.list_resources");
assert!(request.get("id").is_some());
```

Do not export `BrpClient` for tests.

- [ ] **Step 2: Verify failure**

```bash
cargo test client::tests
```

Expected: compile failure for missing `BrpClient`.

- [ ] **Step 3: Implement with `BrpRequest`**

Core request:

```rust
let id = self.next_id.fetch_add(1, Ordering::Relaxed);
let request = BrpRequest {
    method: method.to_owned(),
    id: Some(serde_json::json!(id)),
    params: Some(params),
};

let mut response = ureq::post(&self.url)
    .send_json(&request)
    .map_err(|error| Error::Brp {
        method: method.to_owned(),
        message: error.to_string(),
    })?;
```

Before JSON parsing, inspect response `Content-Type`. If it starts with `text/event-stream`, return `Error::UnsupportedWatch`.

Parse:
- `result` → return it;
- `error.message` → `Error::Brp`;
- malformed body → contextual `Error::Brp`.

No async runtime, retry layer, auth, or SSE reader state.

- [ ] **Step 4: Verify**

```bash
cargo test client::tests
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/client.rs src/error.rs src/lib.rs
git commit -m "feat: add synchronous brp client"
```

---

### Task 3: Add runtime plugin and the rendered fixture

**Files:**
- Create: `src/runtime.rs`
- Create: `tests/fixtures/minimal_game.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Public runtime-only `BevyE2EPlugin`.
- Private `runtime_enabled`.
- Fixture exposes stable IDs/types used by later tests.

- [ ] **Step 1: Write private runtime unit tests first**

Inside `src/runtime.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::runtime_enabled;

    #[test]
    fn activation_requires_exact_one() {
        assert!(runtime_enabled(|key| (key == "BEVY_E2E").then(|| "1".into())));
        assert!(!runtime_enabled(|_| None));
        assert!(!runtime_enabled(|_| Some("0".into())));
    }
}
```

Do not create `tests/runtime.rs` and do not export a testing-only parser.

- [ ] **Step 2: Verify failure**

```bash
cargo test --no-default-features --features runtime runtime::tests
```

Expected: compile failure because runtime module/plugin does not exist.

- [ ] **Step 3: Implement `BevyE2EPlugin`**

Private activation helper:

```rust
fn runtime_enabled(
    lookup: impl Fn(&str) -> Option<String>,
) -> bool {
    lookup("BEVY_E2E").as_deref() == Some("1")
}
```

Plugin behavior:

```rust
impl Plugin for BevyE2EPlugin {
    fn build(&self, app: &mut App) {
        if !runtime_enabled(|key| std::env::var(key).ok()) {
            return;
        }

        app.register_type::<E2eId>();
        app.add_plugins(bevy_brp_extras::BrpExtrasPlugin);
    }
}
```

Do not add custom resources or remote methods.

- [ ] **Step 4: Build the fixture app**

`tests/fixtures/minimal_game.rs` uses `DefaultPlugins` and creates:

- deterministic primary window size/title;
- UI root;
- button with `E2eId::new("main_menu.play")`;
- HUD entity with `E2eId::new("gameplay.hud")`, spawned or made visible after click;
- player entity with `E2eId::new("player")` + reflected `Health { current: 100 }`;
- reflected `FixtureState` resource:
  - `play_clicked: bool`;
  - `space_press_count: u32`;
  - `key_is_down: bool`;
  - `mouse_left_is_down: bool`;
  - `mouse_press_count: u32`;
  - `cursor_position: Option<Vec2>`;
- systems observing `ButtonInput<KeyCode>`, `ButtonInput<MouseButton>`, window cursor position, and UI `Interaction`;
- `--duplicate-id` adds a second `player`;
- `--skip-e2e-plugin` omits plugin;
- `--sleep-forever` blocks before app start for kill fallback;
- `--exit-after-ready-ms=<n>` prints a known stdout line and stderr line, then exits with code 42 after the app has run for the requested delay.

Register `Health` and `FixtureState`.

- [ ] **Step 5: Prove the rendered fixture feature actually builds**

```bash
cargo build --features fixture --bin bevy-e2e-fixture
```

Expected: PASS with `DefaultPlugins`, winit windowing, UI rendering, default font, and picking available through the `bevy/ui` fixture feature.

If the Bevy 0.19.1 high-level `ui` profile itself fails on a supported host due to a missing platform dependency, fix the CI/native package installation; do not move rendering features back into the base client dependency.

- [ ] **Step 6: Verify runtime unit tests and fixture**

```bash
cargo test --no-default-features --features runtime runtime::tests
cargo build --features fixture --bin bevy-e2e-fixture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/runtime.rs src/lib.rs tests/fixtures/minimal_game.rs
git commit -m "feat: add bevy e2e runtime fixture"
```

---

### Task 4: Implement child process lifecycle and `run()`

**Files:**
- Create: `src/process.rs`
- Create: `src/game.rs`
- Create: `tests/lifecycle.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`

**Interfaces:**
- `Game::launch`.
- `Game::shutdown`.
- `run`.
- `Game::is_running`.
- readiness via `brp_extras/get_diagnostics`.

- [ ] **Step 1: Write basic lifecycle tests**

`tests/lifecycle.rs` includes:

```rust
#[test]
fn launch_reaches_diagnostics_and_shutdown_reaps() {
    let mut game = Game::launch(fixture_options()).unwrap();
    assert!(game.is_running());
    game.shutdown().unwrap();
    assert!(!game.is_running());
}

#[test]
fn missing_runtime_returns_startup_timeout_and_reaps() {
    let options = fixture_options()
        .arg("--skip-e2e-plugin")
        .startup_timeout(Duration::from_millis(500));
    assert!(matches!(Game::launch(options), Err(Error::Timeout { .. })));
}
```

- [ ] **Step 2: Add the concurrency experiment**

One test launches two rendered children in two threads, each with its own main BRP port, and requires both to return `brp_extras/get_diagnostics`.

```rust
#[test]
fn two_children_can_answer_distinct_main_brp_sessions() {
    let a = std::thread::spawn(|| Game::launch(fixture_options()));
    let b = std::thread::spawn(|| Game::launch(fixture_options()));

    let mut a = a.join().unwrap().unwrap();
    let mut b = b.join().unwrap().unwrap();

    assert!(a.brp("brp_extras/get_diagnostics", json!({})).is_ok());
    assert!(b.brp("brp_extras/get_diagnostics", json!({})).is_ok());

    a.shutdown().unwrap();
    b.shutdown().unwrap();
}
```

This test pins observed Bevy 0.19 behavior; do not infer the answer from render port 15703.

The framework's broader rendered suite still runs serialized in CI.

- [ ] **Step 3: Verify lifecycle tests fail**

```bash
cargo test --features fixture --test lifecycle -- --test-threads=1
```

Expected: compile failure for missing `Game`.

- [ ] **Step 4: Implement `ChildProcess`**

Responsibilities:

1. select a main port by temporarily binding `127.0.0.1:0`, reading the port, and dropping the probe listener;
2. spawn the requested binary with caller args/env and framework-owned:
   - `BEVY_E2E=1`;
   - `BRP_EXTRAS_PORT=<port>`;
3. pipe stdout/stderr;
4. immediately drain each pipe on a dedicated thread into shared byte buffers;
5. expose `try_wait`, `wait_for_exit`, `kill`, and `reap`.

No log-message scraping.

- [ ] **Step 5: Implement startup readiness without hidden relaunch**

Poll `brp_extras/get_diagnostics` roughly every 25ms until `startup_timeout`.

Rules:
- first successful JSON result → ready;
- null `frame_count` is acceptable;
- early child exit → `ChildExited`;
- alive child + timeout → cleanly kill/reap and return startup timeout with output context.

Do not automatically restart the game on timeout; repeating consumer startup may repeat side effects.

- [ ] **Step 6: Implement shutdown**

```text
brp_extras/shutdown
→ wait shutdown_timeout
→ kill if still alive
→ reap
```

`shutdown()` is idempotent.

- [ ] **Step 7: Implement `run()` panic preservation**

Use `catch_unwind(AssertUnwindSafe(...))`.

For now call a private no-op `capture_failure_best_effort`; Task 7 fills it in.

- [ ] **Step 8: Add force-kill process coverage**

Use fixture `--sleep-forever`, call the same kill/reap primitive used by shutdown fallback, and assert process exit.

- [ ] **Step 9: Run lifecycle verification**

```bash
cargo test --features fixture --test lifecycle -- --test-threads=1
```

Expected: PASS, including the internal two-child concurrency experiment.

If only the two-child test fails, record the observed Bevy 0.19 failure in the test/README and serialize consumers as a documented limitation; do not invent a render-port workaround in this ticket.

- [ ] **Step 10: Commit**

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
- Create: `tests/waits.rs`
- Modify: `src/game.rs`

**Interfaces:**
- `exists`, `find`.
- private `resolve_entity`.
- `component_json`, `resource_json`.
- `wait_for`, `wait_for_gone`, `wait_frames`, `wait`, `wait_until`.

- [ ] **Step 1: Write strict selector tests**

Cover:
- one match;
- zero match;
- duplicate match using `--duplicate-id`.

`exists("missing")` is false; ambiguity is still an error.

- [ ] **Step 2: Write inspection tests**

Assert:

```rust
let health = game.component_json("player", HEALTH_TYPE).unwrap();
assert_eq!(health["current"], 100);

let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
assert_eq!(state["play_clicked"], false);
```

Pin actual reflected type paths in one shared fixture-test constant module if needed.

- [ ] **Step 3: Write wait tests**

`tests/waits.rs` covers:
- immediate `wait_for("player")`;
- timeout for missing selector;
- `wait_frames(2)` observes diagnostics frame delta >=2;
- `wait_until` success and timeout.

- [ ] **Step 4: Verify failure**

```bash
cargo test --features fixture \
  --test selectors --test inspection --test waits -- --test-threads=1
```

Expected: compile failure for missing methods.

- [ ] **Step 5: Implement selector resolution with `world.query`**

Use `BrpQueryParams`/`ComponentSelector` types where practical.

Query `E2eId`, filter values client-side, enforce 0/1/2+.

Do not cache raw entity IDs.

- [ ] **Step 6: Implement reflected reads**

After resolution:
- `world.get_components` with strict behavior for the requested type;
- `world.get_resources` for resource reads.

Preserve remote errors.

- [ ] **Step 7: Implement waits**

`wait_frames`:
1. read diagnostics frame count;
2. store numeric baseline;
3. poll until `current >= baseline + frames`.

If initial count is null, keep polling within operation timeout until numeric before taking baseline.

`wait_for`/`wait_for_gone`/`wait_until` use 10–25ms polling.

- [ ] **Step 8: Verify**

```bash
cargo test --features fixture \
  --test selectors --test inspection --test waits -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/selector.rs src/inspect.rs src/wait.rs src/game.rs \
  tests/selectors.rs tests/inspection.rs tests/waits.rs
git commit -m "feat: add selectors inspection and waits"
```

---

### Task 6: Add correct keyboard, mouse, and Bevy UI interaction

**Files:**
- Create: `src/input.rs`
- Create: `tests/input.rs`
- Modify: `src/game.rs`
- Modify: `tests/fixtures/minimal_game.rs`

**Interfaces:**
- `key_down`, `key_up`, `press_key`.
- `move_mouse`, `mouse_down`, `mouse_up`, `click_at`.
- `click(id)`.

- [ ] **Step 1: Write input-channel regression tests before implementation**

The fixture's `FixtureState` must let the test prove three independent paths.

Keyboard:

```rust
game.key_down(KeyCode::Space).unwrap();
game.wait_frames(1).unwrap();

let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
assert!(state["key_is_down"].as_bool().unwrap());
assert_eq!(state["space_press_count"], 1);

game.key_up(KeyCode::Space).unwrap();
game.wait_frames(1).unwrap();
assert!(!game.resource_json(FIXTURE_STATE_TYPE).unwrap()["key_is_down"]
    .as_bool().unwrap());
```

Mouse `ButtonInput`:

```rust
game.mouse_down(MouseButton::Left).unwrap();
game.wait_frames(1).unwrap();
assert!(game.resource_json(FIXTURE_STATE_TYPE).unwrap()["mouse_left_is_down"]
    .as_bool().unwrap());

game.mouse_up(MouseButton::Left).unwrap();
game.wait_frames(1).unwrap();
```

UI/picking:

```rust
game.click("main_menu.play").unwrap();
game.wait_for("gameplay.hud").unwrap();
assert!(game.resource_json(FIXTURE_STATE_TYPE).unwrap()["play_clicked"]
    .as_bool().unwrap());
```

- [ ] **Step 2: Pin cursor/window state**

Add a test:

```rust
let pos = Vec2::new(120.0, 80.0);
game.move_mouse(pos).unwrap();
game.wait_frames(1).unwrap();

let state = game.resource_json(FIXTURE_STATE_TYPE).unwrap();
assert_eq!(state["cursor_position"][0], 120.0);
assert_eq!(state["cursor_position"][1], 80.0);
```

This proves reuse of extras preserves `Window::cursor_position()`.

- [ ] **Step 3: Pin `UiGlobalTransform` BRP JSON shape**

Query the button's reflected `UiGlobalTransform` and assert:
- it is an array;
- length >= 6;
- entries `[4]` and `[5]` are numeric.

This locks the Bevy 0.19.1 official remote integration-example shape used by `click(id)`.

- [ ] **Step 4: Verify failure**

```bash
cargo test --features fixture --test input -- --test-threads=1
```

Expected: compile failure for missing input API.

- [ ] **Step 5: Implement a private dual-write helper**

For exact held inputs, one logical event is sent through two built-in BRP `world.write_message` calls:

```text
typed message T
aggregate WindowEvent::from(T)
```

Use Bevy's real serializable message types and `BrpWriteMessageParams`; do not hand-maintain raw JSON field names.

Keyboard `T = KeyboardInput`.

Mouse button `T = MouseButtonInput`.

This mirrors `bevy_winit::forward_bevy_events` and reaches both `ButtonInput` systems and picking/window-event consumers.

- [ ] **Step 6: Implement keyboard primitives**

Construct `KeyboardInput` with:
- requested `KeyCode`;
- `ButtonState`;
- primary window entity;
- `logical_key = Key::Unidentified(NativeKey::Unidentified)`;
- `text = None`;
- `repeat = false`.

`press_key`:

```text
key_down
→ wait_frames(1)
→ key_up
→ wait_frames(1)
```

Do not mutate `ButtonInput` directly.

- [ ] **Step 7: Reuse `brp_extras/move_mouse`**

`move_mouse(pos)` calls:

```text
brp_extras/move_mouse
{ "position": [x, y] }
```

Do not replace this with raw `CursorMoved`: extras already dual-writes the relevant messages and updates the `Window` cursor position.

- [ ] **Step 8: Implement mouse button primitives and `click_at`**

`mouse_down/up` dual-write `MouseButtonInput` + aggregate `WindowEvent`.

`click_at`:

```text
move_mouse
→ mouse_down(Left)
→ wait_frames(1)
→ mouse_up(Left)
→ wait_frames(1)
```

Do not mutate `ButtonInput<MouseButton>` or `Interaction` directly.

- [ ] **Step 9: Implement selector click using Bevy's official UI-center path**

Resolve the target and fetch `UiGlobalTransform`.

Use the pinned response shape:

```rust
let arr = transform.as_array().ok_or_else(...)?;
let physical_x = arr.get(4).and_then(Value::as_f64).ok_or_else(...)?;
let physical_y = arr.get(5).and_then(Value::as_f64).ok_or_else(...)?;
```

Query the primary `Window`, read `resolution.scale_factor`, convert:

```rust
let logical = Vec2::new(
    (physical_x / scale_factor) as f32,
    (physical_y / scale_factor) as f32,
);
```

Then `click_at(logical)`.

Missing `UiGlobalTransform` → clear unsupported-target/configuration error.

Do not query `ComputedNode`.

- [ ] **Step 10: Verify rendered input**

```bash
cargo test --features fixture --test input -- --test-threads=1
```

Linux equivalent:

```bash
xvfb-run -a cargo test --features fixture --test input -- --test-threads=1
```

Expected: PASS for typed keyboard state, typed mouse state, cursor position, and real UI interaction.

- [ ] **Step 11: Commit**

```bash
git add src/input.rs src/game.rs tests/input.rs tests/fixtures/minimal_game.rs
git commit -m "feat: drive bevy keyboard mouse and ui"
```

---

### Task 7: Add screenshots and resilient artifact bundles

**Files:**
- Create: `src/artifacts.rs`
- Create: `tests/artifacts.rs`
- Modify: `src/game.rs`
- Modify: `src/process.rs`
- Modify: `src/error.rs`

**Interfaces:**
- `screenshot(label)`.
- `capture_artifacts(label)`.
- private `capture_failure_best_effort`.

- [ ] **Step 1: Write rendered screenshot-content test**

Capture the fixture menu and decode with `image`:

```rust
let image = image::open(path).unwrap().to_rgb8();
assert!(image.width() > 0 && image.height() > 0);

let first = image.get_pixel(0, 0);
assert!(image.pixels().any(|pixel| pixel != first));

let average = image.pixels()
    .map(|p| (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3)
    .sum::<u32>() as f64
    / f64::from(image.width() * image.height());
assert!(average > 2.0);
```

Use deterministic fixture visuals so non-uniformity is expected.

- [ ] **Step 2: Write explicit artifact test**

`capture_artifacts("checkpoint")` must contain:
- `screenshot.png`;
- `world.json`;
- `stdout.log`;
- `stderr.log`.

Explicit non-failure capture may omit `failure.json`.

- [ ] **Step 3: Verify failure**

```bash
cargo test --features fixture --test artifacts -- --test-threads=1
```

Expected: compile failure for missing artifact APIs.

- [ ] **Step 4: Implement screenshot using terminal `brp_extras/screenshot`**

Parent:
1. creates artifact directory;
2. resolves absolute PNG path;
3. calls `brp_extras/screenshot`;
4. waits for the BRP result;
5. verifies file exists and is non-empty.

Do not add SSE parsing or GPU readback.

- [ ] **Step 5: Implement bounded marked-world snapshot with `ComponentSelector::All`**

Use one built-in `world.query`:
- require/filter `E2eId`;
- request `E2eId`;
- set optional selector to `ComponentSelector::All`;
- `strict = false`.

This is explicitly supported by Bevy 0.19 and returns all reflectable optional component values for the small set of E2E-marked entities.

Wrap:

```json
{
  "frame_count": 123,
  "entities": [ ...query result... ]
}
```

Get frame count from diagnostics.

Do not list/get every unmarked world entity.

- [ ] **Step 6: Persist process output**

Expose snapshots of continuously drained stdout/stderr. Use `String::from_utf8_lossy` for artifact files.

- [ ] **Step 7: Implement session naming**

Caller label → sanitized path component.

No label → binary stem + parent PID + atomic counter.

Do not infer Rust test function names.

- [ ] **Step 8: Implement failure-first write order**

`capture_failure_best_effort(error_text)`:

```text
create directory
→ write failure.json
→ write current stdout.log/stderr.log
→ attempt screenshot
→ attempt world snapshot
```

After child teardown, refresh stdout/stderr so late output is preserved.

Remote failures are appended to diagnostic metadata/logging but do not cause this helper to panic/return over the primary failure.

- [ ] **Step 9: Verify artifacts**

```bash
cargo test --features fixture --test artifacts -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/artifacts.rs src/game.rs src/process.rs src/error.rs tests/artifacts.rs
git commit -m "feat: capture bevy e2e diagnostics"
```

---

### Task 8: Guarantee failure artifacts for Err, panic, and dead child

**Files:**
- Create: `tests/failure_harness.rs`
- Modify: `src/lib.rs`
- Modify: `src/game.rs`
- Modify: `src/artifacts.rs`
- Modify: `tests/fixtures/minimal_game.rs`

**Interfaces:**
- `run()` preserves original `Err`/panic.
- dead-child failure still emits local metadata/output files.

- [ ] **Step 1: Add ignored returned-error and panic helpers**

Inside `tests/failure_harness.rs`:

```rust
#[test]
#[ignore]
fn helper_returns_error() {
    bevy_e2e::run(fixture_options("returned-error"), |_game| {
        Err(bevy_e2e::Error::Configuration("intentional returned error".into()))
    }).unwrap();
}

#[test]
#[ignore]
fn helper_panics() {
    bevy_e2e::run(fixture_options("panic"), |_game| -> bevy_e2e::Result<()> {
        panic!("intentional panic");
    }).unwrap();
}
```

Outer tests spawn the current test binary with `--ignored --exact`.

- [ ] **Step 2: Add a dead-child helper**

Use fixture:

```text
--exit-after-ready-ms=250
```

The fixture must print:
- `fixture stdout before crash`;
- `fixture stderr before crash`;

then exit 42.

Helper:

```rust
#[test]
#[ignore]
fn helper_child_exits_mid_test() {
    let options = fixture_options("dead-child")
        .arg("--exit-after-ready-ms=250");

    bevy_e2e::run(options, |game| {
        std::thread::sleep(Duration::from_millis(400));
        game.exists("player")?;
        Ok(())
    }).unwrap();
}
```

Outer harness expects helper failure and checks:
- `failure.json` exists;
- `stdout.log` contains the stdout marker;
- `stderr.log` contains the stderr marker;
- screenshot/world files are optional because BRP is dead.

- [ ] **Step 3: Verify harness fails before automatic capture is complete**

```bash
cargo test --features fixture --test failure_harness -- --test-threads=1
```

Expected: outer harness failure due missing/incomplete bundles.

- [ ] **Step 4: Complete `run()` failure handling**

Returned `Err`:

```text
preserve Error value
→ capture failure best effort
→ shutdown/reap best effort
→ return original Error
```

Panic:

```text
preserve panic payload
→ capture failure best effort
→ shutdown/reap best effort
→ resume_unwind(original payload)
```

Dead child:
- local failure metadata/output must still be written;
- remote artifact failure must not replace the BRP/child-exit failure.

- [ ] **Step 5: Add PID cleanup assertions**

Failure metadata records child PID. Outer harness verifies it is no longer alive after helper process exits.

Use a small cross-platform Rust helper rather than a Unix-only shell call.

- [ ] **Step 6: Run harness + normal suite**

```bash
cargo test --features fixture --test failure_harness -- --test-threads=1
cargo test --features fixture --tests -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs src/game.rs src/artifacts.rs \
  tests/failure_harness.rs tests/fixtures/minimal_game.rs
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
- Linux/Windows release gates.
- Serialized rendered suite.
- Package/docs explain feature split and concurrency precisely.

- [ ] **Step 1: Document consumer setup**

README must include:

1. Bevy 0.19.x / Rust 1.95+.
2. runtime-only dependency:

```toml
[features]
e2e = ["dep:bevy_e2e"]

[dependencies]
bevy_e2e = {
  version = "0.1",
  optional = true,
  default-features = false,
  features = ["runtime"],
}

[dev-dependencies]
bevy_e2e = "0.1"
```

3. feature-gated `BevyE2EPlugin`.
4. `E2eId`.
5. reflection registration.
6. minimal `run()` test.
7. `cargo test --features e2e --test e2e`.
8. artifact layout.
9. raw one-response `brp()`.
10. unsupported watch/SSE.
11. no generic headless mode.
12. concurrency wording:
    - main BRP uses per-child ports;
    - framework does not promise rendered consumer parallelism;
    - its own CI serializes rendered tests for stability;
    - Task 4's two-child test pins observed Bevy 0.19 behavior.

Do not state fixed render port 15703 alone as a reason consumer tests must be serialized.

- [ ] **Step 2: Set package exclusions conservatively**

Exclude generated/local-only paths such as:

```toml
exclude = [
  ".github/",
  "test_output/",
  "target/",
  "docs/superpowers/plans/",
]
```

Do **not** exclude `tests/fixtures/minimal_game.rs` while the manifest explicitly references it as a `[[bin]]` target.

Verify:

```bash
cargo package --allow-dirty --list
cargo package --allow-dirty
```

Expected: package verifies successfully and non-default fixture target is not built for normal consumers.

- [ ] **Step 3: Add Linux/Windows CI**

Matrix:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest]
```

Install Rust 1.95.

Linux installs native packages required by Bevy's `ui` profile/X11/Xvfb.

Run unit/static gates:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo check --no-default-features --features runtime
cargo package
```

Rendered gate:
- Linux: `xvfb-run -a cargo test --features fixture --tests -- --test-threads=1`
- Windows: `cargo test --features fixture --tests -- --test-threads=1`

The two-child concurrency experiment remains one test inside this serialized harness.

- [ ] **Step 4: Make screenshot visibility a hard gate**

Do not skip the artifact screenshot test on Windows.

If a runner produces uniform/black output, the job fails; diagnose adapter/window visibility rather than weakening the assertion.

- [ ] **Step 5: Add survivor scans**

Linux script rejects remaining `bevy-e2e-fixture` processes.

Windows uses `Get-Process` equivalent.

Run with `if: always()` after rendered tests.

- [ ] **Step 6: Upload failure artifacts**

On CI failure upload `test_output/**` / `target/e2e-test-output/**` where present.

- [ ] **Step 7: Run local final gate**

Desktop:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo test --features fixture --tests -- --test-threads=1
cargo package --allow-dirty
```

Linux rendered form:

```bash
xvfb-run -a cargo test --features fixture --tests -- --test-threads=1
./scripts/assert_no_fixture_processes.sh
```

Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml README.md .github/workflows/ci.yml \
  scripts/assert_no_fixture_processes.sh
git commit -m "ci: validate bevy e2e on linux and windows"
```

---

## Final Verification Before Marking the Feature PR Ready

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --lib`
- [ ] `cargo check --no-default-features --features runtime`
- [ ] `cargo build --features fixture --bin bevy-e2e-fixture`
- [ ] full rendered integration suite passes serialized
- [ ] runtime-only dependency tree excludes `ureq`
- [ ] no custom BRP methods
- [ ] `BrpClient` uses `BrpRequest` and rejects SSE
- [ ] selector zero/one/many tests pass
- [ ] `wait_frames` uses extras diagnostics
- [ ] keyboard tests prove `ButtonInput<KeyCode>`
- [ ] mouse tests prove `ButtonInput<MouseButton>`
- [ ] UI click proves `Interaction`/button behavior
- [ ] cursor test proves `Window::cursor_position()`
- [ ] `UiGlobalTransform` array shape is pinned
- [ ] screenshot test requires visible/non-uniform pixels
- [ ] marked-world query uses `ComponentSelector::All` only for E2E-marked entities
- [ ] returned-Err, panic, and dead-child harnesses all produce expected local diagnostics
- [ ] dead-child bundle does not require screenshot/world
- [ ] shutdown and force-kill reap children
- [ ] two-child main-BRP experiment is recorded by a real test, not inferred from render port
- [ ] rendered CI remains serialized for stability
- [ ] no hidden startup relaunch/log scraping
- [ ] package verifies
- [ ] survivor scans report no leaked fixture processes
- [ ] no deferred scope entered the PR

## Self-review

- **Spec coverage:** Tasks 1–9 cover every revised acceptance criterion.
- **Input correctness:** Task 6 mirrors Bevy winit's dual typed/aggregate message delivery for held input and reuses extras for cursor state.
- **Fixture build:** rendered-only Bevy features live behind `fixture`; base client no longer enables render/UI.
- **Concurrency:** a two-child test replaces the previous inference from fixed render port; CI serialization remains a stability choice.
- **Failure resilience:** Task 8 covers a dead child and only requires local artifacts when BRP is unavailable.
- **Private tests:** activation parser and BRP client remain private unit-tested implementation details.
- **World snapshot:** Bevy 0.19 `ComponentSelector::All` is used explicitly; no invented wildcard or N+1 component loop.
- **UI transform:** the official Bevy 0.19.1 remote integration shape is used and pinned by a fixture test.
- **Startup:** no fragile log scraping and no silent automatic game relaunch.
- **Scope:** one package, one implementation PR, no extra protocol/platform/framework.
