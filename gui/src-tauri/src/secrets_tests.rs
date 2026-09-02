use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

use tempfile::tempdir;

use crate::{
    secrets::{
        BackendError, CredentialBackend, FallbackVault, MaskedSecret, SecretId, SecretManager,
        SecretStore,
    },
    vault::EncryptedVault,
};

#[derive(Default)]
pub(crate) struct MemoryBackend {
    available: Cell<bool>,
    values: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl MemoryBackend {
    fn available() -> Self {
        Self {
            available: Cell::new(true),
            values: RefCell::default(),
        }
    }
}

impl CredentialBackend for MemoryBackend {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), BackendError> {
        if !self.available.get() {
            return Err(BackendError::Unavailable);
        }
        self.values
            .borrow_mut()
            .insert(id.as_str().into(), value.into());
        Ok(())
    }

    fn get(&self, id: &SecretId) -> Result<Vec<u8>, BackendError> {
        if !self.available.get() {
            return Err(BackendError::Unavailable);
        }
        self.values
            .borrow()
            .get(id.as_str())
            .cloned()
            .ok_or(BackendError::Missing)
    }

    fn delete(&self, id: &SecretId) -> Result<(), BackendError> {
        if !self.available.get() {
            return Err(BackendError::Unavailable);
        }
        self.values.borrow_mut().remove(id.as_str());
        Ok(())
    }
}

#[test]
fn keyring_store_get_and_delete() {
    let store = SecretManager::keyring(MemoryBackend::available());
    let id = SecretId::new("relay.primary").unwrap();

    store.put(&id, b"rk_private").unwrap();
    assert_eq!(store.get(&id).unwrap().expose(), b"rk_private");
    store.delete(&id).unwrap();
    assert!(store.get(&id).is_err());
}

#[test]
fn selected_keyring_never_silently_switches_stores() {
    let store = SecretManager::keyring(MemoryBackend::available());
    let id = SecretId::new("relay.primary").unwrap();
    store.put(&id, b"rk_private").unwrap();

    let SecretManager::Keyring(backend) = &store else {
        unreachable!();
    };
    backend.available.set(false);
    assert!(store.delete(&id).is_err());
    backend.available.set(true);
    assert_eq!(store.get(&id).unwrap().expose(), b"rk_private");
    store.delete(&id).unwrap();
    assert!(store.get(&id).is_err());
}

#[test]
fn unavailable_keyring_uses_encrypted_fallback() {
    let root = tempdir().unwrap();
    let fallback = FallbackVault::new(root.path().join("vault.json"), "correct horse").unwrap();
    let id = SecretId::new("relay.primary").unwrap();
    let unavailable = SecretManager::keyring(MemoryBackend::default());
    assert!(matches!(
        unavailable.put(&id, b"rk_private"),
        Err(crate::secrets::SecretError::BackendUnavailable)
    ));
    let store = SecretManager::<MemoryBackend>::encrypted_fallback(fallback);

    store.put(&id, b"rk_private").unwrap();
    assert_eq!(store.get(&id).unwrap().expose(), b"rk_private");
    assert!(
        !std::fs::read_to_string(root.path().join("vault.json"))
            .unwrap()
            .contains("rk_private")
    );
    assert!(
        !std::fs::read_to_string(root.path().join("vault.json"))
            .unwrap()
            .contains("correct horse")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path().join("vault.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn wrong_fallback_passphrase_fails_closed() {
    let root = tempdir().unwrap();
    let path = root.path().join("vault.json");
    let id = SecretId::new("relay.primary").unwrap();
    let first = SecretManager::<MemoryBackend>::encrypted_fallback(
        FallbackVault::new(&path, "correct horse").unwrap(),
    );
    first.put(&id, b"rk_private").unwrap();

    let wrong = SecretManager::<MemoryBackend>::encrypted_fallback(
        FallbackVault::new(&path, "wrong").unwrap(),
    );
    assert!(wrong.get(&id).is_err());
    assert!(FallbackVault::new(&path, "").is_err());
}

#[test]
fn tampered_fallback_vault_fails_closed() {
    let mut entries = BTreeMap::new();
    entries.insert("relay.primary".into(), b"rk_private".to_vec());
    let mut vault = EncryptedVault::seal(&entries, "correct horse").unwrap();
    let second = EncryptedVault::seal(&entries, "correct horse").unwrap();
    assert_ne!(vault.ciphertext, second.ciphertext);
    vault.ciphertext[0] ^= 1;

    assert!(vault.open("correct horse").is_err());
}

#[test]
fn masked_secret_serialization_never_exports_the_value() {
    let store = SecretManager::keyring(MemoryBackend::available());
    let id = SecretId::new("relay.primary").unwrap();
    store.put(&id, b"rk_private").unwrap();
    let listing: MaskedSecret = store.masked(&id).unwrap();
    let json = serde_json::to_string(&listing).unwrap();

    assert!(json.contains("relay.primary"));
    assert!(json.contains("••••••••••••"));
    assert!(!json.contains("rk_private"));
}
