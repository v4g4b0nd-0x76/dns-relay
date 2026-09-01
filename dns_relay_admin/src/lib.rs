pub mod apply;
mod paths;
pub mod platform;
pub mod process;

use std::{fmt, fs, io, path::Path};

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use apply::{
    CommandRunner, HEALTH_TIMEOUT, ServiceManager, ServiceStatus, apply_config,
    config_exposes_http_health,
};
use process::SystemCommandRunner;

pub use paths::PlatformPaths;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdminAction {
    Status,
    ReadConfig,
    Install {
        config_toml: String,
        expected_binary_sha256: String,
    },
    Update {
        expected_binary_sha256: String,
    },
    Repair {
        expected_binary_sha256: String,
        config_toml: String,
    },
    Uninstall,
    Start,
    Stop,
    Restart,
    ApplyConfig {
        config_toml: String,
        restart: bool,
    },
}

impl AdminAction {
    pub fn zeroize_sensitive(&mut self) {
        match self {
            Self::Install { config_toml, .. }
            | Self::Repair { config_toml, .. }
            | Self::ApplyConfig { config_toml, .. } => {
                config_toml.zeroize();
            }
            _ => {}
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminRequest {
    pub id: Uuid,
    #[serde(flatten)]
    pub action: AdminAction,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminResponse {
    pub id: Uuid,
    pub ok: bool,
    pub message: String,
}

impl Drop for AdminResponse {
    fn drop(&mut self) {
        self.message.zeroize();
    }
}

#[derive(Debug)]
pub enum AdminError {
    InvalidRequestId,
    InvalidRequestFile(String),
    Io(io::Error),
    Json(serde_json::Error),
    Operation(String),
    Unsupported,
}

impl fmt::Display for AdminError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestId => formatter.write_str("request ID must be a UUID"),
            Self::InvalidRequestFile(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "request I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "invalid request JSON: {error}"),
            Self::Operation(message) => formatter.write_str(message),
            Self::Unsupported => formatter.write_str("service management is not implemented"),
        }
    }
}

impl std::error::Error for AdminError {}

impl From<io::Error> for AdminError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AdminError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn parse_request_id(value: &str) -> Result<Uuid, AdminError> {
    Uuid::parse_str(value).map_err(|_| AdminError::InvalidRequestId)
}

#[derive(Parser)]
#[command(name = "dns_relay_admin")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Request {
        #[arg(long)]
        request_id: String,
    },
    ServiceRun,
}

pub fn run(cli: Cli) -> Result<(), AdminError> {
    match cli.command {
        Command::Request { request_id } => {
            let id = parse_request_id(&request_id)?;
            let paths = PlatformPaths::current()?;
            let request = paths.read_request(id)?;
            fs::remove_file(paths.request_path(id))?;
            let result = dispatch(request.action, &paths);
            let response = AdminResponse {
                id,
                ok: result.is_ok(),
                message: result.unwrap_or_else(|error| error.to_string()),
            };
            paths.write_response(&response)?;
            Ok(())
        }
        Command::ServiceRun => Err(AdminError::Unsupported),
    }
}

trait AdminService: ServiceManager {
    fn install(&self, config_toml: &str) -> Result<(), AdminError>;
    fn update(&self) -> Result<(), AdminError>;
    fn repair(&self, config_toml: &str) -> Result<(), AdminError>;
    fn uninstall(&self) -> Result<(), AdminError>;
    fn diagnostics(&self) -> Option<String> {
        None
    }
}

#[cfg(target_os = "macos")]
impl AdminService for platform::macos::MacosServiceManager {
    fn install(&self, config_toml: &str) -> Result<(), AdminError> {
        self.install(config_toml)
    }

    fn update(&self) -> Result<(), AdminError> {
        self.update()
    }

    fn repair(&self, config_toml: &str) -> Result<(), AdminError> {
        self.repair(config_toml)
    }

    fn uninstall(&self) -> Result<(), AdminError> {
        self.uninstall()
    }
}

#[cfg(target_os = "linux")]
impl AdminService for platform::linux::LinuxServiceManager {
    fn install(&self, config_toml: &str) -> Result<(), AdminError> {
        self.install(config_toml)
    }

    fn update(&self) -> Result<(), AdminError> {
        self.update()
    }

    fn repair(&self, config_toml: &str) -> Result<(), AdminError> {
        self.repair(config_toml)
    }

    fn uninstall(&self) -> Result<(), AdminError> {
        self.uninstall()
    }

    fn diagnostics(&self) -> Option<String> {
        platform::linux::LinuxServiceManager::diagnostics(self)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn dispatch(action: AdminAction, paths: &PlatformPaths) -> Result<String, AdminError> {
    #[cfg(target_os = "macos")]
    let service = platform::macos::MacosServiceManager::new(paths.clone());
    #[cfg(target_os = "linux")]
    let service = platform::linux::LinuxServiceManager::new(paths.clone());
    execute_action(action, paths, &service, &SystemCommandRunner)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn dispatch(_action: AdminAction, _paths: &PlatformPaths) -> Result<String, AdminError> {
    Err(AdminError::Unsupported)
}

fn execute_action(
    action: AdminAction,
    paths: &PlatformPaths,
    service: &impl AdminService,
    runner: &impl CommandRunner,
) -> Result<String, AdminError> {
    match action {
        AdminAction::Status => Ok(match service.status()? {
            ServiceStatus::Running => "running",
            ServiceStatus::Stopped => "stopped",
        }
        .into()),
        AdminAction::ReadConfig => fs::read_to_string(&paths.config).map_err(Into::into),
        AdminAction::Install {
            config_toml,
            expected_binary_sha256,
        } => {
            let config_toml = Zeroizing::new(config_toml);
            verify_bundled_resolver(&expected_binary_sha256)?;
            service
                .install(&config_toml)
                .map_err(|error| service_error(error, service))?;
            wait_for_configured_health(&config_toml, service, runner)?;
            Ok("installed".into())
        }
        AdminAction::Update {
            expected_binary_sha256,
        } => {
            verify_bundled_resolver(&expected_binary_sha256)?;
            service
                .update()
                .map_err(|error| service_error(error, service))?;
            wait_for_live_config_health(paths, service, runner)?;
            Ok("updated".into())
        }
        AdminAction::Repair {
            expected_binary_sha256,
            config_toml,
        } => {
            let config_toml = Zeroizing::new(config_toml);
            verify_bundled_resolver(&expected_binary_sha256)?;
            service
                .repair(&config_toml)
                .map_err(|error| service_error(error, service))?;
            wait_for_configured_health(&config_toml, service, runner)?;
            Ok("repaired".into())
        }
        AdminAction::Uninstall => {
            service.uninstall()?;
            Ok("uninstalled".into())
        }
        AdminAction::Start => {
            service
                .start()
                .map_err(|error| service_error(error, service))?;
            wait_for_live_config_health(paths, service, runner)?;
            Ok("started".into())
        }
        AdminAction::Stop => {
            service.stop()?;
            Ok("stopped".into())
        }
        AdminAction::Restart => {
            service
                .restart()
                .map_err(|error| service_error(error, service))?;
            wait_for_live_config_health(paths, service, runner)?;
            Ok("restarted".into())
        }
        AdminAction::ApplyConfig {
            config_toml,
            restart,
        } => {
            let config_toml = Zeroizing::new(config_toml);
            apply_config(&config_toml, restart, paths, service, runner)
                .map_err(|error| service_error(error, service))?;
            Ok("applied".into())
        }
    }
}

fn wait_for_service_health(
    service: &impl AdminService,
    runner: &impl CommandRunner,
) -> Result<(), AdminError> {
    runner
        .wait_for_health(HEALTH_TIMEOUT)
        .map_err(|error| service_error(error, service))
}

fn wait_for_configured_health(
    config_toml: &str,
    service: &impl AdminService,
    runner: &impl CommandRunner,
) -> Result<(), AdminError> {
    if config_exposes_http_health(config_toml)? {
        wait_for_service_health(service, runner)
    } else {
        Ok(())
    }
}

fn wait_for_live_config_health(
    paths: &PlatformPaths,
    service: &impl AdminService,
    runner: &impl CommandRunner,
) -> Result<(), AdminError> {
    let config_toml = fs::read_to_string(&paths.config)?;
    wait_for_configured_health(&config_toml, service, runner)
}

fn service_error(error: AdminError, service: &impl AdminService) -> AdminError {
    match service.diagnostics() {
        Some(diagnostics) => AdminError::Operation(format!("{error}\n\n{diagnostics}")),
        None => error,
    }
}

fn verify_bundled_resolver(expected: &str) -> Result<(), AdminError> {
    let helper = std::env::current_exe()?;
    let resolver = helper
        .parent()
        .ok_or_else(|| AdminError::Operation("admin helper path has no parent".into()))?
        .join("dns_relay");
    verify_sha256_file(&resolver, expected)
}

fn verify_sha256_file(path: &Path, expected: &str) -> Result<(), AdminError> {
    verify_sha256(&fs::read(path)?, expected)
}

pub fn sha256_file(path: &Path) -> Result<String, AdminError> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

pub(crate) fn verify_sha256(content: &[u8], expected: &str) -> Result<(), AdminError> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AdminError::Operation(
            "expected binary SHA-256 is invalid".into(),
        ));
    }
    let actual = format!("{:x}", Sha256::digest(content));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AdminError::Operation(
            "bundled resolver SHA-256 does not match".into(),
        ))
    }
}

#[cfg(test)]
mod tests;
