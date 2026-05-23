pub mod connect;
pub mod main_view;
pub mod devices;
pub mod widgets;

use ratatui::Frame;
use crate::app::{AppState, Screen};

pub fn render(f: &mut Frame, state: &AppState) {
    match state.screen {
        Screen::Connect => connect::render(f, state),
        Screen::Main => {
            main_view::render(f, state);
            if state.screen == Screen::DeviceSelect {
                devices::render_overlay(f, state);
            }
        }
        Screen::DeviceSelect => {
            main_view::render(f, state);
            devices::render_overlay(f, state);
        }
    }

    // Глобальное уведомление поверх всего
    if let Some((msg, _)) = &state.notification {
        widgets::render_notification(f, msg);
    }
}
