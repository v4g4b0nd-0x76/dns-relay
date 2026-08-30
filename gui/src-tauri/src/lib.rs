pub mod commands;
pub mod observability;
pub mod secrets;
pub mod state;
pub mod tray;
mod vault;

use commands::{
    adopt_service, apply_draft, get_app_state, get_service_state, install_service, load_draft,
    read_history, read_logs, service_action, test_relay, test_resolver, validate_draft,
};
use observability::get_observability;
use state::BackendState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| Ok(tray::setup(app)?))
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
            read_history
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DNS Relay GUI");
}

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod secrets_tests;
