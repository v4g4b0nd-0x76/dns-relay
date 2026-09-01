use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{AdminError, PlatformPaths};

pub(crate) const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Stopped,
}

pub trait CommandRunner {
    fn check_conf(&self, binary: &Path, config: &Path) -> Result<(), AdminError>;
    fn wait_for_health(&self, timeout: Duration) -> Result<(), AdminError>;
}

pub trait ServiceManager {
    fn status(&self) -> Result<ServiceStatus, AdminError>;
    fn start(&self) -> Result<(), AdminError>;
    fn stop(&self) -> Result<(), AdminError>;
    fn restart(&self) -> Result<(), AdminError>;
}

pub fn staging_path(config: &Path) -> PathBuf {
    let name = config
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("conf.toml");
    config.with_file_name(format!(".{name}.staged"))
}

pub fn apply_config(
    config_toml: &str,
    restart: bool,
    paths: &PlatformPaths,
    service: &impl ServiceManager,
    runner: &impl CommandRunner,
) -> Result<(), AdminError> {
    let parsed = parse_config(config_toml)?;
    let has_http_health = exposes_http_health(&parsed);

    let prior_status = service.status()?;
    let staged = staging_path(&paths.config);
    if let Err(error) = write_staged(&staged, config_toml) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    if let Err(error) = runner.check_conf(&paths.installed_binary, &staged) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    let had_live_config = paths.config.exists();
    let backup_result = if had_live_config {
        backup_config(paths)
    } else {
        Ok(())
    };
    if let Err(error) = backup_result {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staged, &paths.config) {
        let _ = fs::remove_file(&staged);
        return Err(error.into());
    }
    if let Err(error) = sync_parent(&paths.config) {
        rollback(paths, had_live_config, prior_status, service)?;
        return Err(error);
    }

    let applied = match (prior_status, restart) {
        (ServiceStatus::Running, true) => service
            .restart()
            .and_then(|()| wait_for_http_health(has_http_health, runner)),
        (ServiceStatus::Running, false) => wait_for_http_health(has_http_health, runner),
        (ServiceStatus::Stopped, _) => Ok(()),
    };

    if let Err(error) = applied {
        rollback(paths, had_live_config, prior_status, service)?;
        return Err(error);
    }
    Ok(())
}

fn wait_for_http_health(
    has_http_health: bool,
    runner: &impl CommandRunner,
) -> Result<(), AdminError> {
    if has_http_health {
        runner.wait_for_health(HEALTH_TIMEOUT)
    } else {
        Ok(())
    }
}

pub(crate) fn config_exposes_http_health(config_toml: &str) -> Result<bool, AdminError> {
    parse_config(config_toml).map(|config| exposes_http_health(&config))
}

fn parse_config(config_toml: &str) -> Result<toml::Value, AdminError> {
    toml::from_str::<toml::Value>(config_toml)
        .map_err(|error| AdminError::Operation(format!("invalid config TOML: {error}")))
}

fn exposes_http_health(config: &toml::Value) -> bool {
    config
        .get("metric_conf")
        .and_then(|metrics| metrics.get("enable"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
        && config
            .get("metric_conf")
            .and_then(|metrics| metrics.get("report_type"))
            .and_then(toml::Value::as_str)
            == Some("http")
}

fn backup_config(paths: &PlatformPaths) -> Result<(), AdminError> {
    fs::copy(&paths.config, &paths.backup)?;
    set_private_permissions(&paths.backup)?;
    fs::File::open(&paths.backup)?.sync_all()?;
    Ok(())
}

fn write_staged(path: &Path, content: &str) -> Result<(), AdminError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn rollback(
    paths: &PlatformPaths,
    had_live_config: bool,
    prior_status: ServiceStatus,
    service: &impl ServiceManager,
) -> Result<(), AdminError> {
    if had_live_config {
        fs::rename(&paths.backup, &paths.config)?;
    } else if paths.config.exists() {
        fs::remove_file(&paths.config)?;
    }
    sync_parent(&paths.config)?;

    match prior_status {
        ServiceStatus::Running => service.restart().or_else(|_| service.start()),
        ServiceStatus::Stopped => service.stop(),
    }
}

fn set_private_permissions(path: &Path) -> Result<(), AdminError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), AdminError> {
    #[cfg(unix)]
    fs::File::open(
        path.parent()
            .ok_or_else(|| AdminError::Operation("config path has no parent directory".into()))?,
    )?
    .sync_all()?;
    Ok(())
}
