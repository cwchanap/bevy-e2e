use bevy::{
    ecs::entity::Entity,
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput, NativeKey},
        mouse::{MouseButton, MouseButtonInput},
    },
    math::Vec2,
    reflect::TypePath,
    remote::builtin_methods::{
        BRP_QUERY_METHOD, BRP_WRITE_MESSAGE_METHOD, BrpQuery, BrpQueryFilter, BrpQueryParams,
        BrpQueryRow, BrpWriteMessageParams,
    },
    window::{PrimaryWindow, Window, WindowEvent},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{Error, Result, game::Game};

const MOVE_MOUSE_METHOD: &str = "brp_extras/move_mouse";
/// Bevy 0.19.1 reflected type path for `UiGlobalTransform` (pinned; avoids base `bevy_ui`).
const UI_GLOBAL_TRANSFORM_TYPE_PATH: &str = "bevy_ui::ui_transform::UiGlobalTransform";

impl Game {
    /// Press and hold a key (exact held input via dual typed + WindowEvent write).
    pub fn key_down(&self, key: KeyCode) -> Result<()> {
        self.write_keyboard(key, ButtonState::Pressed)
    }

    /// Release a key.
    pub fn key_up(&self, key: KeyCode) -> Result<()> {
        self.write_keyboard(key, ButtonState::Released)
    }

    /// Press then release a key, waiting one frame between each edge.
    pub fn press_key(&self, key: KeyCode) -> Result<()> {
        self.key_down(key)?;
        self.wait_frames(1)?;
        self.key_up(key)?;
        self.wait_frames(1)?;
        Ok(())
    }

    /// Move the cursor via `brp_extras/move_mouse` (updates Window cursor + dual-writes).
    pub fn move_mouse(&self, position: Vec2) -> Result<()> {
        let _ = self.brp(
            MOVE_MOUSE_METHOD,
            json!({ "position": [position.x, position.y] }),
        )?;
        Ok(())
    }

    /// Press and hold a mouse button (dual typed + WindowEvent write).
    pub fn mouse_down(&self, button: MouseButton) -> Result<()> {
        self.write_mouse_button(button, ButtonState::Pressed)
    }

    /// Release a mouse button.
    pub fn mouse_up(&self, button: MouseButton) -> Result<()> {
        self.write_mouse_button(button, ButtonState::Released)
    }

    /// Move to `position`, then left-click (down → wait → up → wait).
    pub fn click_at(&self, position: Vec2) -> Result<()> {
        self.move_mouse(position)?;
        self.mouse_down(MouseButton::Left)?;
        self.wait_frames(1)?;
        self.mouse_up(MouseButton::Left)?;
        self.wait_frames(1)?;
        Ok(())
    }

    /// Click the Bevy UI entity matching `id` at its `UiGlobalTransform` center.
    pub fn click(&self, id: &str) -> Result<()> {
        let transform = self
            .component_json(id, UI_GLOBAL_TRANSFORM_TYPE_PATH)
            .map_err(|error| map_click_component_error(id, error))?;

        let arr = transform.as_array().ok_or_else(|| {
            Error::Configuration(format!("selector `{id}` UiGlobalTransform is not an array"))
        })?;
        let physical_x = arr.get(4).and_then(Value::as_f64).ok_or_else(|| {
            Error::Configuration(format!(
                "selector `{id}` UiGlobalTransform missing numeric translation x at index 4"
            ))
        })?;
        let physical_y = arr.get(5).and_then(Value::as_f64).ok_or_else(|| {
            Error::Configuration(format!(
                "selector `{id}` UiGlobalTransform missing numeric translation y at index 5"
            ))
        })?;

        let (_window, scale_factor) = self.primary_window()?;
        let logical = Vec2::new(
            (physical_x / scale_factor) as f32,
            (physical_y / scale_factor) as f32,
        );
        self.click_at(logical)
    }

    fn write_keyboard(&self, key: KeyCode, state: ButtonState) -> Result<()> {
        let (window, _) = self.primary_window()?;
        let event = KeyboardInput {
            key_code: key,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        };
        self.write_dual_message(event)
    }

    fn write_mouse_button(&self, button: MouseButton, state: ButtonState) -> Result<()> {
        let (window, _) = self.primary_window()?;
        let event = MouseButtonInput {
            button,
            state,
            window,
        };
        self.write_dual_message(event)
    }

    /// Dual-write typed message `T` and aggregate `WindowEvent::from(T)` via BRP.
    fn write_dual_message<T>(&self, event: T) -> Result<()>
    where
        T: Serialize + Clone + TypePath,
        WindowEvent: From<T>,
    {
        self.write_message::<T>(&event)?;
        let aggregate = WindowEvent::from(event);
        self.write_message::<WindowEvent>(&aggregate)?;
        Ok(())
    }

    fn write_message<T: Serialize + TypePath>(&self, value: &T) -> Result<()> {
        let value = serde_json::to_value(value).map_err(|error| Error::Brp {
            method: BRP_WRITE_MESSAGE_METHOD.to_owned(),
            message: format!(
                "failed to serialize {} for world.write_message: {error}",
                T::type_path()
            ),
        })?;
        let params = BrpWriteMessageParams {
            message: T::type_path().to_owned(),
            value: Some(value),
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_WRITE_MESSAGE_METHOD.to_owned(),
            message: format!("failed to serialize BrpWriteMessageParams: {error}"),
        })?;
        let _ = self.brp(BRP_WRITE_MESSAGE_METHOD, params)?;
        Ok(())
    }

    fn primary_window(&self) -> Result<(Entity, f64)> {
        let window_type = Window::type_path();
        let primary_type = PrimaryWindow::type_path();
        let params = BrpQueryParams {
            data: BrpQuery {
                components: vec![window_type.to_owned()],
                option: Default::default(),
                has: Vec::new(),
            },
            filter: BrpQueryFilter {
                with: vec![primary_type.to_owned()],
                without: Vec::new(),
            },
            strict: true,
        };
        let params = serde_json::to_value(params).map_err(|error| Error::Brp {
            method: BRP_QUERY_METHOD.to_owned(),
            message: format!("failed to serialize primary window query: {error}"),
        })?;
        let result = self.brp(BRP_QUERY_METHOD, params)?;
        let rows: Vec<BrpQueryRow> =
            serde_json::from_value(result).map_err(|error| Error::Brp {
                method: BRP_QUERY_METHOD.to_owned(),
                message: format!("malformed primary window query response: {error}"),
            })?;
        let row = rows.first().ok_or_else(|| {
            Error::Configuration("primary window entity was not found".to_owned())
        })?;
        let window_json = row.components.get(window_type).ok_or_else(|| {
            Error::Configuration("primary window query missing Window component".to_owned())
        })?;
        let scale_factor = window_json
            .pointer("/resolution/scale_factor")
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                Error::Configuration(
                    "primary Window is missing numeric resolution.scale_factor".to_owned(),
                )
            })?;
        if scale_factor == 0.0 {
            return Err(Error::Configuration(
                "primary Window resolution.scale_factor must not be zero".to_owned(),
            ));
        }
        Ok((row.entity, scale_factor))
    }
}

/// Remap only missing/invalid-component BRP failures into a clear UI-click configuration error.
/// Transport, malformed-response, child-exit, timeout, and other errors stay unchanged.
fn map_click_component_error(id: &str, error: Error) -> Error {
    match error {
        Error::Brp { message, .. } if is_missing_or_invalid_component_brp(&message) => {
            Error::Configuration(format!(
                "selector `{id}` is not a Bevy UI click target (missing or invalid UiGlobalTransform): {message}"
            ))
        }
        other => other,
    }
}

fn is_missing_or_invalid_component_brp(message: &str) -> bool {
    const MARKERS: &[&str] = &[
        "not present",
        "Unknown component",
        "isn't registered",
        "Cannot reflect component",
        "strict get_components response missing",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{is_missing_or_invalid_component_brp, map_click_component_error};
    use crate::Error;
    use std::{process::ExitStatus, time::Duration};

    #[test]
    fn remaps_missing_component_brp_to_configuration() {
        let err = Error::Brp {
            method: "world.get_components".to_owned(),
            message:
                "Component `bevy_ui::ui_transform::UiGlobalTransform` not present in Entity 12v1"
                    .to_owned(),
        };
        match map_click_component_error("main_menu.play", err) {
            Error::Configuration(message) => {
                assert!(
                    message.contains("not a Bevy UI click target"),
                    "expected UI click configuration message, got {message}"
                );
                assert!(message.contains("not present"));
            }
            other => panic!("expected Configuration, got {other:?}"),
        }
    }

    #[test]
    fn preserves_non_ui_transport_brp_errors() {
        let err = Error::Brp {
            method: "world.get_components".to_owned(),
            message: "Connection refused (os error 111)".to_owned(),
        };
        match map_click_component_error("main_menu.play", err) {
            Error::Brp { method, message } => {
                assert_eq!(method, "world.get_components");
                assert_eq!(message, "Connection refused (os error 111)");
                assert!(
                    !message.contains("not a Bevy UI click target"),
                    "transport Brp must not be remapped into configuration wording"
                );
            }
            other => panic!("expected Error::Brp preserved, got {other:?}"),
        }
    }

    #[test]
    fn preserves_malformed_brp_response_errors() {
        let err = Error::Brp {
            method: "world.get_components".to_owned(),
            message: "malformed BRP response: expected value at line 1 column 1".to_owned(),
        };
        match map_click_component_error("player", err) {
            Error::Brp { message, .. } => {
                assert!(message.starts_with("malformed BRP response"));
            }
            other => panic!("expected Error::Brp preserved, got {other:?}"),
        }
    }

    #[test]
    fn preserves_timeout_and_child_exited() {
        let timeout = Error::Timeout {
            operation: "brp world.get_components".to_owned(),
            timeout: Duration::from_secs(5),
        };
        assert!(matches!(
            map_click_component_error("x", timeout),
            Error::Timeout { .. }
        ));

        // Construct a synthetic ExitStatus via a finished process on Unix.
        let status: ExitStatus = std::process::Command::new("true")
            .status()
            .expect("spawn true");
        let child = Error::ChildExited(status);
        assert!(matches!(
            map_click_component_error("x", child),
            Error::ChildExited(_)
        ));
    }

    #[test]
    fn marker_detection_is_narrow() {
        assert!(is_missing_or_invalid_component_brp(
            "Component `bevy_ui::ui_transform::UiGlobalTransform` not present in Entity 1v0"
        ));
        assert!(is_missing_or_invalid_component_brp(
            "Unknown component type: `bevy_ui::ui_transform::UiGlobalTransform`"
        ));
        assert!(is_missing_or_invalid_component_brp(
            "strict get_components response missing `bevy_ui::ui_transform::UiGlobalTransform`"
        ));
        assert!(!is_missing_or_invalid_component_brp(
            "Connection refused (os error 111)"
        ));
        assert!(!is_missing_or_invalid_component_brp(
            "malformed BRP response: missing result/error"
        ));
        assert!(!is_missing_or_invalid_component_brp("Method not found"));
    }
}
