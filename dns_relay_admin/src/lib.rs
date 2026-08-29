pub mod apply;
mod paths;
pub mod process;

use std::{fmt, fs, io};

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use paths::PlatformPaths;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdminAction {
    Status,
    Install {
        config_toml: String,
        expected_binary_sha256: String,
    },
    Update {
        expected_binary_sha256: String,
    },
    Repair {
        expected_binary_sha256: String,
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
            paths.read_request(id)?;
            let response = AdminResponse {
                id,
                ok: false,
                message: AdminError::Unsupported.to_string(),
            };
            paths.write_response(&response)?;
            fs::remove_file(paths.request_path(id))?;
            Ok(())
        }
        Command::ServiceRun => Err(AdminError::Unsupported),
    }
}

#[cfg(test)]
mod tests;
