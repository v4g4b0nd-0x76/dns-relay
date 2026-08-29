use std::{cell::Cell, fs, path::Path, time::Duration};

#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, fs::symlink};
use tempfile::tempdir;
use uuid::Uuid;

use crate::{
    AdminAction, AdminError, AdminRequest, PlatformPaths,
    apply::{CommandRunner, ServiceManager, ServiceStatus, apply_config, staging_path},
    parse_request_id,
    paths::read_request_at,
};

const REQUEST_ID: &str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";

#[test]
fn action_rejects_unknown_variants() {
    let error = serde_json::from_str::<AdminRequest>(&format!(
        r#"{{"id":"{REQUEST_ID}","action":"shell"}}"#
    ))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn status_request_parses() {
    let request = serde_json::from_str::<AdminRequest>(&format!(
        r#"{{"id":"{REQUEST_ID}","action":"status"}}"#
    ))
    .unwrap();

    assert_eq!(request.id, Uuid::parse_str(REQUEST_ID).unwrap());
    assert_eq!(request.action, AdminAction::Status);
}

#[test]
fn malformed_request_id_is_rejected() {
    assert!(parse_request_id("../request").is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_rejects_symlinks_and_wrong_parent() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    let outside = root.path().join("outside");
    fs::create_dir_all(&requests).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let target = outside.join(format!("{id}.json"));
    fs::write(&target, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

    assert!(read_request_at(&target, &requests, id).is_err());

    let link = requests.join(format!("{id}.json"));
    symlink(&target, &link).unwrap();
    assert!(read_request_at(&link, &requests, id).is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_requires_mode_0600() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    fs::create_dir_all(&requests).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let path = requests.join(format!("{id}.json"));
    fs::write(&path, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(read_request_at(&path, &requests, id).is_err());
}

#[cfg(unix)]
#[test]
fn request_reader_accepts_an_owned_0600_status_request() {
    let root = tempdir().unwrap();
    let requests = root.path().join("requests");
    fs::create_dir_all(&requests).unwrap();

    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let path = requests.join(format!("{id}.json"));
    fs::write(&path, format!(r#"{{"id":"{id}","action":"status"}}"#)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(
        read_request_at(&path, &requests, id).unwrap().action,
        AdminAction::Status
    );
}

struct FakeRunner {
    check_fails: Cell<bool>,
    health_fails: Cell<bool>,
    checks: Cell<usize>,
    health_checks: Cell<usize>,
}

impl FakeRunner {
    fn healthy() -> Self {
        Self {
            check_fails: Cell::new(false),
            health_fails: Cell::new(false),
            checks: Cell::new(0),
            health_checks: Cell::new(0),
        }
    }
}

impl CommandRunner for FakeRunner {
    fn check_conf(&self, _binary: &Path, _config: &Path) -> Result<(), AdminError> {
        self.checks.set(self.checks.get() + 1);
        if self.check_fails.get() {
            Err(AdminError::Operation("check-conf failed".into()))
        } else {
            Ok(())
        }
    }

    fn wait_for_health(&self, _timeout: Duration) -> Result<(), AdminError> {
        self.health_checks.set(self.health_checks.get() + 1);
        if self.health_fails.get() {
            Err(AdminError::Operation("health check failed".into()))
        } else {
            Ok(())
        }
    }
}

struct FakeService {
    state: Cell<ServiceStatus>,
    restart_fails: Cell<bool>,
    restarts: Cell<usize>,
}

impl FakeService {
    fn running() -> Self {
        Self {
            state: Cell::new(ServiceStatus::Running),
            restart_fails: Cell::new(false),
            restarts: Cell::new(0),
        }
    }
}

impl ServiceManager for FakeService {
    fn status(&self) -> Result<ServiceStatus, AdminError> {
        Ok(self.state.get())
    }

    fn start(&self) -> Result<(), AdminError> {
        self.state.set(ServiceStatus::Running);
        Ok(())
    }

    fn stop(&self) -> Result<(), AdminError> {
        self.state.set(ServiceStatus::Stopped);
        Ok(())
    }

    fn restart(&self) -> Result<(), AdminError> {
        self.restarts.set(self.restarts.get() + 1);
        if self.restart_fails.replace(false) {
            self.state.set(ServiceStatus::Stopped);
            Err(AdminError::Operation("restart failed".into()))
        } else {
            self.state.set(ServiceStatus::Running);
            Ok(())
        }
    }
}

struct ApplyFixture {
    _root: tempfile::TempDir,
    paths: PlatformPaths,
    runner: FakeRunner,
    service: FakeService,
}

impl ApplyFixture {
    fn running_with_config(config: &str) -> Self {
        let root = tempdir().unwrap();
        let paths = PlatformPaths::for_test(root.path());
        fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
        fs::write(&paths.config, config).unwrap();
        Self {
            _root: root,
            paths,
            runner: FakeRunner::healthy(),
            service: FakeService::running(),
        }
    }

    fn live_config(&self) -> String {
        fs::read_to_string(&self.paths.config).unwrap()
    }

    fn apply(&self, config: &str, restart: bool) -> Result<(), AdminError> {
        apply_config(config, restart, &self.paths, &self.service, &self.runner)
    }
}

const VALID_CONFIG: &str = "drop_list = []\nredirect_list = []\nresolvers = []\n";

#[test]
fn invalid_toml_does_not_replace_live_config() {
    let fixture = ApplyFixture::running_with_config("old");

    assert!(fixture.apply("not = [valid", true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert_eq!(fixture.runner.checks.get(), 0);
}

#[test]
fn failed_check_conf_does_not_replace_live_config() {
    let fixture = ApplyFixture::running_with_config("old");
    fixture.runner.check_fails.set(true);

    assert!(fixture.apply(VALID_CONFIG, true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert!(!staging_path(&fixture.paths.config).exists());
}

#[test]
fn staging_write_failure_does_not_replace_live_config() {
    let fixture = ApplyFixture::running_with_config("old");
    fs::create_dir(staging_path(&fixture.paths.config)).unwrap();

    assert!(fixture.apply(VALID_CONFIG, true).is_err());
    assert_eq!(fixture.live_config(), "old");
}

#[test]
fn restart_failure_restores_config_and_running_state() {
    let fixture = ApplyFixture::running_with_config("old");
    fixture.service.restart_fails.set(true);

    assert!(fixture.apply(VALID_CONFIG, true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert_eq!(fixture.service.state.get(), ServiceStatus::Running);
}

#[test]
fn failed_health_check_restores_config_and_running_state() {
    let fixture = ApplyFixture::running_with_config("old");
    fixture.runner.health_fails.set(true);

    assert!(fixture.apply(VALID_CONFIG, true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert_eq!(fixture.service.state.get(), ServiceStatus::Running);
}

#[test]
fn rule_only_apply_uses_hot_reload_without_restarting() {
    let fixture = ApplyFixture::running_with_config("old");

    fixture.apply(VALID_CONFIG, false).unwrap();

    assert_eq!(fixture.live_config(), VALID_CONFIG);
    assert_eq!(fixture.service.restarts.get(), 0);
    assert_eq!(fixture.runner.health_checks.get(), 1);
    assert_eq!(fs::read_to_string(&fixture.paths.backup).unwrap(), "old");
}

#[test]
fn restarting_apply_restarts_once_and_keeps_rollback_copy() {
    let fixture = ApplyFixture::running_with_config("old");

    fixture.apply(VALID_CONFIG, true).unwrap();

    assert_eq!(fixture.live_config(), VALID_CONFIG);
    assert_eq!(fixture.service.restarts.get(), 1);
    assert_eq!(fixture.runner.health_checks.get(), 1);
    assert_eq!(fs::read_to_string(&fixture.paths.backup).unwrap(), "old");
}
