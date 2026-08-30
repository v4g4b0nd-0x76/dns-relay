use std::{fs, process::Command, sync::Arc};

use dns_relay::conf::Conf;
use dns_relay_admin::{AdminAction, AdminRequest, PlatformPaths, platform::CommandSpec};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    secrets::{SecretId, SecretStore},
    state::BackendState,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    NotInstalled,
    Stopped,
    Running,
    Applying,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Repair,
    Uninstall,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: None,
        }
    }

    pub fn field(
        code: impl Into<String>,
        message: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: Some(field.into()),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub service: ServiceState,
    pub draft: Option<Conf>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<CommandError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub service: ServiceState,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub reachable: bool,
    pub message: String,
}

#[tauri::command]
pub async fn get_app_state(state: State<'_, BackendState>) -> Result<AppState, CommandError> {
    let service = tauri::async_runtime::spawn_blocking(current_service_state)
        .await
        .map_err(|error| CommandError::new("service_status_failed", error.to_string()))??;
    let draft = state
        .draft
        .lock()
        .map_err(|_| unavailable("draft state"))?
        .clone();
    let warnings = draft
        .is_none()
        .then(|| "Existing configuration must be adopted before editing".into())
        .into_iter()
        .collect();
    Ok(AppState {
        service,
        draft,
        warnings,
    })
}

#[tauri::command]
pub fn load_draft(state: State<'_, BackendState>) -> Result<Conf, CommandError> {
    state
        .draft
        .lock()
        .map_err(|_| unavailable("draft state"))?
        .clone()
        .ok_or_else(adoption_required)
}

#[tauri::command]
pub fn validate_draft(draft: Conf) -> ValidationResult {
    match draft.validate() {
        Ok(()) => ValidationResult {
            valid: true,
            errors: Vec::new(),
        },
        Err(error) => ValidationResult {
            valid: false,
            errors: vec![CommandError::new("invalid_config", error.to_string())],
        },
    }
}

#[tauri::command]
pub async fn apply_draft(
    state: State<'_, BackendState>,
    draft: Conf,
) -> Result<ApplyResult, CommandError> {
    if state
        .draft
        .lock()
        .map_err(|_| unavailable("draft state"))?
        .is_none()
    {
        return Err(adoption_required());
    }
    draft
        .validate()
        .map_err(|error| CommandError::new("invalid_config", error.to_string()))?;
    let secrets = Arc::clone(&state.secrets);
    let apply_copy = draft.clone();
    let service = tauri::async_runtime::spawn_blocking(move || {
        let mut materialized = materialize_for_apply(&apply_copy, secrets.as_ref())?;
        let config_toml = materialized.to_toml();
        zeroize_materialized_secrets(&mut materialized);
        let config_toml =
            config_toml.map_err(|error| CommandError::new("invalid_config", error.to_string()))?;
        submit_admin(AdminAction::ApplyConfig {
            config_toml,
            restart: true,
        })?;
        current_service_state()
    })
    .await
    .map_err(|error| CommandError::new("apply_failed", error.to_string()))??;
    *state.draft.lock().map_err(|_| unavailable("draft state"))? = Some(draft);
    Ok(ApplyResult {
        service,
        message: "Configuration applied".into(),
    })
}

#[tauri::command]
pub async fn service_action(action: ServiceAction) -> Result<ServiceState, CommandError> {
    let admin_action = match action {
        ServiceAction::Start => AdminAction::Start,
        ServiceAction::Stop => AdminAction::Stop,
        ServiceAction::Restart => AdminAction::Restart,
        ServiceAction::Uninstall => AdminAction::Uninstall,
        ServiceAction::Repair => {
            return Err(CommandError::new(
                "sidecar_unavailable",
                "Repair is available after sidecar staging",
            ));
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        submit_admin(admin_action)?;
        current_service_state()
    })
    .await
    .map_err(|error| CommandError::new("service_action_failed", error.to_string()))?
}

#[tauri::command]
pub fn test_resolver(resolver: String) -> Result<ProbeResult, CommandError> {
    reject_empty(&resolver, "resolver")?;
    Err(CommandError::new(
        "probe_unavailable",
        "Resolver probes are not connected yet",
    ))
}

#[tauri::command]
pub fn test_relay(relay_url: String) -> Result<ProbeResult, CommandError> {
    if !relay_url.starts_with("https://") {
        return Err(CommandError::field(
            "invalid_relay_url",
            "Relay URL must use HTTPS",
            "relayUrl",
        ));
    }
    Err(CommandError::new(
        "probe_unavailable",
        "Relay probes are not connected yet",
    ))
}

#[tauri::command]
pub fn read_logs(limit: u16) -> Result<Vec<String>, CommandError> {
    validate_limit(limit)?;
    Ok(Vec::new())
}

#[tauri::command]
pub fn read_history(limit: u16) -> Result<Vec<String>, CommandError> {
    validate_limit(limit)?;
    Ok(Vec::new())
}

fn materialize_secrets(draft: &mut Conf, store: &impl SecretStore) -> Result<(), CommandError> {
    for (index, relay) in draft.relay_conf.relay_instances.iter_mut().enumerate() {
        if relay.relay_key.is_empty() {
            continue;
        }
        relay.relay_key = materialize_reference(
            &relay.relay_key,
            store,
            &format!("relayInstances.{index}.relayKey"),
        )?;
    }
    for (index, key) in draft.obfs_conf.keys.iter_mut().enumerate() {
        *key = materialize_reference(key, store, &format!("obfsKeys.{index}"))?;
    }
    Ok(())
}

pub(crate) fn materialize_for_apply(
    draft: &Conf,
    store: &impl SecretStore,
) -> Result<Conf, CommandError> {
    let mut materialized = draft.clone();
    match materialize_secrets(&mut materialized, store) {
        Ok(()) => Ok(materialized),
        Err(error) => {
            zeroize_materialized_secrets(&mut materialized);
            Err(error)
        }
    }
}

fn materialize_reference(
    reference: &str,
    store: &impl SecretStore,
    field: &str,
) -> Result<String, CommandError> {
    let id = reference
        .strip_prefix("vault://")
        .ok_or_else(|| {
            CommandError::field(
                "secret_reference_required",
                "Secrets must be stored in the credential vault",
                field,
            )
        })
        .and_then(|id| {
            SecretId::new(id).map_err(|error| {
                CommandError::field("invalid_secret_reference", error.to_string(), field)
            })
        })?;
    let value = store
        .get(&id)
        .map_err(|error| CommandError::field("secret_unavailable", error.to_string(), field))?;
    std::str::from_utf8(value.expose())
        .map(str::to_owned)
        .map_err(|_| CommandError::field("secret_encoding", "Secret is not UTF-8", field))
}

fn zeroize_materialized_secrets(config: &mut Conf) {
    for relay in &mut config.relay_conf.relay_instances {
        relay.relay_key.zeroize();
    }
    for key in &mut config.obfs_conf.keys {
        key.zeroize();
    }
}

fn submit_admin(mut action: AdminAction) -> Result<(), CommandError> {
    let paths = match PlatformPaths::current() {
        Ok(paths) => paths,
        Err(error) => {
            action.zeroize_sensitive();
            return Err(admin_error(error));
        }
    };
    let id = Uuid::new_v4();
    let mut request = AdminRequest { id, action };
    let write_result = paths.write_request(&request);
    request.action.zeroize_sensitive();
    write_result.map_err(admin_error)?;
    let command = elevation_command(&paths, id).inspect_err(|_| {
        let _ = fs::remove_file(paths.request_path(id));
    })?;
    let status = Command::new(&command.program)
        .args(&command.args)
        .status()
        .inspect_err(|_| {
            let _ = fs::remove_file(paths.request_path(id));
        })
        .map_err(|error| CommandError::new("elevation_failed", error.to_string()))?;
    if !status.success() {
        let _ = fs::remove_file(paths.request_path(id));
        return Err(CommandError::new(
            "elevation_cancelled",
            "Administrator authorization was cancelled or failed",
        ));
    }
    let response = paths.read_response(id);
    let _ = fs::remove_file(paths.response_path(id));
    let response = response.map_err(admin_error)?;
    if response.ok {
        Ok(())
    } else {
        Err(CommandError::new("admin_failed", response.message))
    }
}

fn elevation_command(paths: &PlatformPaths, id: Uuid) -> Result<CommandSpec, CommandError> {
    #[cfg(target_os = "macos")]
    return dns_relay_admin::platform::macos::elevation_command(
        &paths.admin_binary,
        &id.to_string(),
    )
    .map_err(admin_error);

    #[cfg(target_os = "linux")]
    return dns_relay_admin::platform::linux::elevation_command(
        &paths.admin_binary,
        &id.to_string(),
    )
    .map_err(admin_error);

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    Err(CommandError::new(
        "unsupported_platform",
        "Service elevation is not implemented on this platform",
    ))
}

fn current_service_state() -> Result<ServiceState, CommandError> {
    let paths = PlatformPaths::current().map_err(admin_error)?;
    if !paths.installed_binary.is_file() {
        return Ok(ServiceState::NotInstalled);
    }
    use dns_relay_admin::apply::ServiceManager;
    #[cfg(target_os = "macos")]
    let status = dns_relay_admin::platform::macos::MacosServiceManager::new(paths).status();
    #[cfg(target_os = "linux")]
    let status = dns_relay_admin::platform::linux::LinuxServiceManager::new(paths).status();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Ok(ServiceState::Stopped);

    status
        .map(|status| match status {
            dns_relay_admin::apply::ServiceStatus::Running => ServiceState::Running,
            dns_relay_admin::apply::ServiceStatus::Stopped => ServiceState::Stopped,
        })
        .map_err(admin_error)
}

fn validate_limit(limit: u16) -> Result<(), CommandError> {
    if (1..=1_000).contains(&limit) {
        Ok(())
    } else {
        Err(CommandError::new(
            "invalid_limit",
            "Line limit must be between 1 and 1000",
        ))
    }
}

fn reject_empty(value: &str, field: &str) -> Result<(), CommandError> {
    if value.trim().is_empty() {
        Err(CommandError::field("required", "Value is required", field))
    } else {
        Ok(())
    }
}

fn unavailable(subject: &str) -> CommandError {
    CommandError::new("unavailable", format!("{subject} is unavailable"))
}

fn adoption_required() -> CommandError {
    CommandError::new(
        "adoption_required",
        "Existing configuration must be adopted before editing",
    )
}

fn admin_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::new("admin_error", error.to_string())
}
