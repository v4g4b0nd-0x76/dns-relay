use std::{fs, path::Path, process::Command};

use uuid::Uuid;

use crate::{
    AdminError, PlatformPaths,
    apply::{ServiceManager, ServiceStatus},
    parse_request_id,
    platform::{CommandSpec, atomic_copy, atomic_write, bundled_resolver, remove_if_present},
};

const LABEL: &str = "system/com.dns-relay.gui";
const LAUNCHCTL: &str = "/bin/launchctl";
const ELEVATION_SCRIPT: &str = "on run argv\nset helperPath to item 1 of argv\nset requestId to item 2 of argv\ndo shell script quoted form of helperPath & \" request --request-id \" & quoted form of requestId with administrator privileges\nend run";
const PLIST: &[u8] = include_bytes!("../../../assets/gui/com.dns-relay.gui.plist");

pub struct MacosServiceManager {
    paths: PlatformPaths,
}

impl MacosServiceManager {
    pub fn new(paths: PlatformPaths) -> Self {
        Self { paths }
    }

    pub fn bootout_command(&self) -> CommandSpec {
        CommandSpec::new(LAUNCHCTL, ["bootout", LABEL])
    }

    pub fn enable_command(&self) -> CommandSpec {
        CommandSpec::new(LAUNCHCTL, ["enable", LABEL])
    }

    pub fn bootstrap_command(&self) -> CommandSpec {
        CommandSpec::new(
            LAUNCHCTL,
            [
                "bootstrap".into(),
                "system".into(),
                self.paths.service_definition.as_os_str().to_owned(),
            ],
        )
    }

    pub fn restart_command(&self) -> CommandSpec {
        CommandSpec::new(LAUNCHCTL, ["kickstart", "-k", LABEL])
    }

    pub fn status_command(&self) -> CommandSpec {
        CommandSpec::new(LAUNCHCTL, ["print", LABEL])
    }

    pub fn activation_commands(&self) -> Vec<CommandSpec> {
        vec![
            self.enable_command(),
            self.bootstrap_command(),
            self.restart_command(),
        ]
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
        atomic_write(&self.paths.service_definition, PLIST, 0o644)
    }

    pub(crate) fn update_payload(
        &self,
        resolver_source: &Path,
        helper_source: &Path,
    ) -> Result<(), AdminError> {
        atomic_copy(resolver_source, &self.paths.installed_binary, 0o755)?;
        atomic_copy(helper_source, &self.paths.admin_binary, 0o755)
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
        atomic_write(&self.paths.service_definition, PLIST, 0o644)
    }

    pub fn install(&self, config_toml: &str) -> Result<(), AdminError> {
        require_root()?;
        let helper_source = std::env::current_exe()?;
        let resolver_source = bundled_resolver(&helper_source)?;
        let _ = run(&self.bootout_command());
        self.install_payload(&resolver_source, &helper_source, config_toml)?;
        self.activate()
    }

    pub fn update(&self) -> Result<(), AdminError> {
        require_root()?;
        let helper_source = std::env::current_exe()?;
        let resolver_source = bundled_resolver(&helper_source)?;
        let _ = run(&self.bootout_command());
        self.update_payload(&resolver_source, &helper_source)?;
        self.activate()
    }

    pub fn repair(&self, config_toml: &str) -> Result<(), AdminError> {
        require_root()?;
        let helper_source = std::env::current_exe()?;
        let resolver_source = bundled_resolver(&helper_source)?;
        let _ = run(&self.bootout_command());
        self.repair_payload(&resolver_source, &helper_source, config_toml)?;
        self.activate()
    }

    pub fn uninstall(&self) -> Result<(), AdminError> {
        require_root()?;
        let _ = run(&self.bootout_command());
        remove_if_present(&self.paths.service_definition)?;
        remove_if_present(&self.paths.installed_binary)?;
        remove_if_present(&self.paths.admin_binary)?;
        Ok(())
    }

    fn activate(&self) -> Result<(), AdminError> {
        for command in self.activation_commands() {
            run(&command)?;
        }
        Ok(())
    }
}

impl ServiceManager for MacosServiceManager {
    fn status(&self) -> Result<ServiceStatus, AdminError> {
        let command = self.status_command();
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()?;
        launchctl_service_status(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
    }

    fn start(&self) -> Result<(), AdminError> {
        self.restart()
    }

    fn stop(&self) -> Result<(), AdminError> {
        run(&self.bootout_command())
    }

    fn restart(&self) -> Result<(), AdminError> {
        let _ = run(&self.bootout_command());
        self.activate()
    }
}

pub(crate) fn launchctl_service_status(
    success: bool,
    output: &str,
    error: &str,
) -> Result<ServiceStatus, AdminError> {
    if success && output.lines().any(|line| line.trim() == "state = running") {
        return Ok(ServiceStatus::Running);
    }
    if success && output.contains("state = spawn scheduled") {
        return Err(AdminError::Operation(
            "launchd service is repeatedly restarting".into(),
        ));
    }
    if success || error.contains("Could not find service") {
        return Ok(ServiceStatus::Stopped);
    }
    Err(AdminError::Operation(error.trim().to_owned()))
}

pub fn elevation_command(helper: &Path, request_id: &str) -> Result<CommandSpec, AdminError> {
    let id: Uuid = parse_request_id(request_id)?;
    Ok(CommandSpec::new(
        "/usr/bin/osascript",
        [
            "-e".into(),
            ELEVATION_SCRIPT.into(),
            helper.as_os_str().to_owned(),
            id.to_string().into(),
        ],
    ))
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
            "macOS service changes require administrator authorization".into(),
        ))
    }
}
