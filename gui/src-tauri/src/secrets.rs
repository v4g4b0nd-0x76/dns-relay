use std::{
    collections::BTreeMap,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use directories::ProjectDirs;
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

use crate::vault::EncryptedVault;

const SERVICE: &str = "com.dns-relay.gui";
const MASK: &str = "••••••••••••";

#[derive(Debug)]
pub enum SecretError {
    InvalidId,
    InvalidPassphrase,
    Missing,
    BackendUnavailable,
    Backend(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    KeyDerivation,
    VaultAuthentication,
    UnsupportedVaultVersion,
}

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => formatter.write_str("secret ID is invalid"),
            Self::InvalidPassphrase => formatter.write_str("vault passphrase must not be empty"),
            Self::Missing => formatter.write_str("secret is not configured"),
            Self::BackendUnavailable => formatter.write_str("OS credential storage is unavailable"),
            Self::Backend(message) => write!(formatter, "OS credential storage failed: {message}"),
            Self::Io(error) => write!(formatter, "vault I/O failed: {error}"),
            Self::Json(_) => formatter.write_str("vault data is invalid"),
            Self::KeyDerivation => formatter.write_str("vault key derivation failed"),
            Self::VaultAuthentication => formatter.write_str("vault passphrase or data is invalid"),
            Self::UnsupportedVaultVersion => formatter.write_str("vault version is unsupported"),
        }
    }
}

impl std::error::Error for SecretError {}

impl From<std::io::Error> for SecretError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SecretError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SecretId(String);

impl SecretId {
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(SecretError::InvalidId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct SecretValue(Zeroizing<Vec<u8>>);

impl SecretValue {
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue(***)")
    }
}

#[derive(Debug, Serialize)]
pub struct MaskedSecret {
    pub id: SecretId,
    pub value: &'static str,
}

impl MaskedSecret {
    pub fn new(id: SecretId) -> Self {
        Self { id, value: MASK }
    }
}

#[derive(Debug)]
pub enum BackendError {
    Missing,
    Unavailable,
    Other(String),
}

pub trait CredentialBackend {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), BackendError>;
    fn get(&self, id: &SecretId) -> Result<Vec<u8>, BackendError>;
    fn delete(&self, id: &SecretId) -> Result<(), BackendError>;
}

#[derive(Default)]
pub struct KeyringBackend {
    lock: Mutex<()>,
}

impl CredentialBackend for KeyringBackend {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), BackendError> {
        let _guard = self.lock.lock().map_err(|_| BackendError::Unavailable)?;
        entry(id)?.set_secret(value).map_err(map_keyring_error)
    }

    fn get(&self, id: &SecretId) -> Result<Vec<u8>, BackendError> {
        let _guard = self.lock.lock().map_err(|_| BackendError::Unavailable)?;
        entry(id)?.get_secret().map_err(map_keyring_error)
    }

    fn delete(&self, id: &SecretId) -> Result<(), BackendError> {
        let _guard = self.lock.lock().map_err(|_| BackendError::Unavailable)?;
        entry(id)?.delete_credential().map_err(map_keyring_error)
    }
}

fn entry(id: &SecretId) -> Result<keyring::Entry, BackendError> {
    keyring::Entry::new(SERVICE, id.as_str()).map_err(map_keyring_error)
}

fn map_keyring_error(error: keyring::Error) -> BackendError {
    match error {
        keyring::Error::NoEntry => BackendError::Missing,
        keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_) => {
            BackendError::Unavailable
        }
        error => BackendError::Other(error.to_string()),
    }
}

pub struct FallbackVault {
    path: PathBuf,
    passphrase: Zeroizing<String>,
    lock: Mutex<()>,
}

impl FallbackVault {
    pub fn new(
        path: impl Into<PathBuf>,
        passphrase: impl Into<String>,
    ) -> Result<Self, SecretError> {
        let passphrase = passphrase.into();
        if passphrase.is_empty() {
            return Err(SecretError::InvalidPassphrase);
        }
        Ok(Self {
            path: path.into(),
            passphrase: Zeroizing::new(passphrase),
            lock: Mutex::new(()),
        })
    }

    pub fn in_app_data(passphrase: impl Into<String>) -> Result<Self, SecretError> {
        let directory = ProjectDirs::from("com", "dns-relay", "DNS Relay")
            .ok_or(SecretError::BackendUnavailable)?
            .data_local_dir()
            .to_path_buf();
        Self::new(directory.join("vault.json"), passphrase)
    }

    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), SecretError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| SecretError::BackendUnavailable)?;
        let mut entries = self.load()?;
        if let Some(mut replaced) = entries.insert(id.as_str().into(), value.into()) {
            replaced.zeroize();
        }
        let result = self.save(&entries);
        zeroize_entries(&mut entries);
        result
    }

    fn get(&self, id: &SecretId) -> Result<SecretValue, SecretError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| SecretError::BackendUnavailable)?;
        let mut entries = self.load()?;
        let value = entries
            .remove(id.as_str())
            .map(|value| SecretValue(Zeroizing::new(value)))
            .ok_or(SecretError::Missing);
        zeroize_entries(&mut entries);
        value
    }

    fn delete(&self, id: &SecretId) -> Result<(), SecretError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| SecretError::BackendUnavailable)?;
        let mut entries = self.load()?;
        if let Some(mut removed) = entries.remove(id.as_str()) {
            removed.zeroize();
        }
        let result = self.save(&entries);
        zeroize_entries(&mut entries);
        result
    }

    fn load(&self) -> Result<BTreeMap<String, Vec<u8>>, SecretError> {
        match fs::read(&self.path) {
            Ok(content) => {
                serde_json::from_slice::<EncryptedVault>(&content)?.open(&self.passphrase)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, entries: &BTreeMap<String, Vec<u8>>) -> Result<(), SecretError> {
        let vault = EncryptedVault::seal(entries, &self.passphrase)?;
        write_private_json(&self.path, &serde_json::to_vec(&vault)?)
    }
}

pub enum SecretManager<B> {
    Keyring(B),
    EncryptedFallback(FallbackVault),
}

impl<B> SecretManager<B> {
    pub fn keyring(backend: B) -> Self {
        Self::Keyring(backend)
    }

    pub fn encrypted_fallback(fallback: FallbackVault) -> Self {
        Self::EncryptedFallback(fallback)
    }
}

impl<B: CredentialBackend> SecretManager<B> {
    pub fn masked(&self, id: &SecretId) -> Result<MaskedSecret, SecretError> {
        let _ = self.get(id)?;
        Ok(MaskedSecret::new(id.clone()))
    }
}

pub trait SecretStore {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), SecretError>;
    fn get(&self, id: &SecretId) -> Result<SecretValue, SecretError>;
    fn delete(&self, id: &SecretId) -> Result<(), SecretError>;
}

impl<B: CredentialBackend> SecretStore for SecretManager<B> {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), SecretError> {
        match self {
            Self::Keyring(backend) => backend.put(id, value).map_err(map_backend_error),
            Self::EncryptedFallback(fallback) => fallback.put(id, value),
        }
    }

    fn get(&self, id: &SecretId) -> Result<SecretValue, SecretError> {
        match self {
            Self::Keyring(backend) => backend
                .get(id)
                .map(|value| SecretValue(Zeroizing::new(value)))
                .map_err(map_backend_error),
            Self::EncryptedFallback(fallback) => fallback.get(id),
        }
    }

    fn delete(&self, id: &SecretId) -> Result<(), SecretError> {
        match self {
            Self::Keyring(backend) => match backend.delete(id) {
                Ok(()) | Err(BackendError::Missing) => Ok(()),
                Err(error) => Err(map_backend_error(error)),
            },
            Self::EncryptedFallback(fallback) => fallback.delete(id),
        }
    }
}

fn map_backend_error(error: BackendError) -> SecretError {
    match error {
        BackendError::Missing => SecretError::Missing,
        BackendError::Unavailable => SecretError::BackendUnavailable,
        BackendError::Other(message) => SecretError::Backend(message),
    }
}

fn zeroize_entries(entries: &mut BTreeMap<String, Vec<u8>>) {
    entries.values_mut().for_each(Zeroize::zeroize);
}

fn write_private_json(path: &Path, content: &[u8]) -> Result<(), SecretError> {
    let parent = path.parent().ok_or_else(|| {
        SecretError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "vault path has no parent",
        ))
    })?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(content)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
