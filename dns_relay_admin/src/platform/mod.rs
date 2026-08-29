use std::{ffi::OsString, path::PathBuf};

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
