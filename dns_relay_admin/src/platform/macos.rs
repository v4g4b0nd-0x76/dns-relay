use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use uuid::Uuid;

use crate::{
    AdminError, PlatformPaths,
    apply::{ServiceManager, ServiceStatus},
    parse_request_id,
    platform::CommandSpec,
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
        Ok(if succeeds(&self.status_command())? {
            ServiceStatus::Running
        } else {
            ServiceStatus::Stopped
        })
    }

    fn start(&self) -> Result<(), AdminError> {
        if self.status()? == ServiceStatus::Stopped {
            self.activate()
        } else {
            run(&self.restart_command())
        }
    }

    fn stop(&self) -> Result<(), AdminError> {
        run(&self.bootout_command())
    }

    fn restart(&self) -> Result<(), AdminError> {
        run(&self.restart_command())
    }
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

fn bundled_resolver(helper: &Path) -> Result<PathBuf, AdminError> {
    Ok(helper
        .parent()
        .ok_or_else(|| AdminError::Operation("admin helper path has no parent".into()))?
        .join("dns_relay"))
}

fn atomic_copy(source: &Path, destination: &Path, mode: u32) -> Result<(), AdminError> {
    let mut input = fs::File::open(source)?;
    atomic_replace(destination, mode, |output| {
        std::io::copy(&mut input, output)?;
        Ok(())
    })
}

fn atomic_write(destination: &Path, content: &[u8], mode: u32) -> Result<(), AdminError> {
    atomic_replace(destination, mode, |output| {
        output.write_all(content)?;
        Ok(())
    })
}

fn atomic_replace(
    destination: &Path,
    mode: u32,
    write: impl FnOnce(&mut fs::File) -> Result<(), AdminError>,
) -> Result<(), AdminError> {
    let parent = destination
        .parent()
        .ok_or_else(|| AdminError::Operation("install path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AdminError::Operation("install filename is not valid UTF-8".into()))?;
    let staged = parent.join(format!(".{name}.installing"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut output = options.open(&staged)?;
    let result = write(&mut output).and_then(|()| {
        output.sync_all()?;
        fs::rename(&staged, destination)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
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

fn remove_if_present(path: &Path) -> Result<(), AdminError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
