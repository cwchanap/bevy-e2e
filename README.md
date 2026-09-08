# bevy_e2e

Out-of-process end-to-end testing for Bevy games.

Launch your already-built game binary from an ordinary Rust `#[test]`, drive it over Bevy Remote Protocol (BRP) on loopback HTTP, select entities with `E2eId`, send keyboard/mouse/UI input, inspect reflected ECS state, and capture failure diagnostics.

## Requirements

- **Bevy 0.19.x**
- **Rust 1.95+**

## Features

| Feature | Default | Purpose |
| --- | --- | --- |
| `client` | yes | Test-runner API (`E2eLaunchOptions`, `Game`, `run`, …) |
| `runtime` | no | In-game `BevyE2EPlugin` (no `ureq`) |
| `fixture` | no | Repository-only rendered fixture binary |

## Consumer setup

### 1. Depend with a runtime-only game feature

Keep the plugin out of release builds that do not enable your `e2e` feature. Use **runtime-only** for the game crate dependency so `ureq` is not pulled into the game binary:

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

### 2. Feature-gate `BevyE2EPlugin`

Compilation alone does not activate remote control. Register the plugin behind your feature flag:

```rust
#[cfg(feature = "e2e")]
app.add_plugins(bevy_e2e::BevyE2EPlugin);
```

`BevyE2EPlugin` only configures itself when the parent sets `BEVY_E2E=1` (the framework always sets this, plus `BRP_EXTRAS_PORT=<selected-main-port>`).

### 3. Mark entities with `E2eId`

```rust
use bevy_e2e::E2eId;

commands.spawn((
    // … your components …
    E2eId::new("main_menu.play"),
));
```

### 4. Register reflected types your tests will read

`BevyE2EPlugin` registers `E2eId`. Register your own reflected components/resources the same way Bevy remote expects:

```rust
app.register_type::<Health>();
```

### 5. Write a minimal `run()` test

```rust
use bevy_e2e::{E2eLaunchOptions, Result, cargo_bin, run};

#[test]
fn play_starts_game() -> Result<()> {
    run(E2eLaunchOptions::new(cargo_bin!("my-game")), |game| {
        game.wait_for("main_menu.play")?;
        game.click("main_menu.play")?;
        game.wait_for("gameplay.hud")?;
        Ok(())
    })
}
```

Build the game under test with your `e2e` feature enabled so `BevyE2EPlugin` is present. Run integration tests with:

```bash
cargo test --features e2e --test e2e
```

`BevyE2EPlugin` owns the BRP HTTP transport (it sets up `bevy_brp_extras` on the port chosen by the harness via `BRP_EXTRAS_PORT`). Do **not** register `RemoteHttpPlugin` or `RemotePlugin` separately in the game — `bevy_brp_extras` ignores `BRP_EXTRAS_PORT` when an HTTP transport is already present, which would make the harness poll the wrong endpoint until startup times out.

## Artifacts

Default artifact root is `test_output/` (override with `E2eLaunchOptions::artifact_root`).

Explicit capture writes under a labeled session directory. `Game::screenshot`
writes only the PNG; `Game::capture_artifacts` additionally writes the world
snapshot and output logs:

```text
test_output/<label>/
  screenshot.png
  world.json          # capture_artifacts only
  stdout.log          # capture_artifacts only
  stderr.log          # capture_artifacts only
```

On `run()` failure the harness best-effort writes:

```text
test_output/<auto-session>/
  failure.json
  stdout.log
  stderr.log
  screenshot.png      # if BRP still reachable
  world.json          # if BRP still reachable
```

`world.json` is bounded to `E2eId`-marked entities. Dead-child failures still expect `failure.json` + stdout/stderr; screenshot/world are optional when BRP is gone.

## Raw BRP

`Game::brp(method, params)` issues a **one-response** JSON-RPC call on the child's selected main loopback port.

Long-lived **watch / SSE** subscriptions are **not supported** in v0.1. If the server returns `text/event-stream`, the client returns an unsupported-watch error instead of parsing a stream.

There is **no generic headless mode**. v0.1 drives a rendered desktop window (Linux CI uses Xvfb). A project that needs a simulation-only binary can expose one itself; the framework does not rewrite `DefaultPlugins`.

## Concurrency

- Main BRP uses **per-child ports** selected by the parent before spawn.
- The framework **does not promise** rendered consumer parallelism (GPU/compositor/windowing may still contend).
- This repository's own CI **serializes** rendered tests (`--test-threads=1`) for stability.
- A two-child lifecycle test pins the **observed Bevy 0.19** behavior: two main BRP diagnostics sessions can answer concurrently. That experiment is a real test, not an inference from Bevy's fixed render-subapp BRP port.

## Local verification (Linux)

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo check --no-default-features --features runtime
xvfb-run -a cargo test --features fixture --tests -- --test-threads=1
cargo package --allow-dirty
./scripts/assert_no_fixture_processes.sh
```

Release CI gates: **Linux/Xvfb** and **Windows**. macOS is local-development only for v0.1.

## License

MIT OR Apache-2.0
