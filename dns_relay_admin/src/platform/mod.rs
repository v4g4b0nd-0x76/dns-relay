use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::AdminError;

#[cfg(unix)]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl CommandSpec {
    pub fn new<P, I, S>(program: P, args: I) -> Self
    where
        P: Into<PathBuf>,
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

pub(crate) fn bundled_resolver(helper: &Path) -> Result<PathBuf, AdminError> {
    Ok(helper
        .parent()
        .ok_or_else(|| AdminError::Operation("admin helper path has no parent".into()))?
        .join("dns_relay"))
}

pub(crate) fn atomic_copy(source: &Path, destination: &Path, mode: u32) -> Result<(), AdminError> {
    let mut input = fs::File::open(source)?;
    atomic_replace(destination, mode, |output| {
        std::io::copy(&mut input, output)?;
        Ok(())
    })
}

pub(crate) fn atomic_write(
    destination: &Path,
    content: &[u8],
    mode: u32,
) -> Result<(), AdminError> {
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
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut output = options.open(&staged)?;
    let result = write(&mut output).and_then(|()| {
        output.sync_all()?;
        fs::rename(&staged, destination)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

pub(crate) fn remove_if_present(path: &Path) -> Result<(), AdminError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
