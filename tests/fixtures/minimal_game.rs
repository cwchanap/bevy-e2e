//! Rendered fixture binary used by bevy_e2e integration tests.
use std::{
    io::{self, Write},
    process, thread,
    time::Duration,
};

use bevy::prelude::*;
use bevy_e2e::{BevyE2EPlugin, E2eId};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let skip_e2e_plugin = args.iter().any(|a| a == "--skip-e2e-plugin");
    let duplicate_id = args.iter().any(|a| a == "--duplicate-id");
    let sleep_forever = args.iter().any(|a| a == "--sleep-forever");
    let exit_after_ready_ms = args.iter().find_map(|a| {
        a.strip_prefix("--exit-after-ready-ms=")
            .and_then(|v| v.parse::<u64>().ok())
    });

    if sleep_forever {
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }

    if exit_after_ready_ms.is_some() {
        println!("fixture stdout before crash");
        let _ = io::stdout().flush();
        eprintln!("fixture stderr before crash");
        let _ = io::stderr().flush();
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy-e2e-fixture".into(),
            resolution: (800, 600).into(),
            ..default()
        }),
        ..default()
    }));

    if !skip_e2e_plugin {
        app.add_plugins(BevyE2EPlugin);
    }

    app.register_type::<Health>()
        .register_type::<FixtureState>()
        .insert_resource(FixtureState::default())
        .insert_resource(FixtureConfig { duplicate_id })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                observe_play_button,
                observe_input,
                observe_cursor,
                spawn_hud_after_play,
            ),
        );

    if let Some(ms) = exit_after_ready_ms {
        app.insert_resource(ExitAfterReady {
            delay: Duration::from_millis(ms),
        })
        .add_systems(Update, exit_after_ready);
    }

    app.run();
}

#[derive(Resource, Clone, Copy)]
struct FixtureConfig {
    duplicate_id: bool,
}

#[derive(Resource, Clone, Copy)]
struct ExitAfterReady {
    delay: Duration,
}

#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
struct Health {
    current: f32,
}

#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
struct FixtureState {
    play_clicked: bool,
    space_press_count: u32,
    key_is_down: bool,
    mouse_left_is_down: bool,
    mouse_press_count: u32,
    cursor_position: Option<Vec2>,
}

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct HudMarker;

fn setup(mut commands: Commands, config: Res<FixtureConfig>) {
    commands.spawn(Camera2d);

    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: px(16),
            ..default()
        })
        .with_children(|parent| {
            parent
                .spawn((
                    Button,
                    PlayButton,
                    E2eId::new("main_menu.play"),
                    Node {
                        width: px(160),
                        height: px(48),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.2, 0.4, 0.8)),
                ))
                .with_children(|button| {
                    button.spawn((Text::new("Play"), TextColor(Color::WHITE)));
                });
        });

    commands.spawn((
        E2eId::new("player"),
        Health { current: 100.0 },
        Name::new("player"),
    ));

    if config.duplicate_id {
        commands.spawn((
            E2eId::new("player"),
            Health { current: 50.0 },
            Name::new("player-duplicate"),
        ));
    }
}

fn observe_play_button(
    interactions: Query<&Interaction, (Changed<Interaction>, With<PlayButton>)>,
    mut state: ResMut<FixtureState>,
) {
    for interaction in &interactions {
        if *interaction == Interaction::Pressed {
            state.play_clicked = true;
        }
    }
}

fn spawn_hud_after_play(
    mut commands: Commands,
    state: Res<FixtureState>,
    hud: Query<Entity, With<HudMarker>>,
) {
    if !state.play_clicked || !hud.is_empty() {
        return;
    }

    commands.spawn((
        HudMarker,
        E2eId::new("gameplay.hud"),
        Text::new("HUD"),
        TextColor(Color::srgb(0.9, 0.9, 0.1)),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}

fn observe_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<FixtureState>,
) {
    if keys.just_pressed(KeyCode::Space) {
        state.space_press_count = state.space_press_count.saturating_add(1);
    }
    state.key_is_down = keys.pressed(KeyCode::Space);

    if mouse.just_pressed(MouseButton::Left) {
        state.mouse_press_count = state.mouse_press_count.saturating_add(1);
    }
    state.mouse_left_is_down = mouse.pressed(MouseButton::Left);
}

fn observe_cursor(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut state: ResMut<FixtureState>,
) {
    if let Ok(window) = windows.single() {
        state.cursor_position = window.cursor_position();
    }
}

fn exit_after_ready(
    time: Res<Time>,
    config: Res<ExitAfterReady>,
    mut exit: MessageWriter<AppExit>,
    // Elapsed time captured on the first Update frame; the exit delay is
    // measured from here, not from app start. Pre-first-frame Bevy/renderer
    // setup (slow on Windows WARP) can exceed the requested delay, so measuring
    // from app start would exit the child before the harness observes
    // readiness -- turning a "mid-test exit" into a launch failure that
    // bypasses failure-artifact capture.
    mut first_update_elapsed: Local<Option<Duration>>,
) {
    let baseline = first_update_elapsed.get_or_insert_with(|| time.elapsed());
    if time.elapsed().saturating_sub(*baseline) >= config.delay {
        // Prefer an explicit process exit so the harness sees code 42 even if
        // message-driven shutdown is delayed by window teardown.
        let _ = exit.write(AppExit::from_code(42));
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        process::exit(42);
    }
}
