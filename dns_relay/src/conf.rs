use crate::ResponseCache;
use crate::errors::Error;
use crate::resolver::is_secure_resolver;
use serde::Deserialize;
use shared::metric_wrapper::MetricConf;
use shared::{
    dns::{Ipv4Subnet, parse_public_ipv4_subnet},
    domain_trie::{DomainTrie, referenced_rule_files},
};
use std::sync::Arc;
use std::time::SystemTime;
use std::{path::PathBuf, sync::RwLock};
use tokio::time::{Duration, MissedTickBehavior, interval};

#[derive(Default, Deserialize, Clone)]
pub struct Conf {
    #[serde(default = "default_dns_target")]
    pub dns_target: String,
    pub drop_list: Vec<String>,
    #[serde(deserialize_with = "shared::deserialize_redirect_list")]
    pub redirect_list: Vec<(String, String)>,
    pub resolvers: Vec<String>,
    #[serde(default)]
    pub secure_only: bool,
    #[serde(default, deserialize_with = "deserialize_client_subnet")]
    pub client_subnet: Option<Ipv4Subnet>,
    #[serde(default)]
    pub resolver_searching: ResolverSearchingConf,
    #[serde(default)]
    pub hotreload_conf: HotreloadConf,
    #[serde(default)]
    pub relay_conf: RelayConf,
    #[serde(default)]
    pub metric_conf: MetricConf,
    #[serde(default = "default_false")]
    pub vpn_reassertion: bool,
    #[serde(default = "default_false")]
    pub init_tls: bool,
    #[serde(default = "default_false")]
    pub record_history: bool,
    #[serde(default)]
    pub record_history_conf: Option<RecordHisotryConf>,
    #[serde(default)]
    pub obfs_conf: ObfsConf,
}

#[derive(Default, Deserialize, Clone)]
pub struct RecordHisotryConf {
    pub matched_list: Vec<String>, // vector of patters to cover like *.google.com or ads.google.com
    pub lines: usize,
}

fn default_dns_target() -> String {
    String::from("127.0.0.1:53")
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ObfsConf {
    #[serde(default)]
    pub enable: bool,
    #[serde(default = "default_obfs_bind")]
    pub bind_addr: String,
    /// One or more base64 keys. Multiple keys let you run several
    /// resolver_proxy deployments/clients against one dns_relay instance,
    /// each with its own key — the AEAD tag itself tells you which key (if
    /// any) a given packet was encrypted under.
    #[serde(default)]
    pub keys: Vec<String>,
}

fn default_obfs_bind() -> String {
    "0.0.0.0:8853".to_string()
}

fn default_false() -> bool {
    false
}

fn deserialize_client_subnet<'de, D>(deserializer: D) -> Result<Option<Ipv4Subnet>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|value| {
            parse_public_ipv4_subnet(&value).ok_or_else(|| {
                serde::de::Error::custom("client_subnet must be a canonical public IPv4 /24")
            })
        })
        .transpose()
}

#[derive(Default, Clone, Deserialize)]
pub struct RelayConf {
    pub enable: bool,
    pub resolve_manual: bool,
    #[serde(default = "default_relay_timeout_sec")]
    pub relay_timeout_sec: u64,
    pub relay_instances: Vec<Relay>,
}
fn default_relay_timeout_sec() -> u64 {
    5
}

#[derive(Default, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayTransport {
    #[default]
    Direct,
    GoogleChained,
}

#[derive(Default, Clone, Deserialize)]
pub struct Relay {
    pub relay_key: String,
    pub relay_url: String,
    pub transport: RelayTransport,
}

#[derive(Clone, Deserialize)]
pub struct HotreloadConf {
    pub enable: bool,
    pub poll_interval_ms: u64,
}
impl Default for HotreloadConf {
    fn default() -> Self {
        Self {
            enable: true,
            poll_interval_ms: 1_000,
        }
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct ResolverSearchingConf {
    pub enable: bool,
    pub resolver_source: Vec<String>,
    #[serde(default)]
    pub resfresh_interval: Option<u64>,
    pub ipv4: bool,
    pub doh: bool,
}

pub fn load_conf(path: &PathBuf) -> Result<Conf, Error> {
    let content = std::fs::read_to_string(path)?;
    let conf: Conf = toml::from_str(&content).map_err(|err| Error::Config(err.to_string()))?;
    conf.validate()?;
    Ok(conf)
}

impl Conf {
    fn validate(&self) -> Result<(), Error> {
        if !self.secure_only {
            return Ok(());
        }
        if self.relay_conf.enable
            && self
                .relay_conf
                .relay_instances
                .iter()
                .any(|relay| !relay.relay_url.starts_with("https://"))
        {
            return Err(Error::Config("secure relay URLs must use https://".into()));
        }
        let has_secure_resolver = self
            .resolvers
            .iter()
            .any(|resolver| is_secure_resolver(resolver));
        if self.relay_conf.enable && self.relay_conf.resolve_manual && !has_secure_resolver {
            return Err(Error::Config(
                "secure manual relay bootstrap requires an authenticated resolver".into(),
            ));
        }
        let has_secure_relay =
            self.relay_conf.enable && !self.relay_conf.relay_instances.is_empty();
        if !has_secure_resolver && !has_secure_relay {
            return Err(Error::Config(
                "secure_only requires an authenticated resolver or relay".into(),
            ));
        }
        Ok(())
    }
}

use arc_swap::ArcSwap;
use tracing::{error, info};

pub async fn watch_conf_and_reload(
    path: PathBuf,
    poll_interval: Duration,
    conf: Arc<RwLock<Conf>>,
    rule_trie: Arc<ArcSwap<DomainTrie>>,
    cache: Arc<ResponseCache>,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_mtime = tokio::fs::metadata(&path)
        .await
        .and_then(|metadata| metadata.modified())
        .ok();
    let initial_conf = conf.read().unwrap().clone();
    let mut list_mtimes = rule_file_mtimes(&initial_conf).await;

    loop {
        tick.tick().await;

        let mtime = match tokio::fs::metadata(&path)
            .await
            .and_then(|metadata| metadata.modified())
        {
            Ok(m) => m,
            Err(err) => {
                error!("failed to stat {}: {}", path.display(), err);
                continue;
            }
        };
        let config_changed = Some(mtime) != last_mtime;
        let current_conf = conf.read().unwrap().clone();
        let current_list_mtimes = rule_file_mtimes(&current_conf).await;
        let lists_changed = current_list_mtimes != list_mtimes;
        if !config_changed && !lists_changed {
            continue;
        }
        let reload_path = path.clone();
        let reload_result = tokio::task::spawn_blocking(move || {
            let new_conf = if config_changed {
                load_conf(&reload_path)?
            } else {
                current_conf
            };
            let new_trie = DomainTrie::build(&new_conf.drop_list, &new_conf.redirect_list);
            Ok::<_, Error>((new_conf, new_trie))
        })
        .await
        .map_err(|err| Error::Other(format!("config reload task failed: {err}")))
        .and_then(|result| result);

        match reload_result {
            Ok((new_conf, new_trie)) => {
                rule_trie.store(Arc::new(new_trie));
                // A list edit can add, remove, or redirect a name. Clearing a
                // bounded in-memory cache is cheap and prevents stale policy.
                if let Ok(mut guard) = cache.lock() {
                    guard.clear();
                }
                last_mtime = Some(mtime);
                list_mtimes = rule_file_mtimes(&new_conf).await;
                *conf.write().unwrap() = new_conf;

                info!(config_changed, lists_changed, "rules reloaded successfully");
            }
            Err(err) => error!("failed to reload conf.toml, keeping old config: {}", err),
        }
    }
}

async fn rule_file_mtimes(conf: &Conf) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut files = Vec::new();
    for path in referenced_rule_files(&conf.drop_list, &conf.redirect_list) {
        let mtime = tokio::fs::metadata(&path)
            .await
            .and_then(|metadata| metadata.modified())
            .ok();
        files.push((path, mtime));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}
