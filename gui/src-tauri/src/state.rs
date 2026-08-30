use std::sync::{Arc, Mutex};

use dns_relay::conf::Conf;
use dns_relay_admin::PlatformPaths;

use crate::secrets::{KeyringBackend, SecretManager};

pub struct BackendState {
    pub(crate) draft: Mutex<Option<Conf>>,
    pub(crate) secrets: Arc<SecretManager<KeyringBackend>>,
}

impl Default for BackendState {
    fn default() -> Self {
        let installed = PlatformPaths::current()
            .map(|paths| paths.installed_binary.is_file())
            .unwrap_or(true);
        Self {
            draft: Mutex::new(draft_for_install_state(installed)),
            secrets: Arc::new(SecretManager::keyring(KeyringBackend::default())),
        }
    }
}

pub(crate) fn draft_for_install_state(installed: bool) -> Option<Conf> {
    (!installed).then(Conf::default)
}
