# Bevy E2E v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the v0.1 `bevy_e2e` crate: synchronous Rust tests launch a rendered Bevy 0.19.x child process, control it over BRP, interact through keyboard/mouse/UI, inspect reflected ECS state, wait on real child frames, and capture failure diagnostics.

**Architecture:** Publish one crate with `client` and `runtime` Cargo features. The parent side uses Bevy's `BrpRequest` with a synchronous `ureq` client and built-in BRP operations; the child side registers `E2eId` and reuses `bevy_brp_extras` for BRP/HTTP, diagnostics, screenshots, and shutdown. There are no custom BRP methods or framework-owned frame/protocol resources. Rendered tests are serialized because Bevy 0.19 keeps the render-subapp BRP port fixed at 15703.

**Tech Stack:** Rust 2024, Rust 1.95+, Bevy 0.19.1, `bevy_brp_extras` 0.22.3, BRP JSON-RPC/HTTP, `ureq` 3.0.8, Serde/serde_json, thiserror, `image` for screenshot validation, GitHub Actions, Xvfb on Linux.

**Spec:** `docs/superpowers/specs/2026-09-04-bevy-e2e-design.md`

## Global Constraints

- Implement all work in one feature PR; task commits are checkpoints inside that PR, not separate PRs.
- Target Bevy 0.19.x only; do not add multi-version compatibility code.
- Keep one public package: `bevy_e2e`; use Cargo features instead of separate runtime/client crates.
- Default feature is `client`; consumer game binaries use `default-features = false, features = ["runtime"]`.
- Use ordinary synchronous Rust `#[test]`; do not require Tokio or a custom test runner.
- One child process per E2E test; no pooling or reset protocol.
- BRP is the only transport; do not add a second RPC protocol or custom BRP method.
- Use Bevy's `BrpRequest` rather than hand-building the JSON-RPC envelope.
- v0.1 supports one-response BRP calls only; detect/reject `text/event-stream` responses instead of adding an SSE/watch API.
- Bind the main remote-control server to `127.0.0.1` only and require `BEVY_E2E=1` runtime activation.
- `E2eId` is the stable selector. Raw Bevy `Entity` values are diagnostic/transport details only.
- Player-facing input is preferred; ECS convenience APIs remain read-oriented.
- `click(id)` must copy Bevy 0.19's `UiGlobalTransform` + window scale-factor algorithm; do not query `ComputedNode` for the center.
- Use `brp_extras/get_diagnostics.frame_count` for frame waits/readiness; do not add `E2eFrame` or a protocol-version resource.
- Rendered E2E suites run with `--test-threads=1` because Bevy 0.19's render BRP port 15703 is not configurable through `with_port()`.
- Rendered screenshot tests must validate visible/non-uniform pixels, not only PNG headers/file length.
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
│   ├── lib.rs          # feature-gated public exports + run() + cargo_bin!
│   ├── error.rs        # framework error and Result alias
│   ├── options.rs      # E2eLaunchOptions
│   ├── id.rs           # shared E2eId component
│   ├── client.rs       # crate-private synchronous one-response BRP client + unit tests
│   ├── process.rs      # child spawn, pipe draining, exit/kill/reap
│   ├── game.rs         # public client-side Game facade and lifecycle
│   ├── runtime.rs      # runtime-only BevyE2EPlugin
│   ├── selector.rs     # E2eId resolution
│   ├── inspect.rs      # reflected component/resource reads
│   ├── wait.rs         # selector/frame/predicate waits
│   ├── input.rs        # keyboard/mouse/UI-click composition
│   └── artifacts.rs    # screenshot/world/failure/stdout/stderr bundles
├── tests/
│   ├── public_api.rs
│   ├── runtime.rs
│   ├── lifecycle.rs
│   ├── selectors.rs
│   ├── inspection.rs
│   ├── waits.rs
│   ├── input.rs
│   ├── artifacts.rs
│   ├── failure_harness.rs
│   └── fixtures/minimal_game.rs
├── scripts/assert_no_fixture_processes.sh
├── docs/superpowers/specs/2026-09-04-bevy-e2e-design.md
├── docs/superpowers/plans/2026-09-05-bevy-e2e-implementation-plan.md
└── .github/workflows/ci.yml
```

`BrpClient` stays crate-private. Its fake-HTTP tests live in `src/client.rs` under `#[cfg(test)]`; do not add a hidden public `testing` module solely to reach private code from integration tests.

---

### Task 1: Bootstrap the crate, feature split, and shared public types

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/options.rs`
- Create: `src/id.rs`
- Create: `tests/public_api.rs`
- Modify: `README.md`

**Interfaces:**
- Produces shared `E2eId` and client-side `E2eLaunchOptions`, `Error`, `Result`, `cargo_bin!`.
- Produces Cargo features `client`, `runtime`, and repository-only `fixture`.
- Later tasks add `Game`/`run` under `client` and `BevyE2EPlugin` under `runtime`.

- [ ] **Step 1: Create the exact manifest baseline**

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

[features]
default = ["client"]
client = ["dep:ureq"]
runtime = ["dep:bevy_brp_extras"]
fixture = ["runtime"]

[dependencies]
bevy = { version = "0.19.1", default-features = false, features = [
  "bevy_remote",
  "bevy_render",
  "bevy_ui",
  "bevy_window",
  "png",
  "serialize",
] }
bevy_brp_extras = { version = "0.22.3", optional = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
ureq = { version = "3.0.8", features = ["json"], optional = true }

[dev-dependencies]
bevy = { version = "0.19.1", features = ["bevy_remote", "png", "serialize"] }
image = "0.25"
tiny_http = "0.12"

[[bin]]
name = "bevy-e2e-fixture"
path = "tests/fixtures/minimal_game.rs"
required-features = ["fixture"]
```

Do not add a workspace, proc-macro crate, runtime crate, or CLI crate.

- [ ] **Step 2: Write the failing public API test**

Create `tests/public_api.rs`:

```rust
use std::{path::PathBuf, time::Duration};
use bevy_e2e::{E2eId, E2eLaunchOptions};

#[test]
fn shared_id_and_launch_options_are_stable() {
    assert_eq!(E2eId::new("player").value, "player");

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

- [ ] **Step 3: Run the test and verify the missing API fails**

```bash
cargo test --test public_api
```

Expected: compile failure for missing `E2eId` / `E2eLaunchOptions`.

- [ ] **Step 4: Implement `E2eId` exactly once in shared code**

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

Do not duplicate the selector type in client/runtime modules.

- [ ] **Step 5: Implement the initial error/options/export surface**

`src/error.rs` starts with concrete variants used by the first two tasks:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("BRP request `{method}` failed: {message}")]
    Brp { method: String, message: String },
    #[error("BRP watching/SSE response is not supported by v0.1: `{0}`")]
    UnsupportedWatch(String),
    #[error("invalid E2E configuration: {0}")]
    Configuration(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`src/options.rs` keeps fields private and defaults to:

```rust
startup_timeout = Duration::from_secs(10)
operation_timeout = Duration::from_secs(5)
shutdown_timeout = Duration::from_secs(3)
artifact_root = PathBuf::from("test_output")
artifact_label = None
args = Vec::new()
env = Vec::new()
```

Provide builders/accessors named in the public test plus `arg`, `env`, and `artifact_label`.

`src/lib.rs` exports shared `E2eId`; `Error`, `Result`, and `E2eLaunchOptions` are exported under `#[cfg(feature = "client")]`. Define:

```rust
#[macro_export]
macro_rules! cargo_bin {
    ($name:literal) => {
        std::path::PathBuf::from(env!(concat!("CARGO_BIN_EXE_", $name)))
    };
}
```

- [ ] **Step 6: Verify default-client and runtime-only configurations**

```bash
cargo fmt --check
cargo test --test public_api
cargo check --no-default-features --features runtime
```

Expected: all commands PASS. The runtime-only check must not compile `ureq` as an enabled dependency.

- [ ] **Step 7: Commit**

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
- Produces crate-private `BrpClient::new(port: u16, timeout: Duration) -> Self`.
- Produces `BrpClient::request(&self, method: &str, params: Value) -> Result<Value>`.
- Uses `bevy::remote::BrpRequest` for request serialization.
- Rejects `text/event-stream`; v0.1 does not expose a watch-stream API.

- [ ] **Step 1: Add private unit tests in `src/client.rs`**

Under `#[cfg(test)]`, create a `tiny_http::Server` bound to `127.0.0.1:0` and cover these three responses:

```rust
#[test]
fn instant_result_is_returned() {
    let server = TestServer::json(r#"{"jsonrpc":"2.0","id":1,"result":{"ready":true}}"#);
    let client = BrpClient::new(server.port(), Duration::from_secs(1));
    let value = client.request("world.list_resources", serde_json::json!({})).unwrap();
    assert_eq!(value, serde_json::json!({"ready": true}));
    assert_eq!(server.received_json()["method"], "world.list_resources");
}

#[test]
fn remote_error_keeps_method_and_message() {
    let server = TestServer::json(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"missing"}}"#);
    let client = BrpClient::new(server.port(), Duration::from_secs(1));
    let error = client.request("missing.method", serde_json::json!({})).unwrap_err();
    assert!(error.to_string().contains("missing.method"));
    assert!(error.to_string().contains("missing"));
}

#[test]
fn sse_response_is_rejected_explicitly() {
    let server = TestServer::response(
        "text/event-stream",
        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n",
    );
    let client = BrpClient::new(server.port(), Duration::from_secs(1));
    assert!(matches!(
        client.request("world.get_components+watch", serde_json::json!({})),
        Err(Error::UnsupportedWatch(_))
    ));
}
```

`TestServer` stays inside the private test module; do not export testing-only client internals.

- [ ] **Step 2: Verify the unit tests fail**

```bash
cargo test client::tests
```

Expected: compile failure because `BrpClient` is missing.

- [ ] **Step 3: Implement requests with Bevy `BrpRequest`**

The core request construction is:

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

Before `read_json`, inspect `Content-Type`. If it begins with `text/event-stream`, return `Error::UnsupportedWatch(method.to_owned())`.

Parse the JSON-RPC body as `serde_json::Value`; return `result`, map `error.message` to `Error::Brp`, and reject malformed bodies with method/context in the error message.

Do not add retries, connection pools, auth, async runtime, or SSE reader state.

- [ ] **Step 4: Run focused tests**

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

### Task 3: Add the runtime plugin and rendered fixture

**Files:**
- Create: `src/runtime.rs`
- Create: `tests/runtime.rs`
- Create: `tests/fixtures/minimal_game.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces public `BevyE2EPlugin` when feature `runtime` is enabled.
- The plugin is inert unless `BEVY_E2E=1`.
- When active it registers `E2eId` and adds `bevy_brp_extras::BrpExtrasPlugin`.
- It adds no frame resource, protocol resource, increment system, or custom BRP method.

- [ ] **Step 1: Write runtime gate tests**

`tests/runtime.rs`:

```rust
#[test]
fn activation_parser_requires_exact_one() {
    assert!(bevy_e2e::runtime_enabled_for_test(|key| {
        (key == "BEVY_E2E").then(|| "1".to_owned())
    }));
    assert!(!bevy_e2e::runtime_enabled_for_test(|_| None));
    assert!(!bevy_e2e::runtime_enabled_for_test(|_| Some("0".to_owned())));
}
```

Expose the pure parser only as `#[doc(hidden)]` under `cfg(any(test, feature = "fixture"))`; the product API remains `BevyE2EPlugin`/`E2eId`.

- [ ] **Step 2: Verify failure**

```bash
cargo test --features fixture --test runtime -- --test-threads=1
```

Expected: compile failure for missing runtime plugin/parser.

- [ ] **Step 3: Implement the minimal runtime plugin**

`src/runtime.rs`:

```rust
use bevy::prelude::*;
use bevy_brp_extras::BrpExtrasPlugin;
use crate::E2eId;

pub struct BevyE2EPlugin;

impl Plugin for BevyE2EPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("BEVY_E2E").as_deref() != Ok("1") {
            return;
        }

        app.register_type::<E2eId>();
        app.add_plugins(BrpExtrasPlugin);
    }
}
```

Keep activation parsing factored so the test does not mutate process-global environment.

- [ ] **Step 4: Build a deterministic rendered fixture**

`tests/fixtures/minimal_game.rs` must use `DefaultPlugins`, a fixed primary-window size, and visibly contrasting UI. Define reflected state:

```rust
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct Health {
    pub current: u32,
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct FixtureState {
    pub play_clicked: bool,
    pub space_press_count: u32,
    pub space_released: bool,
}
```

Spawn:

- a full-window root UI with a bright non-black background;
- a contrasting button tagged `E2eId::new("main_menu.play")`;
- a gameplay HUD entity tagged `E2eId::new("gameplay.hud")`, spawned only after the button click;
- a non-UI player entity tagged `E2eId::new("player")` with `Health { current: 100 }`;
- a duplicate `player` only when `--duplicate-id` is passed.

Register `Health` and `FixtureState`. Add `BevyE2EPlugin`. Add fixture switches:

```text
--skip-e2e-plugin   do not add BevyE2EPlugin
--sleep-forever     block before App::run for kill/reap testing
```

Record button interaction and `ButtonInput<KeyCode>` observations into `FixtureState` so input tests prove Bevy processed the injected messages.

- [ ] **Step 5: Verify fixture/feature builds**

```bash
cargo build --features fixture --bin bevy-e2e-fixture
cargo test --features fixture --test runtime -- --test-threads=1
cargo check --no-default-features --features runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/runtime.rs src/lib.rs tests/runtime.rs tests/fixtures/minimal_game.rs
git commit -m "feat: add bevy e2e runtime"
```

---

### Task 4: Implement child lifecycle and `run()`

**Files:**
- Create: `src/process.rs`
- Create: `src/game.rs`
- Create: `tests/lifecycle.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`

**Interfaces:**
- Produces `Game::launch(E2eLaunchOptions) -> Result<Game>` and idempotent `Game::shutdown(&mut self) -> Result<()>`.
- Produces `run(options, FnOnce(&mut Game) -> Result<()>) -> Result<()>`.
- Readiness is a successful `brp_extras/get_diagnostics` response; no protocol resource is read.
- The selected port controls only the main BRP server. Rendered tests are serialized because render port 15703 remains shared.

- [ ] **Step 1: Write lifecycle tests without a parallel-launch claim**

`tests/lifecycle.rs`:

```rust
#[test]
fn game_launches_reaches_extras_and_shuts_down() {
    let mut game = Game::launch(fixture_options()).unwrap();
    let diagnostics = game.brp("brp_extras/get_diagnostics", serde_json::json!({})).unwrap();
    assert!(diagnostics.get("frame_count").is_some());
    assert!(game.is_running());
    game.shutdown().unwrap();
    assert!(!game.is_running());
}
```

Add:

- startup-timeout test with `--skip-e2e-plugin` and a short timeout;
- early-child-exit test using a fixture arg that exits immediately;
- double-shutdown idempotence test.

Do **not** add a two-thread rendered launch test.

- [ ] **Step 2: Verify lifecycle tests fail**

```bash
cargo test --features fixture --test lifecycle -- --test-threads=1
```

Expected: compile failure because `Game` is missing.

- [ ] **Step 3: Implement `ChildProcess`**

`process.rs` must:

1. choose a candidate main port via `TcpListener::bind((Ipv4Addr::LOCALHOST, 0))`, read it, then drop the listener;
2. spawn the requested binary with caller args/env plus framework overrides:

```text
BEVY_E2E=1
BRP_EXTRAS_PORT=<candidate>
```

3. pipe stdout and stderr;
4. immediately drain each pipe on a dedicated reader thread into shared byte buffers;
5. expose `try_wait`, bounded `wait_for_exit`, `kill`, and reap behavior.

Do not claim the selected port changes Bevy's render-subapp port.

- [ ] **Step 4: Implement readiness**

Poll every roughly 25 ms until `startup_timeout`:

```rust
client.request("brp_extras/get_diagnostics", serde_json::json!({}))
```

Any successful JSON result is readiness. `frame_count` may be null during the first samples; readiness does not require a protocol number or a non-null count.

During polling, check `child.try_wait()` and return `ChildExited` immediately if the process ends.

If the child remains alive but readiness times out, shut it down/kill/reap before returning the startup timeout. Port-reselection retry is allowed only when captured process/server diagnostics clearly indicate an address-in-use bind failure; do not triple-retry all startup failures.

- [ ] **Step 5: Implement graceful shutdown and force-kill fallback**

Normal shutdown calls:

```text
brp_extras/shutdown
```

then waits `shutdown_timeout`, kills if still alive, and always reaps. Calling `shutdown()` twice returns success once the child is already gone.

Add a process-level test using `--sleep-forever`; invoke the same kill/reap primitive and assert `try_wait()` reports an exited process afterward.

- [ ] **Step 6: Implement `run()` with panic preservation**

Use:

```rust
let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(&mut game)));
```

Behavior:

```text
Ok(Ok(())) → shutdown → Ok
Ok(Err(e)) → capture_failure_best_effort → shutdown best effort → Err(e)
Err(panic) → capture_failure_best_effort → shutdown best effort → resume_unwind(panic)
```

Until Task 7, `capture_failure_best_effort` is a private no-op; do not expose incomplete artifact APIs.

- [ ] **Step 7: Run focused verification**

```bash
cargo test --features fixture --test lifecycle -- --test-threads=1
cargo fmt --check
```

Expected: PASS.

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
- Create: `tests/waits.rs`
- Modify: `src/game.rs`
- Modify: `src/error.rs`

**Interfaces:**
- `Game::exists`, `find`, `wait_for`, `wait_for_gone`.
- crate-private `resolve_entity(id)` returning only the current BRP entity value.
- `Game::component_json`, `resource_json`.
- `Game::wait_frames`, `wait(Duration)`, `wait_until`.
- Frame waits use `brp_extras/get_diagnostics.frame_count`.

- [ ] **Step 1: Write strict selector tests**

`tests/selectors.rs`:

```rust
assert!(game.exists("player").unwrap());
assert!(!game.exists("missing").unwrap());
game.find("player").unwrap();
assert!(matches!(game.find("missing"), Err(Error::SelectorNotFound(_))));
```

Launch a separate serialized fixture with `--duplicate-id` and assert `find("player")` returns `AmbiguousSelector`.

- [ ] **Step 2: Write reflected inspection tests**

`tests/inspection.rs` asserts the fixture's exact reflected type paths:

```rust
let health = game.component_json("player", "bevy_e2e_fixture::Health").unwrap();
assert_eq!(health["current"], 100);

let state = game.resource_json("bevy_e2e_fixture::FixtureState").unwrap();
assert_eq!(state["play_clicked"], false);
```

If compile-time `type_name::<Health>()` in the fixture proves a different path, pin that exact path once in fixture test constants and README rather than supporting aliases.

- [ ] **Step 3: Write wait tests in their own test target**

`tests/waits.rs` covers:

```rust
#[test]
fn wait_frames_observes_diagnostics_frame_count() {
    let game = launch_fixture();
    let before = diagnostics_frame(&game);
    game.wait_frames(2).unwrap();
    let after = diagnostics_frame(&game);
    assert!(after >= before + 2);
}
```

Also test:

- `wait_for("player")` succeeds;
- `wait_for("missing")` times out with the selector in the error;
- `wait_until` succeeds on a changing condition and times out on a permanently false predicate.

- [ ] **Step 4: Verify all three test targets fail**

```bash
cargo test --features fixture --test selectors --test inspection --test waits -- --test-threads=1
```

Expected: compile failure for missing methods.

- [ ] **Step 5: Implement selector resolution with `world.query`**

Request only the reflected `E2eId` component:

```json
{
  "data": {
    "components": ["bevy_e2e::id::E2eId"],
    "option": [],
    "has": []
  },
  "filter": {
    "with": ["bevy_e2e::id::E2eId"],
    "without": []
  },
  "strict": true
}
```

Filter returned component values client-side by `value`. Enforce zero/one/multiple semantics. Do not cache raw entities across calls.

`exists()` returns false only for zero matches; ambiguity remains an error.

- [ ] **Step 6: Implement reflected reads**

After resolving the current entity, call built-in `world.get_components` with `strict: true`. Resources use `world.get_resources`. Preserve BRP remote errors instead of returning null/default values.

- [ ] **Step 7: Implement waits from observable state**

Selector/predicate waits use a 10–25 ms poll interval until the operation timeout.

`wait_frames(n)` repeatedly calls `brp_extras/get_diagnostics`. First wait until `frame_count` is numeric, then:

```rust
let target = start.saturating_add(frames as f64);
while current < target {
    // sleep short poll interval, re-read diagnostics
}
```

Normalize the returned numeric value to `u64` after verifying it is finite/non-negative. Do not create an app-side frame resource and do not convert frame counts to wall-clock durations.

- [ ] **Step 8: Run focused tests**

```bash
cargo test --features fixture --test selectors --test inspection --test waits -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/selector.rs src/inspect.rs src/wait.rs src/game.rs src/error.rs tests/selectors.rs tests/inspection.rs tests/waits.rs
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
- `Game::key_down`, `key_up`, `press_key`.
- `Game::move_mouse`, `mouse_down`, `mouse_up`, `click_at`.
- `Game::click(id)` for Bevy UI entities only.
- Exact input uses built-in `world.write_message` and the primary window entity.

- [ ] **Step 1: Write keyboard behavior tests**

The fixture records `ButtonInput<KeyCode>` observations. Test:

```rust
game.press_key(KeyCode::Space).unwrap();
game.wait_until(Duration::from_secs(2), |game| {
    Ok(game.resource_json(FIXTURE_STATE)?["space_press_count"] == 1)
}).unwrap();
```

Also test explicit `key_down` remains observable for a child frame before `key_up`, then assert the fixture observed release.

- [ ] **Step 2: Write mouse/UI behavior tests**

Test:

```rust
game.click("main_menu.play").unwrap();
game.wait_for("gameplay.hud").unwrap();
assert_eq!(
    game.resource_json(FIXTURE_STATE).unwrap()["play_clicked"],
    true
);
```

Target `E2eId("player")`, which is not UI, and assert a clear unsupported-UI-target error.

- [ ] **Step 3: Verify input tests fail**

```bash
cargo test --features fixture --test input -- --test-threads=1
```

Expected: compile failure for missing input methods.

- [ ] **Step 4: Resolve the primary window once per operation**

Use `world.query` for `bevy_window::window::Window`. Require exactly one primary fixture window. Capture:

- raw window entity for message payloads;
- `resolution.scale_factor` for UI coordinate conversion.

Do not keep a durable cached entity across unrelated public calls.

- [ ] **Step 5: Implement exact keyboard down/up with typed Bevy values**

Construct:

```rust
let input = KeyboardInput {
    key_code: key,
    logical_key: Key::Unidentified(NativeKey::Unidentified),
    state,
    text: None,
    repeat: false,
    window: window_entity,
};
let event = WindowEvent::KeyboardInput(input);
let value = serde_json::to_value(event)?;
```

Send `value` through built-in `world.write_message` with message type `type_name::<WindowEvent>()` using Bevy's `BrpWriteMessageParams`.

Never mutate `ButtonInput<KeyCode>` directly.

`press_key` is:

```text
key_down
→ wait_frames(1)
→ key_up
→ wait_frames(1)
```

- [ ] **Step 6: Implement mouse primitives using Bevy's documented BRP message shape**

`move_mouse(position)` sends `WindowEvent::CursorMoved` for the primary window. `mouse_down/up` send `WindowEvent::MouseButtonInput` with requested button and `Pressed`/`Released` state.

`click_at` is:

```text
move_mouse
→ mouse_down(Left)
→ wait_frames(1)
→ mouse_up(Left)
→ wait_frames(1)
```

Use the Bevy 0.19 integration example's `BrpWriteMessageParams` value shape; do not invent a framework remote method or mutate `Interaction`.

- [ ] **Step 7: Implement `click(id)` by copying Bevy 0.19's UI-center algorithm**

Resolve the target entity and read only `UiGlobalTransform`. Its reflected `Affine2` is a flat array whose translation entries `[4]` and `[5]` are the UI center in physical pixels.

Then:

```rust
let physical_x = transform[4].as_f64().ok_or(...)?;
let physical_y = transform[5].as_f64().ok_or(...)?;
let logical = Vec2::new(
    (physical_x / scale_factor) as f32,
    (physical_y / scale_factor) as f32,
);
self.click_at(logical)
```

Reject the target if `UiGlobalTransform` is absent. Do **not** query `ComputedNode`; do **not** rederive padding/origin/bounds geometry.

- [ ] **Step 8: Run rendered input verification**

```bash
cargo test --features fixture --test input -- --test-threads=1
```

Linux equivalent:

```bash
xvfb-run -a cargo test --features fixture --test input -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 9: Commit**

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
- `Game::screenshot(label) -> Result<PathBuf>`.
- `Game::capture_artifacts(label) -> Result<PathBuf>`.
- crate-private `capture_failure_best_effort(error_text)` used by `run()`.
- Screenshot call remains an ordinary one-response `brp_extras/screenshot` request; no SSE client change is required.

- [ ] **Step 1: Write a rendered screenshot-content test**

`tests/artifacts.rs` decodes the PNG rather than checking only magic bytes:

```rust
#[test]
fn screenshot_contains_visible_fixture_content() {
    let game = launch_fixture();
    let path = game.screenshot("main-menu").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

    assert!(image.width() > 0 && image.height() > 0);
    let first = image.get_pixel(0, 0).0;
    assert!(image.pixels().any(|pixel| pixel.0 != first));
    assert!(image.pixels().any(|pixel| pixel.0[..3].iter().copied().max().unwrap() > 32));
}
```

The fixture's intentionally contrasting background/button makes non-uniform content deterministic. A uniform/black PNG must fail the rendered gate.

Use a unique artifact root under `target/e2e-test-output` so serialized tests do not reuse output paths across runs.

- [ ] **Step 2: Write artifact-bundle tests**

After `capture_artifacts("checkpoint")`, assert:

```text
screenshot.png
world.json
stdout.log
stderr.log
```

For failure capture, also require `failure.json`.

Parse `world.json` and assert it includes `player` and `main_menu.play` plus a numeric/null diagnostics frame count field.

- [ ] **Step 3: Verify artifact tests fail**

```bash
cargo test --features fixture --test artifacts -- --test-threads=1
```

Expected: compile failure for missing artifact APIs.

- [ ] **Step 4: Implement screenshot as a terminal extras request**

Create the parent artifact directory and absolute PNG destination, then call:

```rust
self.brp(
    "brp_extras/screenshot",
    serde_json::json!({ "path": absolute_path }),
)?;
```

The returned BRP result is the completion signal. Only after it succeeds, verify the file exists and has non-zero length. Do not poll the file as a substitute for protocol completion and do not add SSE parsing.

- [ ] **Step 5: Implement the marked-world snapshot**

Use `world.query` restricted to entities with `E2eId`, requesting `E2eId` plus optional all-reflectable component data. Serialize a stable parent-owned object:

```json
{
  "frame_count": 123,
  "entities": [
    {
      "entity": "diagnostic raw id",
      "e2e_id": "player",
      "components": {}
    }
  ]
}
```

Read `frame_count` from `brp_extras/get_diagnostics`. Do not add `E2eFrame` and do not dump the entire world.

- [ ] **Step 6: Persist continuously drained process output**

Expose snapshots of stdout/stderr buffers from `ChildProcess`. Write them with `String::from_utf8_lossy`; invalid bytes must not cause diagnostic capture itself to fail.

- [ ] **Step 7: Implement artifact session naming and failure metadata**

If `artifact_label` exists, sanitize it to one filesystem-safe component. Otherwise use binary stem + parent PID + process-local atomic counter.

`failure.json` contains at least:

```json
{
  "error": "...",
  "last_operation": "...",
  "pid": 12345,
  "elapsed_ms": 456
}
```

Do not infer the Rust test function name.

- [ ] **Step 8: Run artifact tests**

```bash
cargo test --features fixture --test artifacts -- --test-threads=1
```

Expected: PASS, including decoded visible screenshot content.

- [ ] **Step 9: Commit**

```bash
git add src/artifacts.rs src/game.rs src/process.rs src/error.rs tests/artifacts.rs
git commit -m "feat: capture bevy e2e diagnostics"
```

---

### Task 8: Guarantee automatic diagnostics for returned errors and panics

**Files:**
- Create: `tests/failure_harness.rs`
- Modify: `src/lib.rs`
- Modify: `src/game.rs`
- Modify: `src/artifacts.rs`

**Interfaces:**
- `run()` captures diagnostics before teardown for closure `Err` and panic.
- Cleanup/diagnostic failure never replaces the original failure.
- Panic payload is resumed unchanged with `resume_unwind`.

- [ ] **Step 1: Add ignored helper tests that intentionally fail inside `run()`**

`tests/failure_harness.rs`:

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

The non-ignored harness tests spawn the current test binary with `--ignored --exact <helper>`, expect non-zero exit, then inspect the helper's dedicated artifact root.

- [ ] **Step 2: Verify the harness fails before the hook is fully wired**

```bash
cargo test --features fixture --test failure_harness -- --test-threads=1
```

Expected: outer harness failure because the expected bundle is absent/incomplete.

- [ ] **Step 3: Complete returned-error capture**

Implement:

```text
format original error
→ game.capture_failure_best_effort
→ game.shutdown best effort
→ return the original Error value unchanged
```

Do not wrap the primary error in an artifact/cleanup error.

- [ ] **Step 4: Complete panic capture**

Implement:

```text
catch original payload
→ derive diagnostic text only for &str/String payloads, otherwise "panic"
→ capture failure best effort
→ shutdown best effort
→ resume_unwind(original payload)
```

- [ ] **Step 5: Assert the child PID is gone**

Include the child PID in `failure.json`. The outer harness uses a small cross-platform Rust process-existence check to verify the PID no longer represents a live fixture after helper exit.

- [ ] **Step 6: Run failure and normal suites**

```bash
cargo test --features fixture --test failure_harness -- --test-threads=1
cargo test --features fixture -- --test-threads=1
```

Expected: PASS; intentionally failing helpers run only as subprocesses controlled by the harness.

- [ ] **Step 7: Commit**

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
- Linux and Windows run the same serialized rendered behavioral suite.
- Linux uses Xvfb.
- Screenshot content validation is part of both rendered gates.
- Package/feature checks prove client-only and runtime-only configurations.

- [ ] **Step 1: Document consumer feature setup precisely**

README must include:

```toml
[features]
e2e = ["dep:bevy_e2e"]

[dependencies]
bevy_e2e = {
  version = "0.1",
  optional = true,
  default-features = false,
  features = ["runtime"]
}

[dev-dependencies]
bevy_e2e = "0.1"
```

Also document:

1. Bevy 0.19.x / Rust 1.95+ compatibility.
2. feature-gated `BevyE2EPlugin` registration.
3. `E2eId::new(...)` usage.
4. reflected component/resource registration.
5. a minimal synchronous `bevy_e2e::run(...)` test.
6. rendered invocation with `--test-threads=1` for v0.1.
7. the fixed Bevy render BRP port 15703 reason for serialization.
8. failure artifact directory.
9. raw one-response `game.brp(...)` escape hatch and explicit lack of SSE watch-stream API.
10. v0.1 deferrals: no headless switch, process pool, assertion DSL, world picking, other-language client, or MCP integration.

Keep README consumer-focused; do not duplicate the entire spec.

- [ ] **Step 2: Keep the fixture target non-default without fighting Cargo packaging**

Retain:

```toml
[[bin]]
name = "bevy-e2e-fixture"
path = "tests/fixtures/minimal_game.rs"
required-features = ["fixture"]
```

Do not make `fixture` a default feature. Package exclusions should remove generated output (`test_output`, `target/e2e-test-output`) and CI-only transient files. Do not require fixture source itself to disappear from `cargo package --list` if Cargo target verification needs it; the contract is that ordinary consumers do not build the fixture target.

Verify:

```bash
cargo package --allow-dirty --list
cargo check --no-default-features --features client
cargo check --no-default-features --features runtime
```

Expected: feature configurations compile and no generated artifact directories are packaged.

- [ ] **Step 3: Add serialized Linux/Windows rendered CI**

`.github/workflows/ci.yml` uses:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest]
```

Install Rust 1.95.0. Linux installs Xvfb and Bevy's required native packages.

Run on both OSes:

```text
cargo fmt --check
cargo clippy --all-targets --features fixture -- -D warnings
cargo check --no-default-features --features client
cargo check --no-default-features --features runtime
cargo test --features fixture -- --test-threads=1
cargo package --allow-dirty
```

On Linux wrap the rendered `cargo test` command in `xvfb-run -a`.

Do not run the fixture test harness with default parallel test threads.

- [ ] **Step 4: Make visible screenshots a release gate**

Do not add CI conditionals that skip `tests/artifacts.rs` on Windows or accept uniform/black pixels. The existing screenshot-content test must run on both Linux/Xvfb and Windows.

If Windows hosted rendering cannot create a real visible surface, the CI job should fail and the platform contract must be revisited in the spec; do not weaken the assertion to PNG magic bytes.

- [ ] **Step 5: Add survivor checks**

`scripts/assert_no_fixture_processes.sh` checks for remaining `bevy-e2e-fixture` processes on Linux and exits non-zero when found. Windows CI performs equivalent PowerShell `Get-Process` logic.

Run survivor checks with `if: always()` so they execute after test failures.

- [ ] **Step 6: Upload failure artifacts**

On CI failure upload:

```text
test_output/**
target/e2e-test-output/**
```

Do not upload successful-run artifacts by default.

- [ ] **Step 7: Run the complete local gate**

macOS/Windows desktop:

```bash
cargo fmt --check
cargo clippy --all-targets --features fixture -- -D warnings
cargo check --no-default-features --features client
cargo check --no-default-features --features runtime
cargo test --features fixture -- --test-threads=1
cargo package --allow-dirty
```

Linux:

```bash
cargo fmt --check
cargo clippy --all-targets --features fixture -- -D warnings
cargo check --no-default-features --features client
cargo check --no-default-features --features runtime
xvfb-run -a cargo test --features fixture -- --test-threads=1
cargo package --allow-dirty
./scripts/assert_no_fixture_processes.sh
```

Expected: all commands succeed and no fixture process remains.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml README.md .github/workflows/ci.yml scripts/assert_no_fixture_processes.sh
git commit -m "ci: validate bevy e2e on linux and windows"
```

---

## Final Verification Before Marking the Feature PR Ready

- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --features fixture -- -D warnings` passes.
- [ ] `cargo check --no-default-features --features client` passes.
- [ ] `cargo check --no-default-features --features runtime` passes.
- [ ] Full rendered tests pass with `--test-threads=1`.
- [ ] `cargo package --allow-dirty --list` contains no generated E2E output.
- [ ] `BrpClient` uses Bevy `BrpRequest`, remains crate-private, and unit-tests JSON success/error plus explicit SSE rejection.
- [ ] Selector tests cover zero, one, and multiple `E2eId` matches.
- [ ] Component/resource inspection uses built-in BRP reflection operations.
- [ ] `wait_frames` reads `brp_extras/get_diagnostics.frame_count`; no `E2eFrame`/protocol resource exists.
- [ ] Input tests prove Bevy observed keyboard/mouse/UI behavior rather than direct state mutation.
- [ ] `click(id)` uses only `UiGlobalTransform` translation + primary-window scale factor; no `ComputedNode` geometry path exists.
- [ ] Screenshot request uses `brp_extras/screenshot` as a terminal JSON call and the fixture screenshot test verifies visible/non-uniform pixels.
- [ ] Raw `Game::brp` invokes at least one unwrapped one-response method and rejects an SSE/watch response clearly.
- [ ] Panic and returned-`Err` helpers both produce diagnostics while preserving the primary failure.
- [ ] Shutdown and force-kill paths both reap their child.
- [ ] Linux/Xvfb and Windows rendered CI pass serialized.
- [ ] Survivor checks report zero leaked fixture processes.
- [ ] No custom BRP method was added.
- [ ] No deferred scope (pooling, async API, watch-stream API, assertion DSL, world picking, gamepad/touch/IME, headless switch, MCP, multi-Bevy support) slipped into the PR.

## Self-review

- **Spec coverage:** Tasks 1–9 cover all revised v0.1 acceptance criteria.
- **Review finding 1:** Screenshot SSE premise rejected after source verification: Bevy HTTP emits SSE only for method names containing `+watch`; `brp_extras/screenshot` has no such suffix and returns the first watching-handler result as ordinary JSON. The valid reuse part is adopted: `BrpClient` uses Bevy `BrpRequest`. Long-lived watch streams remain deferred.
- **Review finding 2:** Adopted. `click(id)` copies Bevy 0.19's `UiGlobalTransform` translation + scale-factor path; `ComputedNode` is removed.
- **Review finding 3:** Adopted more aggressively. Both `E2eFrame` and `E2eRuntimeInfo` are removed; extras diagnostics owns readiness/frame count.
- **Review finding 4:** Adopted. One crate now has client/runtime features; runtime-only consumers do not enable `ureq`.
- **Review finding 5:** Adopted. No rendered parallelism claim/test remains; rendered suites are serialized and screenshots validate pixels.
- **Plan gaps:** Fixed: waits have their own test target and verification command; client tests are private unit tests; fixture bin has a non-default required feature; package checks no longer demand impossible fixture-source removal.
- **Scope:** Still one coherent implementation PR. No new platform, transport, test runner, compatibility layer, or MCP work was added.
- **Placeholders:** No TBD/TODO implementation placeholders remain.