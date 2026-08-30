use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(target_os = "windows")]
use std::env;

use uuid::Uuid;

use crate::{AdminError, AdminRequest, AdminResponse};

#[derive(Debug, Clone)]
pub struct PlatformPaths {
    pub installed_binary: PathBuf,
    pub admin_binary: PathBuf,
    pub config: PathBuf,
    pub backup: PathBuf,
    pub logs: PathBuf,
    pub service_definition: PathBuf,
    pub authorization_policy: PathBuf,
    request_dir: PathBuf,
    response_dir: PathBuf,
}

impl PlatformPaths {
    pub fn current() -> Result<Self, AdminError> {
        platform_paths()
    }

    pub fn request_path(&self, id: Uuid) -> PathBuf {
        self.request_dir.join(format!("{id}.json"))
    }

    pub fn response_path(&self, id: Uuid) -> PathBuf {
        self.response_dir.join(format!("{id}.json"))
    }

    pub fn read_request(&self, id: Uuid) -> Result<AdminRequest, AdminError> {
        read_request_at(&self.request_path(id), &self.request_dir, id)
    }

    pub fn write_response(&self, response: &AdminResponse) -> Result<(), AdminError> {
        fs::create_dir_all(&self.response_dir)?;
        let content = serde_json::to_vec(response)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        std::io::Write::write_all(
            &mut options.open(self.response_path(response.id))?,
            &content,
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: &Path) -> Self {
        let system = root.join("system");
        let user = root.join("user");
        Self {
            installed_binary: system.join("dns_relay"),
            admin_binary: system.join("dns_relay_admin"),
            config: system.join("conf.toml"),
            backup: system.join("conf.toml.bak"),
            logs: system.join("logs"),
            service_definition: system.join("service"),
            authorization_policy: system.join("authorization.policy"),
            request_dir: user.join("requests"),
            response_dir: user.join("responses"),
        }
    }
}

pub(crate) fn read_request_at(
    path: &Path,
    expected_parent: &Path,
    expected_id: Uuid,
) -> Result<AdminRequest, AdminError> {
    let expected_name = format!("{expected_id}.json");
    if path.parent() != Some(expected_parent)
        || path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
    {
        return Err(AdminError::InvalidRequestFile(
            "request path is outside the fixed request directory".into(),
        ));
    }

    let link_metadata = fs::symlink_metadata(path)?;
    if link_metadata.file_type().is_symlink() {
        return Err(AdminError::InvalidRequestFile(
            "request file must not be a symlink".into(),
        ));
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AdminError::InvalidRequestFile(
            "request path must be a regular file".into(),
        ));
    }
    #[cfg(unix)]
    validate_unix_request(&metadata)?;

    let request: AdminRequest = serde_json::from_reader(file)?;
    if request.id != expected_id {
        return Err(AdminError::InvalidRequestFile(
            "request body ID does not match its filename".into(),
        ));
    }
    Ok(request)
}

#[cfg(unix)]
fn validate_unix_request(metadata: &fs::Metadata) -> Result<(), AdminError> {
    use std::os::unix::fs::MetadataExt;

    if metadata.mode() & 0o777 != 0o600 {
        return Err(AdminError::InvalidRequestFile(
            "request file mode must be 0600".into(),
        ));
    }
    if metadata.uid() != expected_request_owner()? {
        return Err(AdminError::InvalidRequestFile(
            "request file is not owned by the invoking user".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn expected_request_owner() -> Result<u32, AdminError> {
    invoking_uid()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn expected_request_owner() -> Result<u32, AdminError> {
    #[cfg(target_os = "linux")]
    return crate::platform::linux::linux_invoking_uid();

    #[cfg(not(target_os = "linux"))]
    Ok(unsafe { libc::geteuid() })
}

#[cfg(target_os = "macos")]
pub(crate) fn invoking_uid() -> Result<u32, AdminError> {
    use std::os::unix::fs::MetadataExt;

    let effective_uid = unsafe { libc::geteuid() };
    if effective_uid != 0 {
        return Ok(effective_uid);
    }
    let console_uid = fs::metadata("/dev/console")?.uid();
    if console_uid == 0 {
        Err(AdminError::Operation(
            "no logged-in macOS console user is available".into(),
        ))
    } else {
        Ok(console_uid)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn user_home() -> Result<PathBuf, AdminError> {
    use std::{
        ffi::{CStr, OsStr},
        mem::MaybeUninit,
        os::unix::ffi::OsStrExt,
        ptr,
    };

    let uid = invoking_uid()?;
    let mut password = MaybeUninit::<libc::passwd>::uninit();
    let mut result = ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1024];
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            password.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() {
        return Err(AdminError::Operation(format!(
            "could not resolve home directory for uid {uid}"
        )));
    }
    let directory = unsafe { CStr::from_ptr((*result).pw_dir) };
    Ok(PathBuf::from(OsStr::from_bytes(directory.to_bytes())))
}

#[cfg(target_os = "macos")]
fn platform_paths() -> Result<PlatformPaths, AdminError> {
    let system = PathBuf::from("/Library/Application Support/DNS Relay");
    let user = user_home()?.join("Library/Application Support/DNS Relay");
    Ok(PlatformPaths {
        installed_binary: system.join("dns_relay"),
        admin_binary: system.join("dns_relay_admin"),
        config: system.join("conf.toml"),
        backup: system.join("conf.toml.bak"),
        logs: system.join("logs"),
        service_definition: PathBuf::from("/Library/LaunchDaemons/com.dns-relay.gui.plist"),
        authorization_policy: system.join("authorization.policy"),
        request_dir: user.join("requests"),
        response_dir: user.join("responses"),
    })
}

#[cfg(target_os = "linux")]
fn platform_paths() -> Result<PlatformPaths, AdminError> {
    let system = PathBuf::from("/opt/dns-relay-gui");
    let runtime = PathBuf::from(format!(
        "/run/user/{}/dns-relay-gui",
        crate::platform::linux::linux_invoking_uid()?
    ));
    Ok(PlatformPaths {
        installed_binary: system.join("dns_relay"),
        admin_binary: system.join("dns_relay_admin"),
        config: system.join("conf.toml"),
        backup: system.join("conf.toml.bak"),
        logs: PathBuf::from("/var/log/dns-relay-gui"),
        service_definition: PathBuf::from("/etc/systemd/system/dns-relay-gui.service"),
        authorization_policy: PathBuf::from("/usr/share/polkit-1/actions/com.dns-relay.gui.policy"),
        request_dir: runtime.join("requests"),
        response_dir: runtime.join("responses"),
    })
}

#[cfg(target_os = "windows")]
fn platform_paths() -> Result<PlatformPaths, AdminError> {
    let program_files = PathBuf::from(
        env::var_os("ProgramFiles")
            .ok_or_else(|| AdminError::InvalidRequestFile("ProgramFiles is unavailable".into()))?,
    )
    .join("DNS Relay");
    let program_data = PathBuf::from(
        env::var_os("ProgramData")
            .ok_or_else(|| AdminError::InvalidRequestFile("ProgramData is unavailable".into()))?,
    )
    .join("DNS Relay");
    let local_data = PathBuf::from(
        env::var_os("LOCALAPPDATA")
            .ok_or_else(|| AdminError::InvalidRequestFile("LOCALAPPDATA is unavailable".into()))?,
    )
    .join("DNS Relay");
    Ok(PlatformPaths {
        installed_binary: program_files.join("dns_relay.exe"),
        admin_binary: program_files.join("dns_relay_admin.exe"),
        config: program_data.join("conf.toml"),
        backup: program_data.join("conf.toml.bak"),
        logs: program_data.join("logs"),
        service_definition: program_data.join("DNSRelayGui.service"),
        authorization_policy: program_data.join("authorization.policy"),
        request_dir: local_data.join("requests"),
        response_dir: local_data.join("responses"),
    })
}
