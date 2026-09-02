use std::{cell::Cell, fs, path::Path, time::Duration};

#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, fs::symlink};
use tempfile::tempdir;
use uuid::Uuid;

#[cfg(unix)]
use crate::platform::{
    CommandSpec,
    linux::{
        LinuxServiceManager, elevation_command_for, linux_invoking_uid_from,
        linux_service_diagnostics, systemctl_service_status,
    },
};
use crate::{
    AdminAction, AdminError, AdminRequest, PlatformPaths,
    apply::{CommandRunner, ServiceManager, ServiceStatus, apply_config, staging_path},
    parse_request_id,
    paths::read_request_at,
    process::SystemCommandRunner,
    sha256_file, verify_sha256,
};

#[cfg(target_os = "macos")]
use crate::paths::{invoking_uid, user_home};
#[cfg(target_os = "macos")]
use crate::platform::macos::{MacosServiceManager, elevation_command};

const REQUEST_ID: &str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";

#[cfg(unix)]
#[test]
fn check_conf_runs_beside_the_staged_config() {
    let root = tempdir().unwrap();
    let binary = root.path().join("check-conf");
    let config = root.path().join("conf.toml");
    fs::write(
        &binary,
        "#!/bin/sh\nexpected=$(cd \"$(dirname \"$2\")\" && pwd -P)\ntest \"$(pwd -P)\" = \"$expected\"\n",
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&config, "drop_list = []").unwrap();

    SystemCommandRunner.check_conf(&binary, &config).unwrap();
}

#[cfg(unix)]
#[test]
fn check_conf_reports_child_stderr() {
    let root = tempdir().unwrap();
    let binary = root.path().join("check-conf");
    let config = root.path().join("conf.toml");
    fs::write(&binary, "#!/bin/sh\necho 'panic detail' >&2\nexit 101\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&config, "drop_list = []").unwrap();

    let error = SystemCommandRunner
        .check_conf(&binary, &config)
        .unwrap_err();

    assert!(error.to_string().contains("panic detail"));
}

#[test]
#[cfg(unix)]
fn linux_systemctl_commands_are_closed_and_exact() {
    let linux = LinuxServiceManager::new(PlatformPaths::for_test(Path::new("/tmp/linux-test")));

    assert_eq!(
        linux.daemon_reload_command(),
        CommandSpec::new("/usr/bin/systemctl", ["daemon-reload"])
    );
    assert_eq!(
        linux.enable_command(),
        CommandSpec::new("/usr/bin/systemctl", ["enable", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.disable_command(),
        CommandSpec::new("/usr/bin/systemctl", ["disable", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.start_command(),
        CommandSpec::new("/usr/bin/systemctl", ["start", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.stop_command(),
        CommandSpec::new("/usr/bin/systemctl", ["stop", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.restart_command(),
        CommandSpec::new("/usr/bin/systemctl", ["restart", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.status_command(),
        CommandSpec::new("/usr/bin/systemctl", ["is-active", "dns-relay-gui.service"])
    );
    assert_eq!(
        linux.show_command(),
        CommandSpec::new(
            "/usr/bin/systemctl",
            [
                "show",
                "dns-relay-gui.service",
                "-p",
                "ActiveState",
                "-p",
                "SubState",
                "-p",
                "Result",
                "-p",
                "NRestarts",
                "-p",
                "ExecMainCode",
                "-p",
                "ExecMainStatus",
                "--no-pager",
            ]
        )
    );
    assert_eq!(
        linux.journal_command(17),
        CommandSpec::new(
            "/usr/bin/journalctl",
            [
                "-u",
                "dns-relay-gui.service",
                "-n",
                "17",
                "--no-pager",
                "--output=cat",
            ]
        )
    );
}

#[test]
#[cfg(unix)]
fn linux_restart_loop_is_not_reported_as_stopped() {
    assert_eq!(
        systemctl_service_status(true, "active\n", "").unwrap(),
        ServiceStatus::Running
    );
    assert_eq!(
        systemctl_service_status(false, "inactive\n", "").unwrap(),
        ServiceStatus::Stopped
    );
    assert!(systemctl_service_status(false, "activating\n", "").is_err());
}

#[test]
#[cfg(unix)]
fn linux_service_diagnostics_include_systemd_state_and_journal() {
    let message = linux_service_diagnostics(
        "ActiveState=activating\nSubState=auto-restart\nResult=exit-code\nNRestarts=42\nExecMainCode=1\nExecMainStatus=1\n",
        "dns_relay[123]: Error: NoHealthyResolvers\nsystemd[1]: Failed with result 'exit-code'.\n",
    )
    .unwrap();

    assert!(message.contains("ActiveState=activating"));
    assert!(message.contains("SubState=auto-restart"));
    assert!(message.contains("NRestarts=42"));
    assert!(message.contains("NoHealthyResolvers"));
}

#[test]
#[cfg(unix)]
fn linux_pkexec_passes_only_the_fixed_helper_and_uuid() {
    let helper = Path::new("/opt/dns-relay-gui/dns_relay_admin");
    let command = elevation_command_for(helper, REQUEST_ID, true).unwrap();

    assert_eq!(
        command,
        CommandSpec::new(
            "/usr/bin/pkexec",
            [
                "/opt/dns-relay-gui/dns_relay_admin",
                "request",
                "--request-id",
                REQUEST_ID,
            ]
        )
    );
    assert!(elevation_command_for(helper, "$(touch /tmp/no)", true).is_err());
}

#[test]
#[cfg(unix)]
fn linux_missing_pkexec_returns_copyable_sudo_fallback() {
    let error = elevation_command_for(
        Path::new("/opt/dns-relay-gui/dns_relay_admin"),
        REQUEST_ID,
        false,
    )
    .unwrap_err();

    assert!(error.to_string().contains(&format!(
        "sudo /opt/dns-relay-gui/dns_relay_admin request --request-id {REQUEST_ID}"
    )));
}

#[test]
#[cfg(unix)]
fn linux_elevation_uses_the_original_user_id() {
    assert_eq!(linux_invoking_uid_from(501, None, None).unwrap(), 501);
    assert_eq!(
        linux_invoking_uid_from(0, Some("1000"), None).unwrap(),
        1000
    );
    assert_eq!(
        linux_invoking_uid_from(0, None, Some("1001")).unwrap(),
        1001
    );
    assert!(linux_invoking_uid_from(0, Some("root"), None).is_err());
    assert!(linux_invoking_uid_from(0, None, None).is_err());
}

#[cfg(unix)]
#[test]
fn linux_install_payload_writes_hardened_fixed_files() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    let resolver = root.path().join("dns_relay");
    let helper = root.path().join("dns_relay_admin");
    fs::write(&resolver, "resolver").unwrap();
    fs::write(&helper, "helper").unwrap();

    LinuxServiceManager::new(paths.clone())
        .install_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();

    assert_eq!(fs::read_to_string(&paths.config).unwrap(), VALID_CONFIG);
    assert_eq!(
        fs::metadata(&paths.config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let service = fs::read_to_string(&paths.service_definition).unwrap();
    assert!(service.contains("AmbientCapabilities=CAP_NET_BIND_SERVICE"));
    assert!(service.contains("NoNewPrivileges=true"));
    assert!(service.contains("ProtectSystem=strict"));
    assert!(service.contains("ReadOnlyPaths=/opt/dns-relay-gui"));
    let policy = fs::read_to_string(&paths.authorization_policy).unwrap();
    assert!(policy.contains("/opt/dns-relay-gui/dns_relay_admin"));
}

#[cfg(unix)]
#[test]
fn linux_repair_restores_assets_without_overwriting_config() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, "keep-me").unwrap();
    let resolver = root.path().join("dns_relay");
    let helper = root.path().join("dns_relay_admin");
    fs::write(&resolver, "resolver").unwrap();
    fs::write(&helper, "helper").unwrap();

    LinuxServiceManager::new(paths.clone())
        .repair_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();

    assert_eq!(fs::read_to_string(&paths.config).unwrap(), "keep-me");
    assert!(paths.service_definition.is_file());
    assert!(paths.authorization_policy.is_file());

    fs::remove_file(&paths.config).unwrap();
    fs::remove_file(&paths.service_definition).unwrap();
    LinuxServiceManager::new(paths.clone())
        .repair_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();
    assert_eq!(fs::read_to_string(&paths.config).unwrap(), VALID_CONFIG);
    assert!(paths.service_definition.is_file());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_launchd_commands_use_only_fixed_paths_and_label() {
    let macos = MacosServiceManager::new(PlatformPaths::current().unwrap());

    assert_eq!(
        macos.bootout_command(),
        CommandSpec::new("/bin/launchctl", ["bootout", "system/com.dns-relay.gui"])
    );
    assert_eq!(
        macos.enable_command(),
        CommandSpec::new("/bin/launchctl", ["enable", "system/com.dns-relay.gui"])
    );
    assert_eq!(
        macos.bootstrap_command(),
        CommandSpec::new(
            "/bin/launchctl",
            [
                "bootstrap",
                "system",
                "/Library/LaunchDaemons/com.dns-relay.gui.plist"
            ]
        )
    );
    assert_eq!(
        macos.restart_command(),
        CommandSpec::new(
            "/bin/launchctl",
            ["kickstart", "-k", "system/com.dns-relay.gui"]
        )
    );
    assert_eq!(
        macos.status_command(),
        CommandSpec::new("/bin/launchctl", ["print", "system/com.dns-relay.gui"])
    );
    assert_eq!(
        macos.activation_commands(),
        vec![
            macos.enable_command(),
            macos.bootstrap_command(),
            macos.restart_command(),
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_launchd_status_requires_a_running_job() {
    use crate::platform::macos::launchctl_service_status;

    assert_eq!(
        launchctl_service_status(true, "state = running\n", "").unwrap(),
        ServiceStatus::Running
    );
    assert!(
        launchctl_service_status(true, "state = spawn scheduled\nlast exit code = 1\n", "")
            .is_err()
    );
    assert_eq!(
        launchctl_service_status(false, "", "Could not find service").unwrap(),
        ServiceStatus::Stopped
    );
    assert!(launchctl_service_status(false, "", "Operation not permitted").is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_elevation_keeps_helper_path_and_uuid_out_of_applescript_source() {
    let helper = Path::new("/Applications/DNS Relay's 日本.app/Contents/MacOS/dns_relay_admin");
    let command = elevation_command(helper, REQUEST_ID).unwrap();

    assert_eq!(command.program, Path::new("/usr/bin/osascript"));
    assert_eq!(
        command.args,
        vec![
            "-e",
            "on run argv\nset helperPath to item 1 of argv\nset requestId to item 2 of argv\ndo shell script quoted form of helperPath & \" request --request-id \" & quoted form of requestId with administrator privileges\nend run",
            "/Applications/DNS Relay's 日本.app/Contents/MacOS/dns_relay_admin",
            REQUEST_ID,
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_elevation_rejects_non_uuid_request_ids() {
    assert!(
        elevation_command(
            Path::new("/Applications/DNS Relay.app/dns_relay_admin"),
            "../shell"
        )
        .is_err()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_elevated_paths_resolve_to_the_console_user() {
    use std::os::unix::fs::MetadataExt;

    let effective_uid = unsafe { libc::geteuid() };
    let expected_uid = if effective_uid == 0 {
        fs::metadata("/dev/console").unwrap().uid()
    } else {
        effective_uid
    };

    assert_eq!(invoking_uid().unwrap(), expected_uid);
    assert!(user_home().unwrap().is_absolute());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_install_payload_atomically_writes_private_fixed_files() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    let resolver = root.path().join("bundled dns_relay");
    let helper = root.path().join("bundled dns_relay_admin");
    fs::write(&resolver, "resolver-v1").unwrap();
    fs::write(&helper, "helper-v1").unwrap();
    let macos = MacosServiceManager::new(paths.clone());

    macos
        .install_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();

    assert_eq!(
        fs::read_to_string(&paths.installed_binary).unwrap(),
        "resolver-v1"
    );
    assert_eq!(
        fs::read_to_string(&paths.admin_binary).unwrap(),
        "helper-v1"
    );
    assert_eq!(fs::read_to_string(&paths.config).unwrap(), VALID_CONFIG);
    assert!(
        fs::read_to_string(&paths.service_definition)
            .unwrap()
            .contains("<string>com.dns-relay.gui</string>")
    );
    assert_eq!(
        fs::metadata(&paths.config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&paths.service_definition)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_update_payload_preserves_existing_config() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, "keep-me").unwrap();
    let resolver = root.path().join("dns_relay");
    let helper = root.path().join("dns_relay_admin");
    fs::write(&resolver, "resolver-v2").unwrap();
    fs::write(&helper, "helper-v2").unwrap();

    MacosServiceManager::new(paths.clone())
        .update_payload(&resolver, &helper)
        .unwrap();

    assert_eq!(fs::read_to_string(&paths.config).unwrap(), "keep-me");
    assert_eq!(
        fs::read_to_string(&paths.installed_binary).unwrap(),
        "resolver-v2"
    );
    assert_eq!(
        fs::read_to_string(&paths.admin_binary).unwrap(),
        "helper-v2"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_repair_restores_plist_without_overwriting_config() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, "keep-me").unwrap();
    let resolver = root.path().join("dns_relay");
    let helper = root.path().join("dns_relay_admin");
    fs::write(&resolver, "resolver").unwrap();
    fs::write(&helper, "helper").unwrap();

    MacosServiceManager::new(paths.clone())
        .repair_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();

    assert_eq!(fs::read_to_string(&paths.config).unwrap(), "keep-me");
    assert!(paths.service_definition.is_file());

    fs::remove_file(&paths.config).unwrap();
    fs::remove_file(&paths.service_definition).unwrap();
    MacosServiceManager::new(paths.clone())
        .repair_payload(&resolver, &helper, VALID_CONFIG)
        .unwrap();
    assert_eq!(fs::read_to_string(&paths.config).unwrap(), VALID_CONFIG);
    assert!(paths.service_definition.is_file());
}

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
fn read_config_uses_only_the_fixed_config_path() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, VALID_CONFIG).unwrap();

    let content = crate::execute_action(
        AdminAction::ReadConfig,
        &paths,
        &FakeService::running(),
        &FakeRunner::healthy(),
    )
    .unwrap();

    assert_eq!(content, VALID_CONFIG);
}

#[test]
fn malformed_request_id_is_rejected() {
    assert!(parse_request_id("../request").is_err());
}

#[test]
fn bundled_binary_hash_must_match_exact_sha256() {
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    assert!(verify_sha256(b"abc", digest).is_ok());
    assert!(verify_sha256(b"abc", "not-a-digest").is_err());
    assert!(verify_sha256(b"changed", digest).is_err());
    let root = tempdir().unwrap();
    let binary = root.path().join("dns_relay");
    fs::write(&binary, b"abc").unwrap();
    assert_eq!(sha256_file(&binary).unwrap(), digest);
}

#[test]
fn fixed_protocol_files_round_trip() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    let id = Uuid::parse_str(REQUEST_ID).unwrap();
    let request = AdminRequest {
        id,
        action: AdminAction::Status,
    };
    paths.write_request(&request).unwrap();
    assert_eq!(paths.read_request(id).unwrap().action, AdminAction::Status);

    let response = crate::AdminResponse {
        id,
        ok: true,
        message: "running".into(),
    };
    paths.write_response(&response).unwrap();
    let decoded = paths.read_response(id).unwrap();
    assert!(decoded.ok);
    assert_eq!(decoded.message, "running");
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(paths.response_path(id))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
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
    diagnostics: &'static str,
}

impl FakeService {
    fn running() -> Self {
        Self {
            state: Cell::new(ServiceStatus::Running),
            restart_fails: Cell::new(false),
            restarts: Cell::new(0),
            diagnostics: "",
        }
    }

    fn with_diagnostics(mut self, diagnostics: &'static str) -> Self {
        self.diagnostics = diagnostics;
        self
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

impl crate::AdminService for FakeService {
    fn install(&self, _config_toml: &str) -> Result<(), AdminError> {
        self.start()
    }

    fn update(&self) -> Result<(), AdminError> {
        self.restart()
    }

    fn repair(&self, _config_toml: &str) -> Result<(), AdminError> {
        self.restart()
    }

    fn uninstall(&self) -> Result<(), AdminError> {
        self.stop()
    }

    fn diagnostics(&self) -> Option<String> {
        (!self.diagnostics.is_empty()).then(|| self.diagnostics.into())
    }
}

#[test]
fn admin_dispatch_maps_only_closed_service_actions() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, LOG_METRICS_CONFIG).unwrap();
    let service = FakeService::running();
    let runner = FakeRunner::healthy();

    assert_eq!(
        crate::execute_action(AdminAction::Status, &paths, &service, &runner).unwrap(),
        "running"
    );
    assert_eq!(
        crate::execute_action(AdminAction::Stop, &paths, &service, &runner).unwrap(),
        "stopped"
    );
    assert_eq!(service.state.get(), ServiceStatus::Stopped);
    assert_eq!(
        crate::execute_action(AdminAction::Start, &paths, &service, &runner).unwrap(),
        "started"
    );
    assert_eq!(service.state.get(), ServiceStatus::Running);
    assert_eq!(
        crate::execute_action(AdminAction::Uninstall, &paths, &service, &runner).unwrap(),
        "uninstalled"
    );
    assert_eq!(service.state.get(), ServiceStatus::Stopped);
}

#[test]
fn failed_start_reports_service_diagnostics() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, HTTP_METRICS_CONFIG).unwrap();
    let service = FakeService::running()
        .with_diagnostics("systemctl show dns-relay-gui.service:\nNRestarts=42\njournalctl -u dns-relay-gui.service:\nError: NoHealthyResolvers");
    service.stop().unwrap();
    let runner = FakeRunner::healthy();
    runner.health_fails.set(true);

    let error = crate::execute_action(AdminAction::Start, &paths, &service, &runner).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("health check failed"));
    assert!(message.contains("NRestarts=42"));
    assert!(message.contains("NoHealthyResolvers"));
}

#[test]
fn start_with_log_metrics_config_does_not_wait_for_http_health() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, LOG_METRICS_CONFIG).unwrap();
    let service = FakeService::running();
    service.stop().unwrap();
    let runner = FakeRunner::healthy();
    runner.health_fails.set(true);

    crate::execute_action(AdminAction::Start, &paths, &service, &runner).unwrap();

    assert_eq!(runner.health_checks.get(), 0);
}

#[test]
fn apply_keeps_a_stopped_service_stopped() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    fs::create_dir_all(paths.config.parent().unwrap()).unwrap();
    fs::write(&paths.config, "old").unwrap();
    let service = FakeService::running();
    service.stop().unwrap();

    crate::execute_action(
        AdminAction::ApplyConfig {
            config_toml: VALID_CONFIG.into(),
            restart: true,
        },
        &paths,
        &service,
        &FakeRunner::healthy(),
    )
    .unwrap();

    assert_eq!(service.status().unwrap(), ServiceStatus::Stopped);
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
const HTTP_METRICS_CONFIG: &str = "drop_list = []\nredirect_list = []\nresolvers = []\n[metric_conf]\nenable = true\nreport_type = \"http\"\nreport_interval = 30\n";
const LOG_METRICS_CONFIG: &str = "drop_list = []\nredirect_list = []\nresolvers = []\n[metric_conf]\nenable = true\nreport_type = \"log\"\nreport_interval = 30\n";

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
fn stale_staged_config_does_not_block_the_next_save() {
    let fixture = ApplyFixture::running_with_config("old");
    fs::write(staging_path(&fixture.paths.config), "interrupted save").unwrap();

    fixture.apply(VALID_CONFIG, false).unwrap();

    assert_eq!(fixture.live_config(), VALID_CONFIG);
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

    assert!(fixture.apply(HTTP_METRICS_CONFIG, true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert_eq!(fixture.service.state.get(), ServiceStatus::Running);
}

#[test]
fn log_metrics_apply_does_not_wait_for_http_health() {
    let fixture = ApplyFixture::running_with_config("old");
    fixture.runner.health_fails.set(true);

    fixture.apply(LOG_METRICS_CONFIG, true).unwrap();

    assert_eq!(fixture.live_config(), LOG_METRICS_CONFIG);
    assert_eq!(fixture.runner.health_checks.get(), 0);
}

#[test]
fn repair_with_log_metrics_does_not_wait_for_http_health() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    let helper = root.path().join("bundle/dns_relay_admin");
    let resolver = root.path().join("bundle/dns_relay");
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::write(&helper, "helper").unwrap();
    fs::write(&resolver, "resolver").unwrap();
    let service = FakeService::running();
    let runner = FakeRunner::healthy();
    runner.health_fails.set(true);

    crate::execute_action_with_helper(
        AdminAction::Repair {
            expected_binary_sha256: crate::sha256_file(&resolver).unwrap(),
            config_toml: LOG_METRICS_CONFIG.into(),
        },
        &paths,
        &service,
        &runner,
        &helper,
    )
    .unwrap();

    assert_eq!(runner.health_checks.get(), 0);
}

#[test]
fn install_verifies_bundled_resolver_after_installed_files_are_removed() {
    let root = tempdir().unwrap();
    let paths = PlatformPaths::for_test(root.path());
    let helper = root.path().join("bundle/dns_relay_admin");
    let resolver = root.path().join("bundle/dns_relay");
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::write(&helper, "helper").unwrap();
    fs::write(&resolver, "resolver").unwrap();
    let service = FakeService::running();
    service.stop().unwrap();
    let runner = FakeRunner::healthy();

    let result = crate::execute_action_with_helper(
        AdminAction::Install {
            expected_binary_sha256: crate::sha256_file(&resolver).unwrap(),
            config_toml: LOG_METRICS_CONFIG.into(),
        },
        &paths,
        &service,
        &runner,
        &helper,
    )
    .unwrap();

    assert_eq!(result, "installed");
    assert_eq!(service.state.get(), ServiceStatus::Running);
}

#[test]
fn rule_only_apply_uses_hot_reload_without_restarting() {
    let fixture = ApplyFixture::running_with_config("old");

    fixture.apply(VALID_CONFIG, false).unwrap();

    assert_eq!(fixture.live_config(), VALID_CONFIG);
    assert_eq!(fixture.service.restarts.get(), 0);
    assert_eq!(fixture.runner.health_checks.get(), 0);
    assert_eq!(fs::read_to_string(&fixture.paths.backup).unwrap(), "old");
}

#[test]
fn restarting_apply_restarts_once_and_keeps_rollback_copy() {
    let fixture = ApplyFixture::running_with_config("old");

    fixture.apply(HTTP_METRICS_CONFIG, true).unwrap();

    assert_eq!(fixture.live_config(), HTTP_METRICS_CONFIG);
    assert_eq!(fixture.service.restarts.get(), 1);
    assert_eq!(fixture.runner.health_checks.get(), 1);
    assert_eq!(fs::read_to_string(&fixture.paths.backup).unwrap(), "old");
}
