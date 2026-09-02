pub mod commands;
pub mod observability;
pub mod secrets;
pub mod state;
pub mod tray;
mod vault;

use commands::{
    adopt_service, apply_draft, delete_secret, export_config, generate_secret, get_app_state,
    get_service_state, install_service, load_draft, parse_blocklist, parse_config, read_history,
    read_logs, reveal_secret, service_action, store_relay_secret, test_relay, test_resolver,
    validate_draft,
};
use observability::get_observability;
use state::BackendState;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| Ok(tray::setup(app)?))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .manage(BackendState::default())
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            get_service_state,
            get_observability,
            load_draft,
            validate_draft,
            apply_draft,
            install_service,
            adopt_service,
            service_action,
            test_resolver,
            test_relay,
            read_logs,
            read_history,
            parse_config,
            parse_blocklist,
            export_config,
            generate_secret,
            store_relay_secret,
            reveal_secret,
            delete_secret
        ])
        .build(tauri::generate_context!())
        .expect("failed to build DNS Relay GUI");
    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            tray::show(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod secrets_tests;
