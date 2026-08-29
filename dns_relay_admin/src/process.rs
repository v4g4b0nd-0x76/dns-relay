use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use crate::{AdminError, apply::CommandRunner};

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn check_conf(&self, binary: &Path, config: &Path) -> Result<(), AdminError> {
        let status = Command::new(binary)
            .arg("--conf")
            .arg(config)
            .arg("check-conf")
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(AdminError::Operation(format!(
                "dns_relay check-conf exited with {status}"
            )))
        }
    }

    fn wait_for_health(&self, timeout: Duration) -> Result<(), AdminError> {
        let deadline = Instant::now() + timeout;
        let address = SocketAddr::from(([127, 0, 0, 1], 5053));
        while Instant::now() < deadline {
            if health_check(address).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(AdminError::Operation(
            "service health check timed out".into(),
        ))
    }
}

fn health_check(address: SocketAddr) -> Result<(), AdminError> {
    let timeout = Duration::from_millis(500);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200") && response.ends_with("ok") {
        Ok(())
    } else {
        Err(AdminError::Operation("service is unhealthy".into()))
    }
}
