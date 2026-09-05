# Bevy E2E Framework Design

**Status:** Approved for implementation  
**Date:** 2026-09-04  
**Repository:** `cwchanap/bevy-e2e`  
**Initial compatibility target:** Bevy 0.19.x  
**Primary language:** Rust

## 1. Summary

`bevy-e2e` is a Rust-first, out-of-process end-to-end testing framework for Bevy games.

An ordinary Rust integration test launches the game's already-built Bevy binary as a separate child process, enables a test-only Bevy plugin in that child, and controls the running game over the Bevy Remote Protocol (BRP) on localhost.

The framework combines two testing styles:

1. **Player-facing interaction first** — keyboard, mouse, Bevy UI clicks, waits, and screenshots.
2. **Selective ECS introspection second** — stable `E2eId` selectors plus read-oriented access to reflected components/resources for deterministic assertions.

The framework does not replace Rust's test harness or assertion macros. It owns child-process lifecycle, BRP client behavior, stable selectors, synchronization, diagnostics, and cleanup.

The v0.1 implementation reuses existing Bevy infrastructure aggressively:

- BRP / `RemotePlugin` / `RemoteHttpPlugin` for transport and reflected ECS operations.
- `bevy_brp_extras` for screenshot, shutdown, and input behavior where it already solves the Bevy-specific details.
- Bevy reflection, UI transforms, and window/input messages instead of parallel object or serialization systems.

One consumer-facing crate is published: `bevy_e2e`.

## 2. Goals

v0.1 must:

1. Launch a real Bevy game binary from an ordinary Rust `#[test]`.
2. Keep Rust's built-in test harness and normal assertions.
3. Provide a synchronous API; no Tokio requirement.
4. Own one child process per test for deterministic isolation.
5. Use BRP over localhost as the transport foundation.
6. Activate the E2E runtime only in explicitly E2E-enabled builds and launches.
7. Provide stable entity selection through `E2eId`, never durable raw `Entity` IDs.
8. Provide synthetic Bevy-level keyboard and mouse interaction.
9. Provide selector-based clicking for Bevy UI entities.
10. Provide read-oriented reflected component/resource inspection.
11. Provide waits based on real child-game state and frames.
12. Provide screenshots and best-effort failure diagnostics.
13. Drain child stdout/stderr while the process runs.
14. Gracefully shut down and reap the child on success, returned errors, and panics, with force-kill fallback.
15. Validate on Linux/Xvfb and Windows CI.
16. Target one Bevy minor line per framework release.
17. Deliver implementation in one feature PR.

## 3. Non-goals

v0.1 will not:

- provide Python, TypeScript, C#, or other language clients;
- add a custom test runner or procedural test macro;
- add a Playwright-style locator/assertion DSL;
- add suite-level process reuse or process pools;
- expose raw `Entity` IDs as stable selectors;
- make arbitrary ECS mutation a first-class convenience API;
- add world-space sprite/mesh picking;
- add gamepad, touch, IME, or text-entry helpers;
- add video, tracing timelines, or screenshot golden comparison;
- add a generic headless-mode switch;
- target exported/mobile/WASM builds;
- bind outside loopback or add remote-host support;
- add CLI, Cargo subcommand, MCP server, or editor UX;
- wrap the entire BRP method surface;
- maintain compatibility shims across multiple Bevy minor versions.

## 4. Product boundary

Rust's normal test tooling owns discovery, `#[test]`, parallel scheduling, assertions, panic reporting, filtering, and the overall test-process exit status.

`bevy-e2e` owns:

- game child-process launch/reap;
- per-test localhost BRP port selection;
- readiness polling;
- synchronous BRP requests;
- `E2eId` selection;
- synthetic input and Bevy UI clicks;
- waits;
- reflected ECS reads;
- screenshots and failure bundles;
- stdout/stderr capture;
- graceful shutdown and kill fallback;
- a raw BRP escape hatch.

The consuming game owns its normal `App`, gameplay systems, the placement of `E2eId` markers, reflection registration for inspected types, and any game-specific fixture/setup logic.

## 5. Architecture

```text
Rust integration test process
└── bevy_e2e::run(...)
    └── Game
        ├── child-process manager
        ├── synchronous BRP client
        ├── E2eId selector facade
        ├── input / wait / inspection facade
        └── failure artifact collector
                    │
                    │ 127.0.0.1 HTTP
                    │ JSON-RPC / BRP
                    ▼
Bevy child process
├── game plugins / systems
└── BevyE2EPlugin
    ├── E2eId reflection registration
    ├── E2E frame/runtime resources
    └── BrpExtrasPlugin
        ├── RemotePlugin / RemoteHttpPlugin
        ├── screenshot
        ├── shutdown
        └── input helpers
                    │
                    ▼
                 ECS World
```

### 5.1 One public crate

v0.1 publishes only `bevy_e2e`. Internal modules may be split by responsibility (`client`, `process`, `runtime`, `selector`, `input`, `inspect`, `wait`, `artifacts`) but are not separately versioned crates.

### 5.2 BRP is the transport

The framework does not invent another RPC protocol. Built-in BRP methods cover component/resource/query/message operations. `Game::brp(method, params)` remains the universal low-level escape hatch.

### 5.3 Reuse `bevy_brp_extras`

`bevy_brp_extras` is an internal implementation dependency where it already provides reliable screenshot, shutdown, and input behavior. The public `bevy_e2e` API does not expose that dependency's types.

v0.1 adds **no custom BRP methods**. Selector resolution, snapshots, and reflection reads are client-side compositions of built-in BRP operations. Exact key/mouse down/up can use built-in `world.write_message`; extras are preferred where they solve window/cursor behavior more robustly.

A custom BRP method should be introduced only in a future change if a concrete behavior cannot be implemented faithfully through built-in BRP or reused extras.

## 6. Consumer integration

The game adds an optional dependency and one feature:

```toml
[features]
e2e = ["dep:bevy_e2e"]

[dependencies]
bevy_e2e = { version = "0.1", optional = true }
```

```rust
fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    #[cfg(feature = "e2e")]
    app.add_plugins(bevy_e2e::BevyE2EPlugin);

    app.run();
}
```

Compiling with the feature is necessary but not sufficient. `BevyE2EPlugin` activates remote control only when the child receives:

```text
BEVY_E2E=1
```

The runtime binds only to `127.0.0.1`. v0.1 adds no authentication: this is a local developer/CI tool protected by compile-time feature gating, explicit launch activation, loopback binding, per-test ports, and child-process lifetime.

## 7. Child launch and lifecycle

Tests launch the already-built Cargo binary rather than nesting `cargo run` inside `cargo test`.

```rust
#[test]
fn play_starts_game() {
    bevy_e2e::run(
        E2eLaunchOptions::new(cargo_bin!("my-game")),
        |game| {
            game.wait_for("main_menu.play")?;
            game.click("main_menu.play")?;
            game.wait_for("gameplay.hud")?;
            assert!(game.exists("player")?);
            Ok(())
        },
    )
    .unwrap();
}
```

Startup is:

```text
choose candidate loopback port
→ set BEVY_E2E=1 and BRP_EXTRAS_PORT=<port>
→ spawn already-built binary
→ immediately drain stdout/stderr
→ poll reflected E2eRuntimeInfo through BRP
→ execute test closure
```

Port selection must be safe for Rust test parallelism and use bounded retry if a candidate becomes unavailable before the child binds.

`bevy_e2e::run()` is the recommended lifecycle boundary because it can capture diagnostics on both returned `Err` and panics:

```text
success → shutdown → wait → kill fallback if needed → reap
Err     → capture diagnostics → shutdown/reap → return original Err
panic   → capture diagnostics → shutdown/reap → resume original panic
```

Cleanup/diagnostic failures never replace the primary failure.

A lower-level `Game::launch()` / `Game::shutdown()` API remains available for unusual cases, but automatic failure diagnostics are guaranteed by `run()`.

## 8. Stable selectors

The framework provides an explicit test ID:

```rust
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct E2eId {
    pub value: String,
}
```

with `E2eId::new("main_menu.play")` convenience construction.

Resolution uses built-in `world.query` for the reflected `E2eId` type and is strict:

```text
0 matches  → NotFound
1 match    → success
2+ matches → AmbiguousSelector
```

Raw `Entity` IDs may appear internally in BRP requests and diagnostic snapshots but never become the durable public identity. `Name` is not the v0.1 stable-selector contract.

## 9. Player input and Bevy UI click

Public input includes:

```rust
game.key_down(KeyCode::KeyW)?;
game.key_up(KeyCode::KeyW)?;
game.press_key(KeyCode::Space)?;

game.move_mouse(Vec2::new(500.0, 300.0))?;
game.mouse_down(MouseButton::Left)?;
game.mouse_up(MouseButton::Left)?;
game.click_at(Vec2::new(500.0, 300.0))?;
game.click("main_menu.play")?;
```

Input is injected at Bevy's event/message layer, not OS automation and not by directly mutating gameplay resources such as `ButtonInput` or UI `Interaction`.

`press_key()` exposes the pressed state to at least one real child-game frame before release.

`click(id)` supports Bevy UI entities only in v0.1:

```text
resolve E2eId
→ read UiGlobalTransform + ComputedNode
→ read window scale factor
→ derive logical screen-space center
→ move cursor
→ press left button
→ wait frame(s)
→ release
```

The framework must not implement selector clicks by inserting `Interaction::Pressed`. World-space picking is deferred; `click_at()` remains available when coordinates are known.

## 10. ECS inspection

Inspection is read-oriented and opt-in through Bevy reflection registration.

```rust
#[derive(Component, Reflect)]
#[reflect(Component)]
struct Health {
    current: f32,
    max: f32,
}

app.register_type::<Health>();
```

The guaranteed APIs are JSON/value-based:

```rust
let health = game.component_json("player", "my_game::Health")?;
let state = game.resource_json("my_game::GameState")?;
```

They use built-in `world.get_components` / `world.get_resources`. Optional typed conveniences may deserialize JSON when the test-side type supplies the required bounds.

The convenient facade does not add `spawn`, `despawn`, `set_component`, or `insert_resource`. A test that genuinely needs mutation can use raw BRP explicitly.

## 11. Waiting and synchronization

Public waits include:

```rust
game.wait_frames(2)?;
game.wait(Duration::from_millis(250))?;
game.wait_for("gameplay.hud")?;
game.wait_for_gone("loading.spinner")?;

game.wait_until(Duration::from_secs(2), |game| {
    let health = game.component_json("player", HEALTH)?;
    Ok(health["current"] == 100)
})?;
```

`wait_frames()` reads a reflected `E2eFrame` resource maintained by `BevyE2EPlugin`; it does not approximate frames with wall-clock sleeps.

Default waits use a framework timeout (initially five seconds unless testing justifies adjustment) and return errors with operation/selector/type context.

No retrying assertion DSL is added.

## 12. Screenshots and artifacts

`game.screenshot("after_play")` reuses `bevy_brp_extras` screenshot capture and writes PNG output under the test artifact root.

On failure, `run()` best-effort captures:

```text
test_output/<label-or-generated-session>/
├── screenshot.png
├── world.json
├── failure.json
├── stdout.log
└── stderr.log
```

`world.json` is a debugging snapshot, not a full arbitrary-world dump. It contains entities carrying `E2eId`, their diagnostic raw entity IDs, reflected data available for those marked entities, and relevant E2E runtime state.

The Rust standard test harness does not reliably expose the current function name to a library call, so artifact paths use an optional caller-provided label or a generated unique session ID. The framework does not fake test-name discovery.

stdout/stderr are drained continuously from process start so a noisy game cannot deadlock on a full OS pipe.

Successful tests create no failure bundle by default; tests may request explicit screenshots/artifacts.

## 13. Cleanup and abnormal exit

Normal cleanup requests graceful shutdown through `bevy_brp_extras`, waits for the process, then force-kills after a bounded timeout and reaps it.

The cleanup path is idempotent.

v0.1 does not promise a cross-platform parent-death watchdog after hard termination of the test process. The framework's own CI must run a survivor scan so leaked fixture processes fail validation. OS process-group/job-object hardening can be added later if real usage demonstrates a need.

## 14. Rendered execution and CI

Rendered desktop execution is the only generic mode guaranteed by v0.1.

There is no `E2eLaunchOptions::headless(true)` because a launcher cannot generically rewrite an arbitrary `App` built with `DefaultPlugins` into a correct headless application. Projects needing simulation-only E2E may provide a dedicated binary and launch that binary normally.

CI release gates:

- Linux rendered fixture tests under Xvfb.
- Windows rendered fixture tests.
- Rust/format/lint/package checks.
- failure-artifact harness.
- survivor scan after E2E execution.

macOS should work for local development but is not a v0.1 release gate.

## 15. Compatibility and packaging

v0.1 targets Bevy 0.19.x and Rust 1.95 or newer as required by that Bevy line.

One `bevy_e2e` release supports one Bevy minor line at a time. Pre-1.0 API cleanup is preferred over compatibility layers.

Expected repository shape:

```text
.
├── Cargo.toml
├── src/
├── tests/
│   └── fixtures/minimal_game.rs
├── scripts/
├── docs/superpowers/
│   ├── specs/
│   └── plans/
├── .github/workflows/ci.yml
├── README.md
└── LICENSE
```

Only the reusable crate is published. Fixture code, plans, test output, and CI-only support are excluded from the published package where practical.

## 16. Public API sketch

```rust
pub struct BevyE2EPlugin;

pub struct E2eId {
    pub value: String,
}

pub struct E2eLaunchOptions { /* binary, args/env, timeouts, artifact options */ }

pub fn run<F>(options: E2eLaunchOptions, test: F) -> Result<()>
where
    F: FnOnce(&mut Game) -> Result<()>;

pub struct Game;

impl Game {
    pub fn launch(options: E2eLaunchOptions) -> Result<Self>;

    pub fn exists(&self, id: &str) -> Result<bool>;
    pub fn find(&self, id: &str) -> Result<()>;
    pub fn wait_for(&self, id: &str) -> Result<()>;
    pub fn wait_for_gone(&self, id: &str) -> Result<()>;

    pub fn key_down(&self, key: KeyCode) -> Result<()>;
    pub fn key_up(&self, key: KeyCode) -> Result<()>;
    pub fn press_key(&self, key: KeyCode) -> Result<()>;

    pub fn move_mouse(&self, position: Vec2) -> Result<()>;
    pub fn mouse_down(&self, button: MouseButton) -> Result<()>;
    pub fn mouse_up(&self, button: MouseButton) -> Result<()>;
    pub fn click_at(&self, position: Vec2) -> Result<()>;
    pub fn click(&self, id: &str) -> Result<()>;

    pub fn wait_frames(&self, frames: u64) -> Result<()>;
    pub fn wait(&self, duration: Duration) -> Result<()>;
    pub fn wait_until<F>(&self, timeout: Duration, predicate: F) -> Result<()>;

    pub fn component_json(&self, id: &str, type_path: &str) -> Result<Value>;
    pub fn resource_json(&self, type_path: &str) -> Result<Value>;

    pub fn screenshot(&self, label: &str) -> Result<PathBuf>;
    pub fn capture_artifacts(&self, label: &str) -> Result<PathBuf>;

    pub fn brp(&self, method: &str, params: Value) -> Result<Value>;
    pub fn shutdown(&mut self) -> Result<()>;
}
```

Exact signatures may move during TDD, but changes must preserve this product boundary and avoid introducing generic abstraction layers without a concrete need.

## 17. Acceptance criteria

v0.1 is complete when a rendered fixture can:

1. launch from a normal Rust `#[test]`;
2. connect on a unique loopback BRP port;
3. resolve `E2eId` with not-found/ambiguous behavior;
4. click an `E2eId`-marked Bevy UI button through synthetic mouse input;
5. press/release keyboard input through Bevy's normal input path;
6. wait for game-observable markers and real child frames;
7. read a reflected component and resource;
8. capture a rendered screenshot;
9. use raw BRP for an unwrapped operation;
10. capture diagnostics on both panic and returned `Err`;
11. gracefully terminate/reap the child;
12. force-kill a deliberately non-cooperative child after timeout;
13. pass the same core suite on Linux/Xvfb and Windows;
14. leave no fixture process alive after CI.

## 18. Rationale and deferred work

The design favors the smallest useful framework:

- BRP instead of a new protocol.
- `bevy_brp_extras` instead of duplicate screenshot/input plumbing.
- one crate instead of client/runtime packages.
- synchronous API instead of Tokio.
- one process per test instead of reset/pooling machinery.
- `E2eId` instead of raw `Entity` or display/debug names.
- player input first, ECS reads second.
- raw BRP instead of wrappers for every remote operation.
- rendered execution instead of a leaky generic headless abstraction.
- one Bevy minor line instead of compatibility shims.

Explicitly deferred: process pools, async API, test macro, locator/assertion DSLs, world-space picking, gamepad/touch/text helpers, visual regression, video/tracing, full-world dumps, macOS release CI, WASM/export/mobile, parent-death watchdog, MCP integration, multi-Bevy support, and separate runtime/client crates.
