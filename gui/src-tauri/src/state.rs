use std::sync::{Arc, Mutex, atomic::AtomicBool};

use dns_relay::conf::{Conf, ObfsConf, ResolverSearchingConf};
use dns_relay_admin::PlatformPaths;

use crate::secrets::{KeyringBackend, SecretManager};

pub struct BackendState {
    pub(crate) draft: Mutex<Option<Conf>>,
    pub(crate) secrets: Arc<SecretManager<KeyringBackend>>,
    pub(crate) recovery_required: AtomicBool,
}

impl Default for BackendState {
    fn default() -> Self {
        let (draft, recovery_required) = PlatformPaths::current()
            .map(|paths| {
                let resolver = paths.installed_binary.is_file();
                let admin = paths.admin_binary.is_file();
                let config = paths.config.is_file();
                let service = paths.service_definition.is_file();
                let policy = paths.authorization_policy.is_file();
                draft_for_install_files(
                    resolver || admin || config || service || (cfg!(target_os = "linux") && policy),
                    resolver
                        && admin
                        && config
                        && service
                        && (!cfg!(target_os = "linux") || policy),
                    config,
                )
            })
            .unwrap_or((None, false));
        Self {
            draft: Mutex::new(draft),
            secrets: Arc::new(SecretManager::keyring(KeyringBackend::default())),
            recovery_required: AtomicBool::new(recovery_required),
        }
    }
}

pub(crate) fn draft_for_install_files(
    installation_present: bool,
    installation_complete: bool,
    config_present: bool,
) -> (Option<Conf>, bool) {
    if !installation_present {
        (Some(starter_draft()), false)
    } else if !installation_complete {
        ((!config_present).then(starter_draft), true)
    } else {
        (None, false)
    }
}

pub(crate) fn starter_draft() -> Conf {
    Conf {
        dns_target: "127.0.0.1:53".into(),
        resolvers: vec!["https://1.1.1.1/dns-query".into()],
        secure_only: true,
        resolver_searching: ResolverSearchingConf {
            ipv4: true,
            doh: true,
            ..Default::default()
        },
        metric_conf: shared::metric_wrapper::MetricConf {
            enable: true,
            report_type: shared::metric_wrapper::MetricReportType::Http,
            report_interval: 30,
        },
        obfs_conf: ObfsConf {
            bind_addr: "0.0.0.0:8853".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
