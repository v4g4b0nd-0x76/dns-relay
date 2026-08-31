use std::{env, fs, path::Path, process::Command};

use uuid::Uuid;

use crate::{
    AdminError, PlatformPaths,
    apply::{ServiceManager, ServiceStatus},
    parse_request_id,
    platform::{CommandSpec, atomic_copy, atomic_write, bundled_resolver, remove_if_present},
};

const SERVICE: &str = "dns-relay-gui.service";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
const UNIT: &[u8] = include_bytes!("../../../assets/gui/dns-relay-gui.service");
const POLICY: &[u8] = include_bytes!("../../../assets/gui/com.dns-relay.gui.policy");

pub struct LinuxServiceManager {
    paths: PlatformPaths,
}

impl LinuxServiceManager {
    pub fn new(paths: PlatformPaths) -> Self {
        Self { paths }
    }

    pub fn daemon_reload_command(&self) -> CommandSpec {
        CommandSpec::new(SYSTEMCTL, ["daemon-reload"])
    }

    pub fn enable_command(&self) -> CommandSpec {
        service_command("enable")
    }

    pub fn disable_command(&self) -> CommandSpec {
        service_command("disable")
    }

    pub fn start_command(&self) -> CommandSpec {
        service_command("start")
    }

    pub fn stop_command(&self) -> CommandSpec {
        service_command("stop")
    }

    pub fn restart_command(&self) -> CommandSpec {
        service_command("restart")
    }

    pub fn status_command(&self) -> CommandSpec {
        CommandSpec::new(SYSTEMCTL, ["is-active", SERVICE])
    }

    pub(crate) fn install_payload(
        &self,
        resolver_source: &Path,
        helper_source: &Path,
        config_toml: &str,
    ) -> Result<(), AdminError> {
        fs::create_dir_all(&self.paths.logs)?;
        atomic_copy(resolver_source, &self.paths.installed_binary, 0o755)?;
        atomic_copy(helper_source, &self.paths.admin_binary, 0o755)?;
        atomic_write(&self.paths.config, config_toml.as_bytes(), 0o600)?;
        atomic_write(&self.paths.service_definition, UNIT, 0o644)?;
        atomic_write(&self.paths.authorization_policy, POLICY, 0o644)
    }

    pub(crate) fn repair_payload(
        &self,
        resolver_source: &Path,
        helper_source: &Path,
        config_toml: &str,
    ) -> Result<(), AdminError> {
        fs::create_dir_all(&self.paths.logs)?;
        atomic_copy(resolver_source, &self.paths.installed_binary, 0o755)?;
        atomic_copy(helper_source, &self.paths.admin_binary, 0o755)?;
        if !self.paths.config.is_file() {
            atomic_write(&self.paths.config, config_toml.as_bytes(), 0o600)?;
        }
        atomic_write(&self.paths.service_definition, UNIT, 0o644)?;
        atomic_write(&self.paths.authorization_policy, POLICY, 0o644)
    }

    pub fn install(&self, config_toml: &str) -> Result<(), AdminError> {
        require_root()?;
        let helper = env::current_exe()?;
        let resolver = bundled_resolver(&helper)?;
        self.install_payload(&resolver, &helper, config_toml)?;
        run(&self.daemon_reload_command())?;
        run(&self.enable_command())?;
        run(&self.start_command())
    }

    pub fn update(&self) -> Result<(), AdminError> {
        require_root()?;
        let helper = env::current_exe()?;
        let resolver = bundled_resolver(&helper)?;
        atomic_copy(&resolver, &self.paths.installed_binary, 0o755)?;
        atomic_copy(&helper, &self.paths.admin_binary, 0o755)?;
        run(&self.daemon_reload_command())?;
        run(&self.restart_command())
    }

    pub fn repair(&self, config_toml: &str) -> Result<(), AdminError> {
        require_root()?;
        let helper = env::current_exe()?;
        let resolver = bundled_resolver(&helper)?;
        self.repair_payload(&resolver, &helper, config_toml)?;
        run(&self.daemon_reload_command())?;
        run(&self.enable_command())?;
        run(&self.restart_command()).or_else(|_| run(&self.start_command()))
    }

    pub fn uninstall(&self) -> Result<(), AdminError> {
        require_root()?;
        let _ = run(&self.stop_command());
        let _ = run(&self.disable_command());
        remove_if_present(&self.paths.service_definition)?;
        remove_if_present(&self.paths.authorization_policy)?;
        remove_if_present(&self.paths.installed_binary)?;
        remove_if_present(&self.paths.admin_binary)?;
        run(&self.daemon_reload_command())
    }
}

impl ServiceManager for LinuxServiceManager {
    fn status(&self) -> Result<ServiceStatus, AdminError> {
        let command = self.status_command();
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()?;
        systemctl_service_status(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
    }

    fn start(&self) -> Result<(), AdminError> {
        run(&self.start_command())
    }

    fn stop(&self) -> Result<(), AdminError> {
        run(&self.stop_command())
    }

    fn restart(&self) -> Result<(), AdminError> {
        run(&self.restart_command())
    }
}

pub(crate) fn systemctl_service_status(
    success: bool,
    output: &str,
    error: &str,
) -> Result<ServiceStatus, AdminError> {
    let state = output.trim();
    match state {
        "active" if success => Ok(ServiceStatus::Running),
        "inactive" | "failed" | "unknown" => Ok(ServiceStatus::Stopped),
        _ => Err(AdminError::Operation(if error.trim().is_empty() {
            format!("systemd service is {state}")
        } else {
            error.trim().to_owned()
        })),
    }
}

pub fn elevation_command(helper: &Path, request_id: &str) -> Result<CommandSpec, AdminError> {
    elevation_command_for(helper, request_id, Path::new("/usr/bin/pkexec").is_file())
}

pub(crate) fn elevation_command_for(
    helper: &Path,
    request_id: &str,
    pkexec_available: bool,
) -> Result<CommandSpec, AdminError> {
    let id: Uuid = parse_request_id(request_id)?;
    if !pkexec_available {
        return Err(AdminError::Operation(format!(
            "pkexec is unavailable; run: sudo {} request --request-id {id}",
            helper.display()
        )));
    }
    Ok(CommandSpec::new(
        "/usr/bin/pkexec",
        [
            helper.as_os_str().to_owned(),
            "request".into(),
            "--request-id".into(),
            id.to_string().into(),
        ],
    ))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn linux_invoking_uid_from(
    effective_uid: u32,
    pkexec_uid: Option<&str>,
    sudo_uid: Option<&str>,
) -> Result<u32, AdminError> {
    if effective_uid != 0 {
        return Ok(effective_uid);
    }
    pkexec_uid
        .or(sudo_uid)
        .and_then(|uid| uid.parse().ok())
        .filter(|uid| *uid != 0)
        .ok_or_else(|| AdminError::Operation("could not determine the invoking Linux user".into()))
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_invoking_uid() -> Result<u32, AdminError> {
    linux_invoking_uid_from(
        unsafe { libc::geteuid() },
        env::var("PKEXEC_UID").ok().as_deref(),
        env::var("SUDO_UID").ok().as_deref(),
    )
}

fn service_command(action: &str) -> CommandSpec {
    CommandSpec::new(SYSTEMCTL, [action, SERVICE])
}

fn run(command: &CommandSpec) -> Result<(), AdminError> {
    if succeeds(command)? {
        Ok(())
    } else {
        Err(AdminError::Operation(format!(
            "{} failed",
            command.program.display()
        )))
    }
}

fn succeeds(command: &CommandSpec) -> Result<bool, AdminError> {
    Ok(Command::new(&command.program)
        .args(&command.args)
        .status()?
        .success())
}

fn require_root() -> Result<(), AdminError> {
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err(AdminError::Operation(
            "Linux service changes require administrator authorization".into(),
        ))
    }
}
