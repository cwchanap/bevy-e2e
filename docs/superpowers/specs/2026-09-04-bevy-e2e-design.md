# Bevy E2E Framework Design

**Status:** Reviewed and approved for implementation  
**Date:** 2026-09-04, revised 2026-09-05  
**Repository:** `cwchanap/bevy-e2e`  
**Initial compatibility target:** Bevy 0.19.x / Rust 1.95+  
**Primary language:** Rust

## 1. Summary

`bevy-e2e` is a Rust-first, out-of-process end-to-end testing framework for Bevy games.

An ordinary Rust `#[test]` launches the game's already-built binary as a child process, enables a test-only `BevyE2EPlugin`, and controls the running game over the Bevy Remote Protocol (BRP) on loopback HTTP.

The framework combines:

1. **Player-facing interaction first** — keyboard, mouse, Bevy UI clicks, waits, and screenshots.
2. **Selective ECS introspection second** — stable `E2eId` selectors plus read-oriented reflected component/resource access for deterministic assertions.

Rust's standard test harness remains responsible for discovery and assertions. `bevy-e2e` owns child lifecycle, BRP access, stable selectors, input composition, waits, diagnostics, and cleanup.

v0.1 reuses Bevy infrastructure instead of creating parallel systems:

- Bevy BRP / `BrpRequest` for JSON-RPC and reflected ECS access.
- `bevy_brp_extras` for HTTP setup, diagnostics, screenshots, shutdown, and cursor movement.
- Bevy's typed input messages plus aggregate `WindowEvent` messages for exact held input.
- Bevy's `UiGlobalTransform` + window scale-factor path for UI click coordinates.

One consumer-facing package is published: `bevy_e2e`.

## 2. Goals

v0.1 must:

1. Launch a real Bevy game binary from an ordinary synchronous Rust `#[test]`.
2. Keep Rust's built-in test harness and normal assertions.
3. Own one child process per `run()` call.
4. Use BRP over `127.0.0.1` as the only transport.
5. Activate the runtime only in explicitly E2E-enabled builds and launches.
6. Provide stable selection through `E2eId`; raw `Entity` IDs remain transport/diagnostic details.
7. Provide keyboard and mouse input that reaches both Bevy's typed input consumers and aggregate window-event consumers.
8. Provide selector-based clicking for Bevy UI entities.
9. Provide read-oriented reflected component/resource inspection.
10. Provide synchronization using real child frame diagnostics and observable conditions.
11. Provide rendered screenshots and best-effort failure bundles.
12. Continuously drain child stdout/stderr.
13. Capture diagnostics for returned errors, panics, and dead-child failures without replacing the primary failure.
14. Gracefully shut down/reap children, with force-kill fallback.
15. Validate rendered behavior on Linux/Xvfb and Windows.
16. Target one Bevy minor line per framework release.
17. Deliver v0.1 implementation in one feature PR.

## 3. Non-goals

v0.1 will not:

- provide non-Rust clients;
- add Tokio, a custom runner, or a procedural test macro;
- add a Playwright-style locator/assertion DSL;
- add process pools or suite-level child reuse;
- expose raw `Entity` IDs as stable selectors;
- make arbitrary ECS mutation a first-class convenience API;
- add world-space mesh/sprite picking;
- add gamepad, touch, IME, or text-entry helpers;
- add video, traces, or golden-image comparison;
- add a generic headless-mode switch;
- target exported/mobile/WASM builds;
- bind outside loopback;
- add auth/TLS for the local test transport;
- add CLI/Cargo subcommand/editor/MCP UX;
- wrap the full BRP method surface;
- expose long-lived `+watch`/SSE subscriptions;
- maintain compatibility shims across Bevy minors.

## 4. Product boundary

Rust's normal test tooling owns discovery, scheduling, `#[test]`, assertions, panic reporting, filtering, and the outer test-process status.

`bevy-e2e` owns:

- child spawn/reap;
- main BRP port selection;
- readiness polling;
- synchronous one-response BRP requests;
- `E2eId` selection;
- input helpers;
- waits;
- reflected ECS reads;
- screenshots and artifact bundles;
- stdout/stderr capture;
- graceful shutdown/kill fallback;
- raw one-response `Game::brp`.

The game owns:

- its normal `App`;
- where `E2eId` markers are placed;
- reflection registration for inspected game types;
- game-specific fixture/setup logic;
- any dedicated simulation-only binary it chooses to expose.

## 5. Architecture

```text
Rust integration test process
└── bevy_e2e::run(...)
    └── Game
        ├── ChildProcess
        ├── synchronous BrpClient
        ├── selector / inspection / wait facade
        ├── input facade
        └── artifact collector
                    │
                    │ 127.0.0.1 HTTP
                    │ JSON-RPC / BRP
                    ▼
Bevy child process
├── game plugins / systems
└── BevyE2EPlugin
    ├── E2eId reflection registration
    └── BrpExtrasPlugin
        ├── RemotePlugin / RemoteHttpPlugin
        ├── diagnostics
        ├── screenshot
        ├── shutdown
        └── mouse cursor helpers
```

v0.1 adds **no custom BRP methods**.

### 5.1 One package, feature-separated sides

One package is published, but parent-only HTTP/process code is not compiled into runtime-only consumers.

```toml
[features]
default = ["client"]
client = ["dep:ureq"]
runtime = ["dep:bevy_brp_extras"]
fixture = ["runtime", "bevy/ui"]
```

The root Bevy dependency starts minimal:

```toml
bevy = {
  version = "0.19.1",
  default-features = false,
  features = ["bevy_remote", "serialize"],
}
```

The repository-only `fixture` feature enables Bevy's supported `ui` profile so the fixture has windowing, UI rendering, fonts, picking, and platform support without forcing those features on client-only consumers.

`bevy_brp_extras` is optional behind `runtime`.

Consumer game:

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
```

Tests use the default `client` side.

Separate runtime/client packages are deferred.

### 5.2 BRP is the only transport

The client uses Bevy's `BrpRequest` instead of hand-maintaining a JSON-RPC envelope.

Built-in BRP covers:

- `world.query`;
- component/resource reads;
- `world.write_message`;
- list/discovery operations;
- raw mutation for tests that explicitly choose the escape hatch.

`Game::brp(method, params)` exposes one-response methods directly.

Long-lived watch streams are not a v0.1 feature. If a response is `text/event-stream`, the client returns a clear unsupported-watch error instead of parsing it as JSON.

### 5.3 Screenshot remains a normal one-response call

`bevy_brp_extras` registers `brp_extras/screenshot` with a watching handler internally because screenshot capture spans frames. The public method name does not contain `+watch`.

Bevy 0.19's HTTP transport emits SSE only for `+watch` request names, so `brp_extras/screenshot` waits for its first completion result and returns ordinary JSON. v0.1 does not add an SSE client merely for screenshots.

## 6. Runtime activation

A game registers:

```rust
#[cfg(feature = "e2e")]
app.add_plugins(bevy_e2e::BevyE2EPlugin);
```

Compilation alone does not activate remote control. `BevyE2EPlugin` only configures its runtime when:

```text
BEVY_E2E=1
```

The parent also sets:

```text
BRP_EXTRAS_PORT=<selected-main-port>
```

The plugin:

1. checks the activation flag;
2. registers `E2eId` for reflection;
3. adds `BrpExtrasPlugin`.

There is no `E2eFrame`, protocol-version resource, or framework readiness resource.

Activation parsing is a private helper tested by a `#[cfg(test)]` unit test in `src/runtime.rs`; it is not exported solely for integration testing.

## 7. Child lifecycle

Tests launch Cargo's already-built binary, not nested `cargo run`.

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

Startup:

```text
select an available main loopback port
→ set BEVY_E2E=1 and BRP_EXTRAS_PORT
→ spawn already-built binary
→ immediately drain stdout/stderr
→ poll brp_extras/get_diagnostics
→ first successful JSON response means ready
→ execute test closure
```

`frame_count` may be null in an early diagnostics sample; readiness only requires a successful response.

v0.1 does **not** silently relaunch a consumer game after startup timeout. Relaunching can repeat save writes or other startup side effects. The framework reports the timeout together with captured child output. Port-race hardening can be added later if real usage shows it is needed.

If the child exits during startup, return `ChildExited` immediately.

### 7.1 One child per run; concurrency is not a protocol promise

Each `run()` owns one child and one selected main BRP port.

Bevy 0.19 also starts a render-subapp BRP server on fixed port `15703` when rendering is enabled. The framework never talks to that port. Its bind occurs in a detached task, so the fixed port alone is not proof that two main BRP sessions cannot run concurrently.

v0.1 therefore:

- does not promise consumer-side rendered parallelism;
- does not prohibit it as a protocol rule;
- includes a two-child test that pins the observed Bevy 0.19 behavior by requiring both main BRP diagnostics endpoints to answer;
- runs the framework's own rendered CI suite with `--test-threads=1` as conservative GPU/compositor stability policy.

### 7.2 `run()` is the failure-diagnostic boundary

```text
success → shutdown → wait/kill fallback → reap
Err     → capture failure best effort → shutdown/reap → return original Err
panic   → capture failure best effort → shutdown/reap → resume original payload
```

Cleanup/diagnostic errors never replace the primary error/panic.

Manual `Game::launch()` remains available, but callers then own lifecycle and explicit artifact capture.

## 8. Stable selectors

```rust
#[derive(Component, Reflect, Clone, Debug, PartialEq, Eq)]
#[reflect(Component)]
pub struct E2eId {
    pub value: String,
}
```

Resolution queries reflected `E2eId` values and filters by `value`.

```text
0 matches  → SelectorNotFound
1 match    → success
2+ matches → AmbiguousSelector
```

Raw entities are resolved fresh per operation rather than cached as durable identity.

`Name` is not the stable selector contract.

## 9. ECS inspection

Game types that tests inspect are reflected and registered normally:

```rust
#[derive(Component, Reflect)]
#[reflect(Component)]
struct Health {
    current: f32,
}

app.register_type::<Health>();
```

Guaranteed JSON APIs:

```rust
game.component_json("player", "my_game::Health")?;
game.resource_json("my_game::GameState")?;
```

Typed deserialization may be a small convenience when cleanly expressible, but raw JSON is the baseline.

Convenience APIs remain read-oriented. Mutation is available only through raw BRP.

## 10. Waiting

Public waits include:

```rust
game.wait_frames(2)?;
game.wait(Duration::from_millis(250))?;
game.wait_for("gameplay.hud")?;
game.wait_for_gone("loading.spinner")?;
game.wait_until(Duration::from_secs(2), |game| { ... })?;
```

`wait_frames` samples `brp_extras/get_diagnostics.frame_count` and waits for the requested delta. It does not approximate frames with wall-clock sleeps.

Selector/predicate waits poll with a short bounded interval and return contextual timeout errors.

No assertion DSL is added.

## 11. Input model

Synthetic input must preserve the two channels Bevy's winit layer normally writes:

1. the **typed message** (`KeyboardInput`, `MouseButtonInput`, `CursorMoved`, etc.), used by input systems such as `ButtonInput`;
2. the aggregate **`bevy_window::WindowEvent`**, used by consumers such as Bevy picking.

Writing only the aggregate event is insufficient for `ButtonInput<KeyCode>` / `ButtonInput<MouseButton>`.

### 11.1 Keyboard

Public API:

```rust
game.key_down(KeyCode::KeyW)?;
game.key_up(KeyCode::KeyW)?;
game.press_key(KeyCode::Space)?;
```

`key_down` / `key_up` use built-in `world.write_message` twice for the same event:

```text
KeyboardInput
WindowEvent::KeyboardInput(same event)
```

No custom RPC method is added.

`press_key` composes the exact primitives:

```text
key_down
→ wait_frames(1)
→ key_up
→ wait_frames(1)
```

`bevy_brp_extras/send_keys` remains available through raw BRP, but its duration-based press/hold/release API cannot represent a key held across arbitrary assertions, so it does not replace the exact primitives.

### 11.2 Mouse

Public API:

```rust
game.move_mouse(pos)?;
game.mouse_down(MouseButton::Left)?;
game.mouse_up(MouseButton::Left)?;
game.click_at(pos)?;
```

`move_mouse` reuses `brp_extras/move_mouse`. That helper already:

- emits typed `MouseMotion` / `CursorMoved` plus aggregate `WindowEvent`;
- tracks simulated cursor position;
- updates the `Window` component so `window.cursor_position()` remains current even when the window is unfocused.

`mouse_down` / `mouse_up` dual-write:

```text
MouseButtonInput
WindowEvent::MouseButtonInput(same event)
```

`click_at` composes:

```text
move_mouse
→ mouse_down(Left)
→ wait_frames(1)
→ mouse_up(Left)
→ wait_frames(1)
```

Input fixture tests pin all three relevant observations:

- `ButtonInput<KeyCode>`;
- `ButtonInput<MouseButton>`;
- Bevy UI/picking interaction.

They also verify `Window::cursor_position()` follows `move_mouse`, because cursor state is part of the reused extras contract.

## 12. Selector-based Bevy UI click

`game.click(id)` is Bevy-UI-only in v0.1.

The coordinate algorithm follows Bevy 0.19.1's official `examples/remote/integration_test.rs`:

1. resolve `E2eId`;
2. read `UiGlobalTransform`;
3. use its reflected `Affine2` translation entries `[4]` and `[5]` as the UI center in physical pixels;
4. read the primary `Window` scale factor;
5. convert physical center to logical coordinates;
6. call `click_at`.

The fixture test pins the actual BRP JSON shape: the transform must deserialize as a flat array with numeric translation slots `[4]` and `[5]`.

Targets without `UiGlobalTransform` fail clearly. The framework never inserts `Interaction::Pressed` directly.

World-space picking is deferred.

## 13. Screenshots and artifacts

`game.screenshot(label)` calls `brp_extras/screenshot` with an absolute/canonical parent-selected PNG path.

The BRP completion result is authoritative. File existence is validated after the result; file polling is not used as a substitute for protocol completion.

Rendered fixture tests decode the PNG and require visible, non-uniform content rather than checking only a PNG magic header.

### 13.1 Marked-world snapshot

`world.json` is bounded to `E2eId` entities.

Use built-in `world.query` with:

- `E2eId` as a required/filter component;
- `ComponentSelector::All` for optional reflected component data;
- `strict: false`.

Bevy 0.19 explicitly supports the `all` component selector, so no per-entity list/get loop or custom serializer is required.

The parent wraps the result with diagnostics frame count and stable metadata. Raw entity IDs are diagnostic only.

### 13.2 Failure-first artifact order

Failure capture must remain useful when BRP is already dead.

On failure:

```text
create artifact directory
→ write failure.json immediately
→ snapshot current stdout/stderr immediately
→ if child is reachable: attempt screenshot + world snapshot
→ shutdown/kill/reap
→ refresh stdout/stderr with final drained tail
```

Expected bundle when the child is healthy:

```text
screenshot.png
world.json
failure.json
stdout.log
stderr.log
```

When the child has crashed/hung, `failure.json`, `stdout.log`, and `stderr.log` are still expected; remote screenshot/world files are best effort.

Successful tests create no failure bundle unless explicitly requested.

## 14. Cleanup

Graceful cleanup calls `brp_extras/shutdown`, waits for `shutdown_timeout`, force-kills if needed, and reaps.

The path is idempotent.

v0.1 does not add a parent-death watchdog. The framework's CI runs survivor checks after tests.

## 15. Rendered execution and CI

Rendered desktop execution is the generic v0.1 path.

There is no generic `headless(true)` option; an external launcher cannot rewrite arbitrary `DefaultPlugins` construction. A project can expose a dedicated simulation binary if needed.

Release gates:

- Linux rendered tests under Xvfb;
- Windows rendered tests;
- client/unit tests;
- format/clippy/package checks;
- failure harness, including dead-child artifacts;
- survivor scan.

The framework's rendered suite is serialized for CI stability, not because `bevy-e2e` claims Bevy main-BRP sessions inherently cannot coexist.

macOS is a local-development target, not a v0.1 release gate.

## 16. Packaging

Expected repository:

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

The fixture binary is behind non-default `fixture`; its source may remain in the crate package because Cargo target metadata must remain internally consistent. Generated output, CI-only artifacts, and planning docs may be excluded where practical.

No second crate, CLI, or proc-macro package is published.

## 17. Public API sketch

```rust
pub struct E2eId {
    pub value: String,
}

#[cfg(feature = "runtime")]
pub struct BevyE2EPlugin;

#[cfg(feature = "client")]
pub struct E2eLaunchOptions { /* binary, args/env, timeouts, artifacts */ }

#[cfg(feature = "client")]
pub fn run<F>(options: E2eLaunchOptions, test: F) -> Result<()>
where
    F: FnOnce(&mut Game) -> Result<()>;

#[cfg(feature = "client")]
pub struct Game;

#[cfg(feature = "client")]
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

## 18. Error model

One framework error type should cover:

- spawn failure;
- BRP transport/remote error;
- unsupported SSE/watch response;
- selector not found/ambiguous;
- unsupported UI-click target;
- wait/startup timeout;
- child exited unexpectedly;
- artifact failure;
- invalid configuration.

Errors include operation/method/selector context. BRP error data is preserved where useful.

## 19. Security boundary

v0.1 is a local developer/CI tool:

- compile-time runtime feature;
- explicit `BEVY_E2E=1`;
- loopback-only server;
- per-child main port;
- child lifetime.

No auth/TLS/remote binding is added.

## 20. Acceptance criteria

The v0.1 fixture suite proves that:

1. `cargo build --features fixture --bin bevy-e2e-fixture` builds a rendered Bevy 0.19.x UI app.
2. A normal Rust test launches the already-built fixture and reaches BRP diagnostics.
3. Runtime-only compilation does not enable parent-only `ureq`.
4. `E2eId` resolution handles zero/one/multiple matches.
5. Reflected component/resource reads work.
6. `wait_frames` advances by diagnostics frame count.
7. `key_down/up` reaches `ButtonInput<KeyCode>` and aggregate window events.
8. `mouse_down/up` reaches `ButtonInput<MouseButton>` and aggregate window events.
9. `move_mouse` updates both input events and `Window::cursor_position()`.
10. `click(id)` drives real Bevy UI interaction.
11. The `UiGlobalTransform` JSON shape used by the click algorithm is pinned by a fixture test.
12. Screenshot capture produces visible/non-uniform rendered content.
13. `world.json` contains only `E2eId`-marked entities plus their reflectable data.
14. Returned `Err` and panic both produce failure bundles and preserve the original failure.
15. A child that exits mid-test still yields `failure.json`, stdout, and stderr even when screenshot/world capture is unavailable.
16. Graceful shutdown and force-kill paths both reap the child.
17. A two-child test records whether distinct main BRP sessions can coexist under Bevy 0.19; the framework does not infer this solely from render port 15703.
18. Linux/Xvfb and Windows rendered gates pass serialized with no leaked fixture processes.

## 21. Deferred work

Deferred until there is demonstrated need:

- process pools;
- async client;
- custom test macro;
- locator/assertion DSL;
- world picking;
- gamepad/touch/IME;
- screenshot golden comparison;
- video/traces;
- long-lived watch/SSE client;
- generic headless mode;
- parent-death watchdog;
- MCP integration;
- multi-Bevy compatibility;
- separate runtime/client crates.

## 22. Design rationale

The design stays intentionally small:

- BRP instead of a new protocol.
- `bevy_brp_extras` instead of duplicate screenshot/shutdown/cursor code.
- Dual typed + aggregate input messages because Bevy consumers genuinely split across both.
- One package with feature separation instead of multiple crates.
- Synchronous API because tests drive a separate process.
- One child per test instead of reset/pooling.
- `E2eId` instead of raw entities or display names.
- Player interaction first; ECS reads only for deterministic assertions.
- Raw BRP escape hatch instead of wrapper proliferation.
- Diagnostics-owned frame count instead of a framework frame resource.
- No hidden game relaunch after timeout, avoiding repeated consumer startup side effects.
- Serialized framework CI as rendering-stability policy, not an unproven protocol limitation.
