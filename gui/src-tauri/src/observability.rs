use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use serde::{Deserialize, Serialize};

const METRICS_ADDRESS: &str = "127.0.0.1:5053";
const IO_TIMEOUT: Duration = Duration::from_millis(800);
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataState<T> {
    pub value: Option<T>,
    pub error: Option<String>,
    pub error_kind: Option<DataErrorKind>,
}

impl<T> DataState<T> {
    fn from_result(result: Result<T, ObservabilityError>) -> Self {
        match result {
            Ok(value) => Self {
                value: Some(value),
                error: None,
                error_kind: None,
            },
            Err(error) => Self {
                value: None,
                error: Some(error.message),
                error_kind: Some(error.kind),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataErrorKind {
    ConnectionRefused,
    Unavailable,
}

struct ObservabilityError {
    kind: DataErrorKind,
    message: String,
}

impl ObservabilityError {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: DataErrorKind::Unavailable,
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for ObservabilityError {
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: if error.kind() == std::io::ErrorKind::ConnectionRefused {
                DataErrorKind::ConnectionRefused
            } else {
                DataErrorKind::Unavailable
            },
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Metrics {
    pub total_req: u64,
    pub resolved_count: u64,
    pub failed_count: u64,
    pub timeout_count: u64,
    pub redirect_count: u64,
    pub drop_count: u64,
    pub cached_count: u64,
    pub relay_resolved_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObservabilitySnapshot {
    pub health: DataState<bool>,
    pub metrics: DataState<Metrics>,
}

#[tauri::command]
pub async fn get_observability() -> ObservabilitySnapshot {
    let health_task = tauri::async_runtime::spawn_blocking(check_health);
    let metrics_task = tauri::async_runtime::spawn_blocking(read_metrics);
    let health = health_task.await;
    let metrics = metrics_task.await;
    snapshot_from_results(
        health
            .map_err(|error| ObservabilityError::unavailable(error.to_string()))
            .and_then(|result| result),
        metrics
            .map_err(|error| ObservabilityError::unavailable(error.to_string()))
            .and_then(|result| result),
    )
}

fn snapshot_from_results(
    health: Result<bool, ObservabilityError>,
    metrics: Result<Metrics, ObservabilityError>,
) -> ObservabilitySnapshot {
    ObservabilitySnapshot {
        health: DataState::from_result(health),
        metrics: DataState::from_result(metrics),
    }
}

fn check_health() -> Result<bool, ObservabilityError> {
    fetch_local("/health").map(|body| body.trim() == "ok")
}

fn read_metrics() -> Result<Metrics, ObservabilityError> {
    serde_json::from_str(&fetch_local("/metrics")?)
        .map_err(|error| ObservabilityError::unavailable(error.to_string()))
}

fn fetch_local(path: &str) -> Result<String, ObservabilityError> {
    let address: SocketAddr =
        METRICS_ADDRESS
            .parse()
            .map_err(|error: std::net::AddrParseError| {
                ObservabilityError::unavailable(error.to_string())
            })?;
    let mut stream =
        TcpStream::connect_timeout(&address, IO_TIMEOUT).map_err(ObservabilityError::from)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(ObservabilityError::from)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(ObservabilityError::from)?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .map_err(ObservabilityError::from)?;
    let mut response = String::new();
    stream
        .take(MAX_RESPONSE_BYTES)
        .read_to_string(&mut response)
        .map_err(ObservabilityError::from)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| ObservabilityError::unavailable("invalid local metrics response"))?;
    if !headers
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("HTTP/1.1 200 ") || line.starts_with("HTTP/1.0 200 "))
    {
        return Err(ObservabilityError::unavailable(
            "local metrics endpoint returned an error",
        ));
    }
    Ok(body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_failure_does_not_erase_independent_health() {
        let snapshot = snapshot_from_results(
            Ok(true),
            Err(ObservabilityError::unavailable("metrics offline")),
        );

        assert_eq!(snapshot.health.value, Some(true));
        assert!(snapshot.health.error.is_none());
        assert!(snapshot.metrics.value.is_none());
        assert_eq!(snapshot.metrics.error.as_deref(), Some("metrics offline"));
        assert_eq!(
            snapshot.metrics.error_kind,
            Some(DataErrorKind::Unavailable)
        );
    }

    #[test]
    fn connection_refused_has_a_stable_error_kind() {
        let state = DataState::<bool>::from_result(Err(ObservabilityError::from(
            std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        )));

        assert_eq!(state.error_kind, Some(DataErrorKind::ConnectionRefused));
    }
}
