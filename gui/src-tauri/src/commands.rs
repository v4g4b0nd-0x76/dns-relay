use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Instant,
};

use dns_relay::conf::Conf;
use dns_relay_admin::{
    AdminAction, AdminRequest, AdminResponse, PlatformPaths, platform::CommandSpec,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    secrets::{SecretId, SecretStore},
    state::{BackendState, starter_draft},
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
    pub recovery_required: bool,
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
    pub latency_ms: u128,
}

#[tauri::command]
pub async fn get_app_state(state: State<'_, BackendState>) -> Result<AppState, CommandError> {
    let service_result = tauri::async_runtime::spawn_blocking(current_service_state).await;
    let draft = state
        .draft
        .lock()
        .map_err(|_| unavailable("draft state"))?
        .clone();
    let mut warnings: Vec<String> = draft
        .is_none()
        .then(|| "Existing configuration must be adopted before editing".into())
        .into_iter()
        .collect();
    let recovery_required = state.recovery_required.load(Relaxed);
    if recovery_required {
        warnings.push("Installation is incomplete; run Repair to restore fixed assets".into());
    }
    let service = match service_result {
        Ok(Ok(service)) => service,
        Ok(Err(error)) => {
            warnings.push(format!("Service status unavailable: {error}"));
            ServiceState::Error
        }
        Err(error) => {
            warnings.push(format!("Service status task failed: {error}"));
            ServiceState::Error
        }
    };
    Ok(AppState {
        service,
        draft,
        warnings,
        recovery_required,
    })
}

#[tauri::command]
pub async fn get_service_state() -> Result<ServiceState, CommandError> {
    tauri::async_runtime::spawn_blocking(current_service_state)
        .await
        .map_err(|error| CommandError::new("service_status_failed", error.to_string()))?
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
    let errors = gui_validation_errors(&draft);
    if errors.is_empty() {
        ValidationResult {
            valid: true,
            errors: Vec::new(),
        }
    } else {
        ValidationResult {
            valid: false,
            errors,
        }
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
    require_valid_draft(&draft)?;
    let saved = state
        .draft
        .lock()
        .map_err(|_| unavailable("draft state"))?
        .clone();
    let restart = config_change_requires_restart(saved.as_ref(), &draft);
    let secrets = Arc::clone(&state.secrets);
    let apply_copy = draft.clone();
    let service = tauri::async_runtime::spawn_blocking(move || {
        ensure_versions_match()?;
        let config_toml = config_for_admin(&apply_copy, secrets.as_ref())?;
        submit_admin(AdminAction::ApplyConfig {
            config_toml,
            restart,
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
pub async fn install_service(
    state: State<'_, BackendState>,
    draft: Conf,
) -> Result<ApplyResult, CommandError> {
    require_valid_draft(&draft)?;
    let secrets = Arc::clone(&state.secrets);
    let install_copy = draft.clone();
    let service = tauri::async_runtime::spawn_blocking(move || {
        let (helper, resolver) = bundled_paths()?;
        require_sidecar(&helper, "admin helper")?;
        require_sidecar(&resolver, "resolver")?;
        let expected_binary_sha256 =
            dns_relay_admin::sha256_file(&resolver).map_err(admin_error)?;
        let config_toml = config_for_admin(&install_copy, secrets.as_ref())?;
        submit_admin_with_helper(
            AdminAction::Install {
                config_toml,
                expected_binary_sha256,
            },
            &helper,
        )?;
        current_service_state()
    })
    .await
    .map_err(|error| CommandError::new("install_failed", error.to_string()))??;
    *state.draft.lock().map_err(|_| unavailable("draft state"))? = Some(draft);
    state.recovery_required.store(false, Relaxed);
    Ok(ApplyResult {
        service,
        message: "DNS Relay installed".into(),
    })
}

#[tauri::command]
pub async fn adopt_service(state: State<'_, BackendState>) -> Result<Conf, CommandError> {
    let migration_store = Arc::clone(&state.secrets);
    let rollback_store = Arc::clone(&state.secrets);
    let (mut draft, inserted) = tauri::async_runtime::spawn_blocking(move || {
        let paths = PlatformPaths::current().map_err(admin_error)?;
        require_sidecar(&paths.admin_binary, "installed admin helper")?;
        let mut response =
            request_admin_with_paths(AdminAction::ReadConfig, &paths, &paths.admin_binary)?;
        let config_toml = Zeroizing::new(std::mem::take(&mut response.message));
        let mut draft: Conf = toml::from_str(&config_toml)
            .map_err(|error| CommandError::new("invalid_installed_config", error.to_string()))?;
        draft
            .validate()
            .map_err(|error| CommandError::new("invalid_installed_config", error.to_string()))?;
        let inserted = migrate_legacy_secrets(&mut draft, migration_store.as_ref())?;
        Ok((draft, inserted))
    })
    .await
    .map_err(|error| CommandError::new("adoption_failed", error.to_string()))??;
    let mut state_draft = match state.draft.lock() {
        Ok(state_draft) => state_draft,
        Err(_) => {
            for id in inserted {
                let _ = rollback_store.delete(&id);
            }
            zeroize_materialized_secrets(&mut draft);
            return Err(unavailable("draft state"));
        }
    };
    *state_draft = Some(draft.clone());
    state.recovery_required.store(false, Relaxed);
    Ok(draft)
}

#[tauri::command]
pub async fn service_action(
    state: State<'_, BackendState>,
    action: ServiceAction,
) -> Result<ServiceState, CommandError> {
    let repair_draft = if action == ServiceAction::Repair {
        let recovery_required = state.recovery_required.load(Relaxed);
        Some(
            state
                .draft
                .lock()
                .map_err(|_| unavailable("draft state"))?
                .clone()
                .or_else(|| recovery_required.then(starter_draft))
                .ok_or_else(adoption_required)?,
        )
    } else {
        None
    };
    let secrets = Arc::clone(&state.secrets);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let repair_config = repair_draft
            .as_ref()
            .map(|draft| config_for_admin(draft, secrets.as_ref()))
            .transpose()?;
        perform_service_action_with_config(action, repair_config)
    })
    .await
    .map_err(|error| CommandError::new("service_action_failed", error.to_string()))?;
    if result.is_ok() && matches!(action, ServiceAction::Repair | ServiceAction::Uninstall) {
        state.recovery_required.store(false, Relaxed);
    }
    result
}

pub(crate) fn perform_service_action(action: ServiceAction) -> Result<ServiceState, CommandError> {
    perform_service_action_with_config(action, None)
}

fn perform_service_action_with_config(
    action: ServiceAction,
    repair_config: Option<String>,
) -> Result<ServiceState, CommandError> {
    let admin_action = match action {
        ServiceAction::Start => AdminAction::Start,
        ServiceAction::Stop => AdminAction::Stop,
        ServiceAction::Restart => AdminAction::Restart,
        ServiceAction::Uninstall => AdminAction::Uninstall,
        ServiceAction::Repair => {
            let (helper, resolver) = bundled_paths()?;
            require_sidecar(&helper, "admin helper")?;
            require_sidecar(&resolver, "resolver")?;
            let expected_binary_sha256 =
                dns_relay_admin::sha256_file(&resolver).map_err(admin_error)?;
            submit_admin_with_helper(
                AdminAction::Repair {
                    expected_binary_sha256,
                    config_toml: repair_config.ok_or_else(|| {
                        CommandError::new("repair_config_required", "Repair requires a valid draft")
                    })?,
                },
                &helper,
            )?;
            return current_service_state();
        }
    };
    submit_admin(admin_action)?;
    current_service_state()
}

#[tauri::command]
pub async fn test_resolver(resolver: String) -> Result<ProbeResult, CommandError> {
    reject_empty(&resolver, "resolver")?;
    if !valid_resolver(&resolver) {
        return Err(CommandError::field(
            "invalid_resolver",
            "Resolver must be an HTTPS, QUIC, or socket endpoint",
            "resolver",
        ));
    }
    let started = Instant::now();
    dns_relay::DnsResolver::new(dns_relay::ResolverConfig {
        resolvers: vec![resolver],
        relay: None,
    })
    .await
    .map_err(|error| CommandError::new("resolver_unreachable", error.to_string()))?;
    Ok(ProbeResult {
        reachable: true,
        message: "Resolver is reachable".into(),
        latency_ms: started.elapsed().as_millis(),
    })
}

#[tauri::command]
pub async fn test_relay(relay_url: String) -> Result<ProbeResult, CommandError> {
    if !relay_url.starts_with("https://") {
        return Err(CommandError::field(
            "invalid_relay_url",
            "Relay URL must use HTTPS",
            "relayUrl",
        ));
    }
    let started = Instant::now();
    let response = shared::build_http_client()
        .map_err(|error| CommandError::new("relay_probe_failed", error.to_string()))?
        .head(&relay_url)
        .send()
        .await
        .map_err(|error| CommandError::new("relay_unreachable", error.to_string()))?;
    Ok(ProbeResult {
        reachable: true,
        message: format!("Relay responded with {}", response.status()),
        latency_ms: started.elapsed().as_millis(),
    })
}

#[tauri::command]
pub fn read_logs(limit: u16) -> Result<Vec<String>, CommandError> {
    validate_limit(limit)?;
    let paths = PlatformPaths::current().map_err(admin_error)?;
    read_bounded_lines(
        &[
            paths.logs.join("dns-relay.out.log"),
            paths.logs.join("dns-relay.err.log"),
        ],
        usize::from(limit),
    )
}

#[tauri::command]
pub fn read_history(limit: u16) -> Result<Vec<String>, CommandError> {
    validate_limit(limit)?;
    let paths = PlatformPaths::current().map_err(admin_error)?;
    let history = paths
        .config
        .parent()
        .ok_or_else(|| unavailable("history path"))?
        .join("history.txt");
    read_bounded_lines(&[history], usize::from(limit))
}

#[tauri::command]
pub fn parse_config(config_toml: String) -> Result<Conf, CommandError> {
    let config_toml = Zeroizing::new(config_toml);
    reject_unknown_config_fields(&config_toml)?;
    let mut draft: Conf = toml::from_str(&config_toml)
        .map_err(|error| CommandError::new("invalid_config", error.to_string()))?;
    if let Err(error) = require_valid_draft(&draft).and_then(|_| require_secret_references(&draft))
    {
        zeroize_materialized_secrets(&mut draft);
        return Err(error);
    }
    Ok(draft)
}

#[tauri::command]
pub fn parse_blocklist(content: String) -> Vec<String> {
    shared::domain_trie::parse_blocklist(&content)
}

#[tauri::command]
pub fn export_config(
    state: State<'_, BackendState>,
    draft: Conf,
    plaintext: bool,
) -> Result<String, CommandError> {
    require_valid_draft(&draft)?;
    if plaintext {
        config_for_admin(&draft, state.secrets.as_ref())
    } else {
        require_secret_references(&draft)?;
        draft
            .to_toml()
            .map_err(|error| CommandError::new("export_failed", error.to_string()))
    }
}

#[tauri::command]
pub async fn generate_secret(
    state: State<'_, BackendState>,
    kind: String,
) -> Result<String, CommandError> {
    if !matches!(kind.as_str(), "relay" | "obfs") {
        return Err(CommandError::new(
            "invalid_secret_kind",
            "Unknown secret kind",
        ));
    }
    let secrets = Arc::clone(&state.secrets);
    tauri::async_runtime::spawn_blocking(move || {
        let id = SecretId::new(format!("{kind}.{}", Uuid::new_v4()))
            .map_err(|error| CommandError::new("secret_failed", error.to_string()))?;
        let value = Zeroizing::new(dns_relay::generate_relay_key());
        secrets
            .put(&id, value.as_bytes())
            .map_err(|error| CommandError::new("secret_failed", error.to_string()))?;
        Ok(format!("vault://{}", id.as_str()))
    })
    .await
    .map_err(|error| CommandError::new("secret_failed", error.to_string()))?
}

#[tauri::command]
pub async fn reveal_secret(
    state: State<'_, BackendState>,
    reference: String,
) -> Result<String, CommandError> {
    let secrets = Arc::clone(&state.secrets);
    tauri::async_runtime::spawn_blocking(move || {
        let id = secret_id_from_reference(&reference)?;
        let value = secrets
            .get(&id)
            .map_err(|error| CommandError::new("secret_unavailable", error.to_string()))?;
        std::str::from_utf8(value.expose())
            .map(str::to_owned)
            .map_err(|_| CommandError::new("secret_encoding", "Secret is not UTF-8"))
    })
    .await
    .map_err(|error| CommandError::new("secret_unavailable", error.to_string()))?
}

#[tauri::command]
pub async fn delete_secret(
    state: State<'_, BackendState>,
    reference: String,
) -> Result<(), CommandError> {
    let secrets = Arc::clone(&state.secrets);
    tauri::async_runtime::spawn_blocking(move || {
        let id = secret_id_from_reference(&reference)?;
        secrets
            .delete(&id)
            .map_err(|error| CommandError::new("secret_failed", error.to_string()))
    })
    .await
    .map_err(|error| CommandError::new("secret_failed", error.to_string()))?
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

fn config_for_admin(draft: &Conf, store: &impl SecretStore) -> Result<String, CommandError> {
    let mut materialized = materialize_for_apply(draft, store)?;
    let config_toml = materialized.to_toml();
    zeroize_materialized_secrets(&mut materialized);
    config_toml.map_err(|error| CommandError::new("invalid_config", error.to_string()))
}

pub(crate) fn config_change_requires_restart(saved: Option<&Conf>, draft: &Conf) -> bool {
    if !draft.hotreload_conf.enable {
        return true;
    }
    let Some(saved) = saved else {
        return true;
    };
    let mut saved_without_rules = saved.clone();
    let mut draft_without_rules = draft.clone();
    saved_without_rules.drop_list.clear();
    saved_without_rules.redirect_list.clear();
    draft_without_rules.drop_list.clear();
    draft_without_rules.redirect_list.clear();
    saved_without_rules.to_toml().ok() != draft_without_rules.to_toml().ok()
}

pub(crate) fn migrate_legacy_secrets(
    draft: &mut Conf,
    store: &impl SecretStore,
) -> Result<Vec<SecretId>, CommandError> {
    let mut inserted = Vec::new();
    let result = (|| {
        for (index, relay) in draft.relay_conf.relay_instances.iter_mut().enumerate() {
            migrate_secret(
                &mut relay.relay_key,
                "relay",
                &format!("relayInstances.{index}.relayKey"),
                store,
                &mut inserted,
            )?;
        }
        for (index, key) in draft.obfs_conf.keys.iter_mut().enumerate() {
            migrate_secret(
                key,
                "obfs",
                &format!("obfsKeys.{index}"),
                store,
                &mut inserted,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        for id in inserted {
            let _ = store.delete(&id);
        }
        zeroize_materialized_secrets(draft);
        return Err(error);
    }
    Ok(inserted)
}

fn migrate_secret(
    value: &mut String,
    kind: &str,
    field: &str,
    store: &impl SecretStore,
    inserted: &mut Vec<SecretId>,
) -> Result<(), CommandError> {
    if value.is_empty() {
        return Ok(());
    }
    let plaintext = Zeroizing::new(std::mem::take(value));
    if let Some(id) = plaintext.strip_prefix("vault://") {
        SecretId::new(id).map_err(|error| {
            CommandError::field("invalid_secret_reference", error.to_string(), field)
        })?;
        *value = plaintext.to_string();
        return Ok(());
    }
    let id = SecretId::new(format!("adopted.{kind}.{}", Uuid::new_v4())).map_err(|error| {
        CommandError::field("secret_migration_failed", error.to_string(), field)
    })?;
    store.put(&id, plaintext.as_bytes()).map_err(|error| {
        CommandError::field("secret_migration_failed", error.to_string(), field)
    })?;
    *value = format!("vault://{}", id.as_str());
    inserted.push(id);
    Ok(())
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
    let helper = paths.admin_binary.clone();
    request_admin_with_paths(action, &paths, &helper).map(|_| ())
}

fn submit_admin_with_helper(mut action: AdminAction, helper: &Path) -> Result<(), CommandError> {
    let paths = match PlatformPaths::current() {
        Ok(paths) => paths,
        Err(error) => {
            action.zeroize_sensitive();
            return Err(admin_error(error));
        }
    };
    request_admin_with_paths(action, &paths, helper).map(|_| ())
}

fn request_admin_with_paths(
    action: AdminAction,
    paths: &PlatformPaths,
    helper: &Path,
) -> Result<AdminResponse, CommandError> {
    let id = Uuid::new_v4();
    let mut request = AdminRequest { id, action };
    let write_result = paths.write_request(&request);
    request.action.zeroize_sensitive();
    write_result.map_err(admin_error)?;
    let command = elevation_command(helper, id).inspect_err(|_| {
        let _ = fs::remove_file(paths.request_path(id));
    })?;
    let mut elevated = Command::new(&command.program);
    elevated.args(&command.args);
    #[cfg(target_os = "macos")]
    elevated.current_dir("/");
    let status = elevated
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
    let mut response = response.map_err(admin_error)?;
    if response.ok {
        Ok(response)
    } else {
        let message = std::mem::take(&mut response.message);
        Err(CommandError::new("admin_failed", message))
    }
}

fn elevation_command(helper: &Path, id: Uuid) -> Result<CommandSpec, CommandError> {
    #[cfg(target_os = "macos")]
    return dns_relay_admin::platform::macos::elevation_command(helper, &id.to_string())
        .map_err(admin_error);

    #[cfg(target_os = "linux")]
    return dns_relay_admin::platform::linux::elevation_command(helper, &id.to_string())
        .map_err(admin_error);

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    Err(CommandError::new(
        "unsupported_platform",
        "Service elevation is not implemented on this platform",
    ))
}

fn bundled_paths() -> Result<(PathBuf, PathBuf), CommandError> {
    std::env::current_exe()
        .map(|executable| bundled_paths_from_exe(&executable))
        .map_err(|error| CommandError::new("sidecar_unavailable", error.to_string()))
}

pub(crate) fn bundled_paths_from_exe(executable: &Path) -> (PathBuf, PathBuf) {
    let parent = executable.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(target_os = "windows")]
    return (
        parent.join("dns_relay_admin.exe"),
        parent.join("dns_relay.exe"),
    );
    #[cfg(not(target_os = "windows"))]
    (parent.join("dns_relay_admin"), parent.join("dns_relay"))
}

fn require_sidecar(path: &Path, name: &str) -> Result<(), CommandError> {
    path.is_file().then_some(()).ok_or_else(|| {
        CommandError::new(
            "sidecar_unavailable",
            format!("Bundled {name} is missing at {}", path.display()),
        )
    })
}

fn ensure_versions_match() -> Result<(), CommandError> {
    let paths = PlatformPaths::current().map_err(admin_error)?;
    let (_, bundled) = bundled_paths()?;
    require_sidecar(&bundled, "resolver")?;
    require_sidecar(&paths.installed_binary, "installed resolver")?;
    let bundled_version = binary_version(&bundled)?;
    let installed_version = binary_version(&paths.installed_binary)?;
    if version_outputs_match(&bundled_version, &installed_version) {
        Ok(())
    } else {
        Err(CommandError::new(
            "update_required",
            format!(
                "Bundled resolver {bundled_version} does not match installed resolver {installed_version}"
            ),
        ))
    }
}

fn binary_version(path: &Path) -> Result<String, CommandError> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|error| CommandError::new("version_check_failed", error.to_string()))?;
    if !output.status.success() {
        return Err(CommandError::new(
            "version_check_failed",
            format!(
                "{} --version failed ({}): {}",
                path.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|version| version.trim().to_string())
        .map_err(|error| CommandError::new("version_check_failed", error.to_string()))
}

pub(crate) fn version_outputs_match(bundled: &str, installed: &str) -> bool {
    bundled.trim() == installed.trim() && !bundled.trim().is_empty()
}

pub(crate) fn current_service_state() -> Result<ServiceState, CommandError> {
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

fn require_valid_draft(draft: &Conf) -> Result<(), CommandError> {
    gui_validation_errors(draft)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

fn gui_validation_errors(draft: &Conf) -> Vec<CommandError> {
    let mut errors = Vec::new();
    if let Err(error) = draft.validate() {
        errors.push(CommandError::new("invalid_config", error.to_string()));
    }
    if draft.dns_target.parse::<SocketAddr>().is_err() {
        errors.push(CommandError::field(
            "invalid_listener",
            "Listener must be an IP address and port",
            "dnsTarget",
        ));
    }
    for (index, resolver) in draft.resolvers.iter().enumerate() {
        if !valid_resolver(resolver) {
            errors.push(CommandError::field(
                "invalid_resolver",
                "Resolver must be an HTTPS, QUIC, or socket endpoint",
                format!("resolvers.{index}"),
            ));
        }
    }
    for (index, rule) in draft.drop_list.iter().enumerate() {
        if !is_file_reference(rule) && !valid_domain_pattern(rule) {
            errors.push(CommandError::field(
                "invalid_drop_rule",
                "Drop rule must be a domain pattern or local list path",
                format!("dropList.{index}"),
            ));
        }
    }
    for (index, (domain, target)) in draft.redirect_list.iter().enumerate() {
        if is_file_reference(domain)
            || !valid_domain_pattern(domain)
            || !target.split(',').all(valid_redirect_target)
        {
            errors.push(CommandError::field(
                "invalid_redirect_rule",
                "Redirect rule must map a domain pattern to IP addresses",
                format!("redirectList.{index}"),
            ));
        }
    }
    for (index, source) in draft.resolver_searching.resolver_source.iter().enumerate() {
        if !source.starts_with("https://") || dns_relay::relay::host_from_url(source).is_err() {
            errors.push(CommandError::field(
                "invalid_resolver_source",
                "Resolver discovery sources must use HTTPS",
                format!("resolverSearching.resolverSource.{index}"),
            ));
        }
    }
    if draft.resolver_searching.resfresh_interval == Some(0) {
        errors.push(CommandError::field(
            "invalid_refresh_interval",
            "Refresh interval must be greater than zero",
            "resolverSearching.resfreshInterval",
        ));
    }
    if draft.hotreload_conf.poll_interval_ms == 0 {
        errors.push(CommandError::field(
            "invalid_poll_interval",
            "Hot reload interval must be greater than zero",
            "hotreloadConf.pollIntervalMs",
        ));
    }
    if draft.metric_conf.report_interval == 0 {
        errors.push(CommandError::field(
            "invalid_metric_interval",
            "Metrics interval must be greater than zero",
            "metricConf.reportInterval",
        ));
    }
    if draft.relay_conf.relay_timeout_sec == 0 {
        errors.push(CommandError::field(
            "invalid_relay_timeout",
            "Relay timeout must be greater than zero",
            "relayConf.relayTimeoutSec",
        ));
    }
    for (index, relay) in draft.relay_conf.relay_instances.iter().enumerate() {
        if !relay.relay_url.starts_with("https://")
            || dns_relay::relay::host_from_url(&relay.relay_url).is_err()
        {
            errors.push(CommandError::field(
                "invalid_relay_url",
                "Relay URL must use HTTPS",
                format!("relayConf.relayInstances.{index}.relayUrl"),
            ));
        }
        if draft.relay_conf.enable && secret_id_from_reference(&relay.relay_key).is_err() {
            errors.push(CommandError::field(
                "secret_reference_required",
                "Enabled relays require a key stored in the credential vault",
                format!("relayConf.relayInstances.{index}.relayKey"),
            ));
        }
    }
    if draft.obfs_conf.bind_addr.parse::<SocketAddr>().is_err() {
        errors.push(CommandError::field(
            "invalid_obfs_listener",
            "Obfuscation listener must be an IP address and port",
            "obfsConf.bindAddr",
        ));
    }
    if draft.obfs_conf.enable && draft.obfs_conf.keys.is_empty() {
        errors.push(CommandError::field(
            "secret_reference_required",
            "Enabled obfuscation requires at least one vault key",
            "obfsConf.keys",
        ));
    }
    if draft.obfs_conf.enable {
        for (index, key) in draft.obfs_conf.keys.iter().enumerate() {
            if secret_id_from_reference(key).is_err() {
                errors.push(CommandError::field(
                    "secret_reference_required",
                    "Obfuscation keys must be stored in the credential vault",
                    format!("obfsConf.keys.{index}"),
                ));
            }
        }
    }
    if draft
        .record_history_conf
        .as_ref()
        .is_some_and(|history| history.lines == 0)
    {
        errors.push(CommandError::field(
            "invalid_history_lines",
            "History line retention must be greater than zero",
            "recordHistoryConf.lines",
        ));
    }
    errors
}

fn valid_resolver(resolver: &str) -> bool {
    if resolver.starts_with("https://") {
        dns_relay::relay::host_from_url(resolver).is_ok()
    } else if let Some(address) = resolver.strip_prefix("quic://") {
        address.parse::<SocketAddr>().is_ok()
    } else {
        resolver.parse::<SocketAddr>().is_ok()
    }
}

fn is_file_reference(value: &str) -> bool {
    value.starts_with('/') || value.starts_with("./") || value.starts_with("../")
}

fn valid_domain_pattern(value: &str) -> bool {
    let value = value.trim_end_matches('.');
    !value.is_empty()
        && value.contains('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'*'))
}

fn valid_redirect_target(value: &str) -> bool {
    value
        .split(',')
        .all(|address| address.trim().parse::<Ipv4Addr>().is_ok())
}

fn require_secret_references(draft: &Conf) -> Result<(), CommandError> {
    for (index, relay) in draft.relay_conf.relay_instances.iter().enumerate() {
        if !relay.relay_key.is_empty() {
            secret_id_from_reference(&relay.relay_key).map_err(|_| {
                CommandError::field(
                    "secret_reference_required",
                    "Imported secrets must use vault references",
                    format!("relayConf.relayInstances.{index}.relayKey"),
                )
            })?;
        }
    }
    for (index, key) in draft.obfs_conf.keys.iter().enumerate() {
        secret_id_from_reference(key).map_err(|_| {
            CommandError::field(
                "secret_reference_required",
                "Imported secrets must use vault references",
                format!("obfsConf.keys.{index}"),
            )
        })?;
    }
    Ok(())
}

fn reject_unknown_config_fields(config_toml: &str) -> Result<(), CommandError> {
    let value: toml::Value = toml::from_str(config_toml)
        .map_err(|error| CommandError::new("invalid_config", error.to_string()))?;
    let mut fields = Vec::new();
    collect_toml_leaf_paths(&value, "", &mut fields);
    let registry: Vec<String> = serde_json::from_str(include_str!("../../src/config-fields.json"))
        .map_err(|error| CommandError::new("field_registry_invalid", error.to_string()))?;
    if let Some(field) = fields.iter().find(|field| !registry.contains(field)) {
        Err(CommandError::field(
            "unknown_config_field",
            format!("Unknown configuration field: {field}"),
            field,
        ))
    } else {
        Ok(())
    }
}

fn collect_toml_leaf_paths(value: &toml::Value, prefix: &str, output: &mut Vec<String>) {
    match value {
        toml::Value::Table(entries) => {
            if entries.is_empty() && !prefix.is_empty() {
                output.push(prefix.into());
            }
            for (key, value) in entries {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                collect_toml_leaf_paths(value, &path, output);
            }
        }
        toml::Value::Array(entries) => {
            let path = format!("{prefix}[]");
            if entries.first().is_some_and(toml::Value::is_table) {
                for entry in entries {
                    collect_toml_leaf_paths(entry, &path, output);
                }
            } else {
                output.push(path);
            }
        }
        _ => output.push(prefix.into()),
    }
}

fn secret_id_from_reference(reference: &str) -> Result<SecretId, CommandError> {
    let id = reference.strip_prefix("vault://").ok_or_else(|| {
        CommandError::new("secret_reference_required", "Expected a vault reference")
    })?;
    SecretId::new(id)
        .map_err(|error| CommandError::new("invalid_secret_reference", error.to_string()))
}

const MAX_ACTIVITY_BYTES: u64 = 256 * 1024;

pub(crate) fn read_bounded_lines(
    paths: &[PathBuf],
    limit: usize,
) -> Result<Vec<String>, CommandError> {
    let mut lines = Vec::new();
    for path in paths {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CommandError::new("activity_unavailable", error.to_string())),
        };
        let metadata = file
            .metadata()
            .map_err(|error| CommandError::new("activity_unavailable", error.to_string()))?;
        if !metadata.is_file() {
            return Err(CommandError::new(
                "activity_unavailable",
                "Activity source is not a regular file",
            ));
        }
        let start = metadata.len().saturating_sub(MAX_ACTIVITY_BYTES);
        file.seek(SeekFrom::Start(start))
            .map_err(|error| CommandError::new("activity_unavailable", error.to_string()))?;
        let mut content = Vec::new();
        file.take(MAX_ACTIVITY_BYTES)
            .read_to_end(&mut content)
            .map_err(|error| CommandError::new("activity_unavailable", error.to_string()))?;
        let content = String::from_utf8_lossy(&content);
        let mut source_lines = content.lines();
        if start > 0 {
            source_lines.next();
        }
        lines.extend(source_lines.map(str::to_owned));
    }
    let skip = lines.len().saturating_sub(limit);
    Ok(lines.into_iter().skip(skip).collect())
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
