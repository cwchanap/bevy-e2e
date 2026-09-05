# Bevy E2E Framework Design

**Status:** Approved for implementation  
**Date:** 2026-09-04  
**Repository:** `cwchanap/bevy-e2e`  
**Initial compatibility target:** Bevy 0.19.x  
**Primary language:** Rust

## 1. Summary

`bevy-e2e` is a Rust-first, out-of-process end-to-end testing framework for Bevy games.

An ordinary Rust integration test launches an already-built Bevy game binary as a separate child process, enables a test-only Bevy plugin in that child, and controls it over the Bevy Remote Protocol (BRP) on localhost.

The framework combines two testing styles:

1. **Player-facing interaction first** — keyboard, mouse, Bevy UI clicks, waits, and screenshots.
2. **Selective ECS introspection second** — stable `E2eId` selectors plus read-oriented reflected component/resource access for deterministic assertions.

The framework does not replace Rust's test harness or assertion macros. It owns child-process lifecycle, synchronous BRP calls, stable selectors, synchronization, diagnostics, and cleanup.

v0.1 reuses existing Bevy infrastructure aggressively:

- BRP / `RemotePlugin` / `RemoteHttpPlugin` for transport and reflected ECS operations.
- `bevy_brp_extras` 0.22.3 for BRP setup, screenshots, shutdown, and diagnostics.
- Bevy's own documented `UiGlobalTransform` + window-scale-factor click path.
- Bevy `WindowEvent` / `world.write_message` for exact keyboard and mouse down/up.

One consumer-facing crate is published: `bevy_e2e`.

## 2. Goals

v0.1 must:

1. Launch a real Bevy game binary from an ordinary synchronous Rust `#[test]`.
2. Keep Rust's built-in test harness and normal assertion macros.
3. Own one child process per E2E test.
4. Use BRP over localhost as the only remote-control transport.
5. Activate the E2E runtime only in explicitly E2E-enabled builds and launches.
6. Provide stable entity selection through `E2eId`, never durable raw `Entity` IDs.
7. Provide synthetic Bevy-level keyboard and mouse interaction.
8. Provide selector-based clicking for Bevy UI entities.
9. Provide read-oriented reflected component/resource inspection.
10. Provide real child-frame waits without a framework-owned frame counter.
11. Provide rendered screenshots and best-effort failure diagnostics.
12. Drain child stdout/stderr while the child runs.
13. Gracefully shut down and reap the child on success, returned errors, and panics, with force-kill fallback.
14. Validate rendered behavior on Linux/Xvfb and Windows CI.
15. Target one Bevy minor line per framework release.
16. Keep the full v0.1 implementation in one feature PR.

## 3. Non-goals

v0.1 will not:

- provide Python, TypeScript, C#, or other language clients;
- require Tokio or add an async public test API;
- add a custom test runner or procedural test macro;
- add a Playwright-style locator/assertion DSL;
- add suite-level process reuse or process pools;
- promise parallel rendered child processes;
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
- expose long-lived `+watch`/SSE subscriptions;
- maintain compatibility shims across multiple Bevy minor versions.

## 4. Product boundary

Rust's normal test tooling owns discovery, `#[test]`, assertions, panic reporting, filtering, and overall test-process status.

`bevy-e2e` owns:

- game child-process launch/reap;
- per-child main BRP port selection;
- readiness polling;
- synchronous one-response BRP calls;
- `E2eId` selection;
- synthetic input and Bevy UI clicks;
- waits;
- reflected ECS reads;
- screenshots and failure bundles;
- stdout/stderr capture;
- graceful shutdown and kill fallback;
- a raw one-response BRP escape hatch.

The consuming game owns its normal `App`, gameplay systems, placement of `E2eId` markers, reflection registration for inspected game types, and game-specific fixture/setup logic.

## 5. Architecture

```text
Rust integration test process
└── bevy_e2e::run(...)
    └── Game
        ├── child-process manager
        ├── synchronous one-response BRP client
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
    └── BrpExtrasPlugin
        ├── RemotePlugin / RemoteHttpPlugin
        ├── screenshot
        ├── shutdown
        └── diagnostics
                    │
                    ▼
                 ECS World
```

### 5.1 One public crate, feature-separated sides

v0.1 publishes one crate, but does not compile the parent-side HTTP/process implementation into runtime-only consumers unnecessarily.

```toml
[features]
default = ["client"]
client = ["dep:ureq"]
runtime = ["dep:bevy_brp_extras"]
fixture = ["runtime"]
```

- `client` enables `Game`, `run`, process management, HTTP, waits, input facade, and artifacts.
- `runtime` enables `BevyE2EPlugin` and `bevy_brp_extras`.
- `E2eId` is shared public API and is available independent of the two sides.
- `fixture` is repository-only and gates the framework's rendered fixture binary.

This remains one package/version/API surface. Separate runtime/client crates are deferred.

### 5.2 BRP is the transport

The framework does not invent another RPC protocol. Built-in BRP methods cover query/component/resource/message operations. Requests use Bevy's `BrpRequest` type rather than a hand-maintained JSON-RPC envelope.

`Game::brp(method, params)` remains the low-level escape hatch for one-response BRP operations.

Long-lived BRP watch streams are not part of v0.1. The client should detect `text/event-stream` and return a clear unsupported-watch error rather than attempting to parse it as ordinary JSON.

### 5.3 Screenshot does not require SSE support

`bevy_brp_extras` internally registers `brp_extras/screenshot` as a `Watching` handler so it can wait across frames for GPU capture and file publication. However, the HTTP method name is `brp_extras/screenshot`, not a `+watch` method. Bevy 0.19's HTTP transport only switches to an SSE response when the request method contains `+watch`; otherwise it waits for the first handler result and returns ordinary JSON.

Therefore v0.1 does **not** add an SSE client merely to support screenshots. `brp_extras/screenshot` is treated as a terminal one-response call that returns after the PNG is published.

## 6. Consumer integration

A game uses the runtime-only side as an optional dependency:

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
bevy_e2e = { version = "0.1" }
```

The game registers the plugin behind its own feature:

```rust
fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    #[cfg(feature = "e2e")]
    app.add_plugins(bevy_e2e::BevyE2EPlugin);

    app.run();
}
```

Cargo may unify dependency features while running the package's own test graph; the feature split is still valuable because normal runtime-only E2E builds do not require `ureq`/process/artifact code.

Compiling with the feature is necessary but not sufficient. `BevyE2EPlugin` activates remote control only when the process receives:

```text
BEVY_E2E=1
```

The runtime binds only to `127.0.0.1`. v0.1 adds no authentication: this is a local developer/CI tool protected by compile-time runtime gating, explicit launch activation, loopback binding, and child-process lifetime.

## 7. Runtime plugin

When `BEVY_E2E=1`, `BevyE2EPlugin` does only two framework-specific things:

1. register `E2eId` for reflection;
2. add `bevy_brp_extras::BrpExtrasPlugin`, which supplies BRP/HTTP, diagnostics, screenshot, and shutdown.

There is no `E2eFrame`, protocol-version resource, or custom BRP method in v0.1.

Readiness is established by a successful `brp_extras/get_diagnostics` call. That proves the main BRP HTTP server and the extras methods are live without creating a framework protocol/version contract before there are shipped clients to negotiate with.

`wait_frames()` reads the `frame_count` returned by the same diagnostics method. No framework increment system is copied into every consumer game.

## 8. Child launch and lifecycle

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
choose candidate loopback main BRP port
→ set BEVY_E2E=1 and BRP_EXTRAS_PORT=<port>
→ spawn already-built binary
→ immediately drain stdout/stderr
→ poll brp_extras/get_diagnostics until success
→ execute test closure
```

The main BRP port uses a bounded retry if the candidate becomes unavailable between allocation and child bind.

`bevy_e2e::run()` is the recommended lifecycle boundary because it can capture diagnostics for both returned `Err` and panics:

```text
success → shutdown → wait → kill fallback if needed → reap
Err     → capture diagnostics → shutdown/reap → return original Err
panic   → capture diagnostics → shutdown/reap → resume original panic
```

Cleanup/diagnostic failures never replace the primary failure.

A lower-level `Game::launch()` / `Game::shutdown()` API remains available for unusual cases, but automatic failure diagnostics are guaranteed by `run()`.

### 8.1 Rendered parallelism limitation

Bevy 0.19's `RemoteHttpPlugin::with_port()` configures the main BRP port only. When `bevy_render` is enabled, Bevy also starts a render-subapp BRP server on fixed `DEFAULT_RENDER_PORT` 15703, and there is no public render-port setter in 0.19.1.

Therefore v0.1 does **not** promise parallel rendered child processes. The framework's own rendered suites run with:

```text
--test-threads=1
```

Per-child main ports still prevent ordinary 15702 conflicts, but full rendered BRP isolation is not claimed. This can be revisited when Bevy exposes render-port configuration or a concrete need justifies a different transport setup.

## 9. Stable selectors

The framework provides an explicit test ID:

```rust
#[derive(Component, Reflect, Clone, Debug, PartialEq, Eq)]
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

Raw `Entity` IDs may appear internally in BRP calls and diagnostic snapshots but never become durable public identity. `Name` is not the v0.1 stable-selector contract.

## 10. Player input and Bevy UI click

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

Input is injected at Bevy's event/message layer, not OS automation and not by directly mutating `ButtonInput` or UI `Interaction`.

Exact key/mouse down/up uses built-in `world.write_message` with Bevy `WindowEvent` values. `press_key()` exposes the pressed state for at least one observed child frame before release.

### 10.1 `click(id)` follows Bevy's 0.19 integration example

Selector-based click supports Bevy UI entities only:

```text
resolve E2eId
→ query UiGlobalTransform
→ take transform translation as the UI center in physical pixels
→ query primary Window and resolution.scale_factor
→ divide physical center by scale factor to get logical cursor position
→ send WindowEvent::CursorMoved
→ send WindowEvent::MouseButtonInput Pressed
→ wait frame(s)
→ send Released
```

No `ComputedNode` read is required for the click center. The framework should copy the engine's documented BRP integration path rather than maintain a second geometry algorithm.

Targets without `UiGlobalTransform` are rejected as unsupported selector-click targets. The framework never inserts or mutates `Interaction` directly. World-space picking is deferred; `click_at()` remains available when coordinates are known.

## 11. ECS inspection

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

Guaranteed JSON/value APIs:

```rust
let health = game.component_json("player", "my_game::Health")?;
let state = game.resource_json("my_game::GameState")?;
```

They use built-in `world.get_components` / `world.get_resources`. Optional typed conveniences may deserialize JSON when the test-side type supplies the needed bounds.

The convenient facade does not add `spawn`, `despawn`, `set_component`, or `insert_resource`. Mutation remains reachable through raw one-response BRP calls when a test genuinely needs it.

## 12. Waiting and synchronization

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

`wait_frames(n)`:

1. calls `brp_extras/get_diagnostics` until `frame_count` is numeric;
2. records the starting frame;
3. polls until `frame_count >= start + n` or timeout.

It never converts frames to milliseconds.

Default waits use a framework timeout (initially five seconds unless implementation testing justifies adjustment) and return errors with operation/selector/type context.

No retrying assertion DSL is added.

## 13. Screenshots and artifacts

`game.screenshot("after_play")` calls `brp_extras/screenshot` with an absolute output path. The remote call is authoritative: it returns only after capture/encoding/publication completes. The parent then verifies that the file exists and is non-empty.

On failure, `run()` best-effort captures:

```text
test_output/<label-or-generated-session>/
├── screenshot.png
├── world.json
├── failure.json
├── stdout.log
└── stderr.log
```

`world.json` is a debugging snapshot, not an arbitrary full-world dump. It contains entities carrying `E2eId`, diagnostic raw entity IDs, reflected data available for those marked entities, and the current diagnostics frame count when available.

The Rust test harness does not reliably expose the current function name to a library call, so artifact paths use an optional caller-provided label or generated session ID. The framework does not fake test-name discovery.

stdout/stderr are drained continuously from process start so a noisy game cannot deadlock on a full OS pipe.

Successful tests create no failure bundle by default; tests may request explicit screenshots/artifacts.

### 13.1 Screenshot correctness gate

A PNG header/file-size check is insufficient because Bevy documents that hidden/fully occluded rendered windows can produce black screenshots.

The framework fixture uses deliberately visible contrasting UI, and its screenshot test decodes the PNG and asserts visible/non-uniform pixel content. Linux/Xvfb and Windows rendered CI must fail if the produced image is effectively black or uniform; they must not green a rendered gate on PNG magic bytes alone.

## 14. Cleanup and abnormal exit

Normal cleanup requests graceful shutdown through `bevy_brp_extras`, waits for the process, then force-kills after a bounded timeout and reaps it.

The cleanup path is idempotent.

v0.1 does not promise a cross-platform parent-death watchdog after hard termination of the test process. The framework's CI runs a survivor scan so leaked fixture processes fail validation. OS process-group/job-object hardening can be added later if real usage demonstrates a need.

## 15. Rendered execution and CI

Rendered desktop execution is the only generic mode guaranteed by v0.1.

There is no `E2eLaunchOptions::headless(true)` because a launcher cannot generically rewrite an arbitrary `App` built with `DefaultPlugins` into a correct headless application. Projects needing simulation-only E2E may provide a dedicated binary and launch it normally.

Release gates:

- Linux rendered fixture tests under Xvfb, serialized with `--test-threads=1`.
- Windows rendered fixture tests, serialized with `--test-threads=1`.
- Screenshot content validation on both rendered gates.
- `cargo fmt`, clippy, feature-configuration checks, and package checks.
- failure-artifact harness.
- survivor scan after E2E execution.

macOS should work for local development but is not a v0.1 release gate.

Windows GPU/surface failure is a release-gate failure, not a reason to accept a black screenshot. If hosted Windows cannot reliably satisfy the rendered contract, the platform gate must be reconsidered explicitly rather than weakened silently.

## 16. Compatibility and packaging

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

The rendered fixture binary is gated by a non-default `fixture` feature via `required-features = ["fixture"]`, so ordinary consumers do not build it. The package check focuses on preventing generated artifacts/CI-only output from shipping; fixture source may remain in the source package if Cargo target verification requires it.

## 17. Public API sketch

With `client` enabled:

```rust
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

Shared API:

```rust
#[derive(Component, Reflect, Clone, Debug, PartialEq, Eq)]
#[reflect(Component)]
pub struct E2eId {
    pub value: String,
}
```

With `runtime` enabled:

```rust
pub struct BevyE2EPlugin;
```

## 18. Error model

The crate exposes one framework error type and `Result<T>` alias. Errors should clearly report:

- child spawn failure;
- startup timeout;
- BRP HTTP/JSON-RPC failure;
- unsupported streaming/watch response;
- selector not found;
- selector ambiguity;
- selector target not usable as Bevy UI;
- wait timeout;
- screenshot/artifact failure;
- unexpected child exit;
- shutdown timeout.

Raw BRP remote errors preserve method and remote message/context.

## 19. Acceptance criteria

v0.1 is complete when the rendered fixture suite proves that it can:

1. compile client-only and runtime-only feature configurations;
2. launch an already-built Bevy 0.19.x child from a normal Rust `#[test]`;
3. activate BRP only with the E2E runtime gate;
4. connect through a selected loopback main BRP port;
5. resolve zero/one/multiple `E2eId` matches correctly;
6. click an `E2eId`-marked Bevy UI button using the Bevy 0.19 `UiGlobalTransform`/window-scale path;
7. press/release keyboard input through `WindowEvent`/`world.write_message`;
8. read reflected component and resource state;
9. wait for real child frames using `brp_extras/get_diagnostics.frame_count`;
10. capture a rendered screenshot with verified visible/non-uniform content;
11. use raw BRP for at least one unwrapped one-response operation;
12. capture diagnostics for both a returned `Err` and a panic;
13. gracefully shut down/reap a cooperative child;
14. force-kill/reap a non-cooperative child;
15. pass serialized rendered Linux/Xvfb and Windows gates;
16. leave no fixture process alive after the suite;
17. add no custom BRP method.

## 20. Review resolutions

The design review was applied with these decisions:

- **Keep:** one crate, synchronous `#[test]`, BRP only, `E2eId`, read-oriented inspection, no headless mode, one Bevy line, one PR.
- **Adopt:** Bevy `BrpRequest`; Bevy's exact UI-center/click path; client/runtime Cargo features; diagnostics frame count instead of runtime frame/protocol resources; serialized rendered tests; screenshot pixel-content validation; fixture feature gating; private client unit tests.
- **Do not adopt SSE for screenshots:** `brp_extras/screenshot` is not a `+watch` HTTP request. Bevy's HTTP transport returns it as one completed JSON response after the watching handler yields its result. Long-lived SSE/watch APIs remain deferred.

These changes reduce custom framework machinery while making the documented rendered/diagnostic behavior more accurate.