use tauri::{
    App, AppHandle, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::commands::{ServiceAction, ServiceState, current_service_state, perform_service_action};

const OPEN: &str = "open";
const TOGGLE: &str = "toggle";
const RESTART: &str = "restart";
const QUIT: &str = "quit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrayIntent {
    Open,
    Toggle,
    Restart,
    Quit,
}

fn intent(id: &str) -> Option<TrayIntent> {
    match id {
        OPEN => Some(TrayIntent::Open),
        TOGGLE => Some(TrayIntent::Toggle),
        RESTART => Some(TrayIntent::Restart),
        QUIT => Some(TrayIntent::Quit),
        _ => None,
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, "Open", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, TOGGLE, "Start/Stop", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, RESTART, "Restart", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &restart, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("DNS Relay")
        .menu(&menu)
        .on_menu_event(handle_menu);
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    #[cfg(target_os = "macos")]
    let tray = tray.icon_as_template(false);
    tray.build(app)?;
    Ok(())
}

fn handle_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match intent(event.id().as_ref()) {
        Some(TrayIntent::Open) => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        Some(TrayIntent::Toggle) => toggle_service(),
        Some(TrayIntent::Restart) => run_service(ServiceAction::Restart),
        Some(TrayIntent::Quit) => app.exit(0),
        None => {}
    }
}

fn run_service(action: ServiceAction) {
    tauri::async_runtime::spawn_blocking(move || {
        let _ = perform_service_action(action);
    });
}

fn toggle_service() {
    tauri::async_runtime::spawn_blocking(|| {
        let action = match current_service_state() {
            Ok(ServiceState::Running) => ServiceAction::Stop,
            _ => ServiceAction::Start,
        };
        let _ = perform_service_action(action);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_is_a_gui_intent_not_a_service_action() {
        assert_eq!(intent(QUIT), Some(TrayIntent::Quit));
        assert_eq!(intent(RESTART), Some(TrayIntent::Restart));
        assert_eq!(intent("stop-service-and-quit"), None);
    }
}
