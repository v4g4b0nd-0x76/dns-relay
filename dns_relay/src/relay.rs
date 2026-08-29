use crate::{
    Error, ResolverPicker,
    cache::ResponseCache,
    conf::{Relay, RelayConf, RelayTransport},
    dns::{
        Ipv4Subnet, build_lookup_query, effective_ipv4_subnet, parse_a_records,
        parse_public_ipv4_subnet,
    },
    resolver::{DoqPool, UdpDispatcher},
};
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{error, info, warn};
use url::Url;

type HmacSha256 = Hmac<Sha256>;

pub fn gen_relay_key(_conf_path: &PathBuf) -> Result<(), Error> {
    let key = Aes256Gcm::generate_key(OsRng);
    println!("{}", STANDARD.encode(key));
    Ok(())
}

pub fn encode_for_relay(key: &Key<Aes256Gcm>, dns_message: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, dns_message)
        .expect("encryption failure");
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn decode_from_relay(key: &Key<Aes256Gcm>, packet: &[u8]) -> Option<Vec<u8>> {
    if packet.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = packet.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    Aes256Gcm::new(key).decrypt(nonce, ciphertext).ok()
}

pub fn load_key_from_str(key_b64: &str) -> Result<Key<Aes256Gcm>, Error> {
    let bytes = STANDARD
        .decode(key_b64)
        .map_err(|e| Error::Config(format!("invalid RELAY_KEY base64: {e}")))?;
    if bytes.len() != 32 {
        return Err(Error::Config(format!(
            "RELAY_KEY must decode to 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
}

/// Computes an opaque cache-key tag for `domain`, derived from the relay
/// key via HMAC-SHA256. Used by the Google Apps Script hop to cache
/// responses without ever seeing the domain in plaintext — it only ever
/// sees a tag it can't reverse without the key itself.
fn cache_key_for_domain(key: &Key<Aes256Gcm>, domain: &str) -> String {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key.as_slice()).expect("HMAC accepts any key length");
    mac.update(domain.to_ascii_lowercase().as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn resolve_via_relay(
    http: &reqwest::Client,
    worker_url: &str,
    key: &Key<Aes256Gcm>,
    dns_query: &[u8],
) -> Result<Vec<u8>, Error> {
    let encrypted = encode_for_relay(key, dns_query);
    let response = http
        .post(worker_url)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .body(encrypted)
        .send()
        .await
        .map_err(|e| Error::Config(e.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| Error::Config(e.to_string()))?;
    if !status.is_success() {
        return Err(Error::Config(format!(
            "relay returned {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    decode_from_relay(key, &body).ok_or_else(|| Error::Config("decrypt failed".into()))
}

pub(crate) async fn discover_client_subnet(
    client: &reqwest::Client,
    relay_url: &str,
) -> Result<Ipv4Subnet, Error> {
    let mut url =
        Url::parse(relay_url).map_err(|err| Error::Config(format!("invalid relay URL: {err}")))?;
    url.query_pairs_mut().append_pair("subnet", "1");
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| Error::Config(format!("subnet discovery failed: {err}")))?;
    if !response.status().is_success() {
        return Err(Error::Config(format!(
            "subnet discovery returned {}",
            response.status()
        )));
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| Error::Config(format!("invalid subnet discovery body: {err}")))?;
    if body.len() > 32 {
        return Err(Error::Config("subnet discovery body is too large".into()));
    }
    let body = std::str::from_utf8(&body)
        .map_err(|_| Error::Config("subnet discovery returned invalid text".into()))?;
    parse_public_ipv4_subnet(body.trim())
        .ok_or_else(|| Error::Config("subnet discovery returned an invalid IPv4 /24".into()))
}

fn replace_discovered_subnet(
    state: &RwLock<Option<Ipv4Subnet>>,
    cache: &ResponseCache,
    subnet: Option<Ipv4Subnet>,
) -> bool {
    let Ok(mut current) = state.write() else {
        return false;
    };
    if *current == subnet {
        return false;
    }
    *current = subnet;
    drop(current);
    if let Ok(mut cache) = cache.lock() {
        cache.clear();
    }
    true
}

#[derive(Serialize)]
struct AppsScriptRequest<'a> {
    data: String,
    k: &'a str,
}

#[derive(Deserialize)]
struct AppsScriptResponse {
    data: Option<String>,
    error: Option<String>,
}

/// Same job as `resolve_via_relay`, but wraps the encrypted packet in the
/// JSON+base64 envelope a Google Apps Script hop expects, and attaches an
/// opaque HMAC cache-key tag so the hop can cache responses without ever
/// seeing the domain in plaintext. Used when routing around networks where
/// Cloudflare is reachable but Google generally isn't blocked.
pub async fn resolve_via_relay_apps_script(
    http: &reqwest::Client,
    script_url: &str,
    key: &Key<Aes256Gcm>,
    domain: &str,
    dns_query: &[u8],
) -> Result<Vec<u8>, Error> {
    let encrypted = encode_for_relay(key, dns_query);
    let req_body = AppsScriptRequest {
        data: STANDARD.encode(&encrypted),
        k: &cache_key_for_domain(key, domain),
    };

    let response = http
        .post(script_url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| Error::Config(e.to_string()))?;

    let parsed: AppsScriptResponse = response
        .json()
        .await
        .map_err(|e| Error::Config(format!("invalid apps script response: {e}")))?;

    if let Some(err) = parsed.error {
        return Err(Error::Config(format!("apps script error: {err}")));
    }

    let data_b64 = parsed
        .data
        .ok_or_else(|| Error::Config("apps script response missing data field".into()))?;
    let encrypted_reply = STANDARD
        .decode(&data_b64)
        .map_err(|e| Error::Config(format!("invalid base64 in apps script response: {e}")))?;

    decode_from_relay(key, &encrypted_reply).ok_or_else(|| Error::Config("decrypt failed".into()))
}

pub async fn resolve_domain_via_relay(
    http: &reqwest::Client,
    worker_url: &str,
    key: &Key<Aes256Gcm>,
    domain: &str,
) -> Result<Vec<Ipv4Addr>, Error> {
    let query = build_lookup_query(domain);
    let encrypted = encode_for_relay(key, &query);
    let response = http
        .post(worker_url)
        .body(encrypted)
        .send()
        .await
        .map_err(|e| Error::Config(e.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| Error::Config(e.to_string()))?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&body);
        return Err(Error::Config(format!(
            "relay returned {status} for {domain}: {text}"
        )));
    }
    let reply =
        decode_from_relay(key, &body).ok_or_else(|| Error::Config("decrypt failed".into()))?;
    let ips = parse_a_records(&reply);
    if ips.is_empty() {
        return Err(Error::Config(format!("no A records for {domain}")));
    }
    Ok(ips)
}

pub fn host_from_url(url_str: &str) -> Result<String, Error> {
    let url = Url::parse(url_str).map_err(|e| Error::Config(format!("invalid relay url: {e}")))?;
    url.host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| Error::Config("relay url has no host".into()))
}

pub fn client_for_relay(
    worker_url: &str,
    ipv4: Option<&[Ipv4Addr]>,
) -> Result<reqwest::Client, Error> {
    let host = host_from_url(worker_url)?;
    let mut builder = Client::builder();
    if let Some(ipv4) = ipv4 {
        let ip = *ipv4
            .first()
            .ok_or_else(|| Error::Config("no resolved IPs for relay".into()))?;
        let addr = SocketAddr::new(IpAddr::V4(ip), 443);
        builder = builder.resolve(&host, addr);
    }
    builder.build().map_err(|e| Error::Config(e.to_string()))
}

pub struct RelayInstance {
    relay_client: Arc<reqwest::Client>,
    key: Key<Aes256Gcm>,
    url: String,
    transport: RelayTransport,
}

impl RelayInstance {
    async fn new(
        conf: &Relay,
        resolver_picker: &ResolverPicker,
        http: &reqwest::Client,
        doq_pool: &DoqPool,
        udp_dispatcher: &UdpDispatcher,
        resolve_ipv4: bool,
    ) -> Result<Self, Error> {
        let relay_host = host_from_url(&conf.relay_url).map_err(|err| {
            let msg = format!("invalid relay_url {}: {}", conf.relay_url, err);
            error!("{}", msg);
            Error::RelayErr(msg)
        })?;
        let ipv4: Option<Vec<Ipv4Addr>> = if resolve_ipv4 {
            let resolved = resolver_picker
                .resolve(&relay_host, None, http, doq_pool, udp_dispatcher)
                .await
                .map_err(|err| {
                    let msg = format!("failed to resolve relay host {}: {}", relay_host, err);
                    error!("{}", msg);
                    Error::RelayErr(msg)
                })?;
            if resolved.is_empty() {
                let msg = format!("failed to resolve relay host {}", relay_host);
                error!("{}", msg);
                return Err(Error::RelayErr(msg));
            }
            Some(resolved)
        } else {
            None
        };
        let relay_client = client_for_relay(&conf.relay_url, ipv4.as_deref()).map_err(|err| {
            let msg = format!("failed to build relay client: {}", err);
            error!("{}", msg);
            Error::RelayErr(msg)
        })?;
        let key = load_key_from_str(&conf.relay_key)
            .map_err(|err| Error::RelayErr(format!("invalid relay instance key: {}", err)))?;
        Ok(Self {
            relay_client: Arc::new(relay_client),
            key,
            url: conf.relay_url.clone(),
            transport: conf.transport.clone(),
        })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.relay_client
    }

    pub fn key(&self) -> &Key<Aes256Gcm> {
        &self.key
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Resolves `payload` (a raw DNS query) through this instance,
    /// dispatching to the right wire format based on `transport`. The
    /// pinned client (`self.client()`) is used either way — a Google Apps
    /// Script hostname needs the same IP-pinning treatment as a direct
    /// Cloudflare Worker hostname, to avoid the DNS-bootstrap self-loop
    /// when this process is also acting as the system resolver.
    pub async fn resolve(&self, domain: &str, payload: &[u8]) -> Result<Vec<u8>, Error> {
        match self.transport {
            RelayTransport::Direct => {
                resolve_via_relay(self.client(), self.url(), self.key(), payload).await
            }
            RelayTransport::GoogleChained => {
                resolve_via_relay_apps_script(
                    self.client(),
                    self.url(),
                    self.key(),
                    domain,
                    payload,
                )
                .await
            }
        }
    }

    #[cfg(test)]
    pub fn for_test(url: &str, key: Key<Aes256Gcm>) -> Self {
        Self {
            relay_client: Arc::new(reqwest::Client::new()),
            key,
            url: url.to_string(),
            transport: RelayTransport::Direct,
        }
    }

    #[cfg(test)]
    pub fn for_test_with_transport(
        url: &str,
        key: Key<Aes256Gcm>,
        transport: RelayTransport,
    ) -> Self {
        Self {
            relay_client: Arc::new(reqwest::Client::new()),
            key,
            url: url.to_string(),
            transport,
        }
    }
}

/// How a given relay instance is reached. `Direct` talks straight to the
/// Cloudflare Worker as before. `GoogleChained` wraps the same encrypted
/// packet in JSON+base64 and routes it through an Apps Script hop first —
/// useful when Cloudflare's own IPs are blocked but Google's aren't.
pub struct RelayPicker {
    instances: Vec<RelayInstance>,
    last_idx: AtomicUsize,
    timeout_duration: Duration,
    configured_subnet: Option<Ipv4Subnet>,
    discovered_subnet: Arc<RwLock<Option<Ipv4Subnet>>>,
}

impl RelayPicker {
    pub async fn new(
        conf: &RelayConf,
        resolver_picker: &ResolverPicker,
        http: &reqwest::Client,
        doq_pool: &DoqPool,
        udp_dispatcher: &UdpDispatcher,
    ) -> Result<Self, Error> {
        if conf.relay_instances.is_empty() {
            return Err(Error::RelayErr("no relay instances configured".into()));
        }
        let mut instances = Vec::with_capacity(conf.relay_instances.len());
        for instance_conf in &conf.relay_instances {
            instances.push(
                RelayInstance::new(
                    instance_conf,
                    resolver_picker,
                    http,
                    doq_pool,
                    udp_dispatcher,
                    conf.resolve_manual,
                )
                .await?,
            );
        }
        Ok(Self {
            instances,
            last_idx: AtomicUsize::new(0),
            timeout_duration: Duration::from_secs(conf.relay_timeout_sec),
            configured_subnet: None,
            discovered_subnet: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn new_secure(
        conf: &RelayConf,
        resolver_picker: &ResolverPicker,
        http: &reqwest::Client,
        doq_pool: &DoqPool,
        udp_dispatcher: &UdpDispatcher,
        configured_subnet: Option<Ipv4Subnet>,
        cache: Arc<ResponseCache>,
    ) -> Result<Self, Error> {
        let mut picker = Self::new(conf, resolver_picker, http, doq_pool, udp_dispatcher).await?;
        picker.configured_subnet = configured_subnet;
        if configured_subnet.is_none() {
            picker.spawn_subnet_discovery(cache);
        }
        Ok(picker)
    }

    fn spawn_subnet_discovery(&self, cache: Arc<ResponseCache>) {
        let Some(instance) = self
            .instances
            .iter()
            .find(|instance| matches!(instance.transport, RelayTransport::Direct))
        else {
            return;
        };
        let client = Arc::clone(&instance.relay_client);
        let url = instance.url.clone();
        let state = Arc::clone(&self.discovered_subnet);
        tokio::spawn(async move {
            let mut refresh = interval(Duration::from_secs(300));
            refresh.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                refresh.tick().await;
                let previous = state.read().ok().and_then(|value| *value);
                let subnet = discover_client_subnet(&client, &url).await.ok();
                if !replace_discovered_subnet(&state, &cache, subnet) {
                    continue;
                }
                match (previous, subnet) {
                    (None, Some(_)) => info!("relay client subnet is available"),
                    (Some(_), Some(_)) => info!("relay client subnet changed"),
                    (Some(_), None) => warn!("relay client subnet is unavailable"),
                    (None, None) => {}
                }
            }
        });
    }

    pub fn pick(&self) -> &RelayInstance {
        let idx = self.last_idx.fetch_add(1, Ordering::Relaxed) % self.instances.len();
        &self.instances[idx]
    }
    pub fn timeout_duration(&self) -> Duration {
        self.timeout_duration
    }

    pub fn effective_subnet(&self, client_addr: SocketAddr) -> Option<Ipv4Subnet> {
        let discovered = self.discovered_subnet.read().ok().and_then(|value| *value);
        effective_ipv4_subnet(self.configured_subnet, client_addr, discovered)
    }

    #[cfg(test)]
    pub fn from_instances(instances: Vec<RelayInstance>) -> Self {
        Self {
            instances,
            last_idx: AtomicUsize::new(0),
            timeout_duration: Duration::from_secs(1),
            configured_subnet: None,
            discovered_subnet: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub fn from_instances_with_subnets(
        instances: Vec<RelayInstance>,
        configured_subnet: Option<Ipv4Subnet>,
        discovered_subnet: Option<Ipv4Subnet>,
    ) -> Self {
        Self {
            instances,
            last_idx: AtomicUsize::new(0),
            timeout_duration: Duration::ZERO,
            configured_subnet,
            discovered_subnet: Arc::new(RwLock::new(discovered_subnet)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{discover_client_subnet, replace_discovered_subnet};
    use crate::{
        cache::{cache_key_from_query, cache_store, new_cache},
        dns::{craft_redirect_response, parse_domain},
    };
    use shared::mock_query_google;
    use std::sync::{Arc, RwLock};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn relay_discovery_accepts_public_canonical_subnet() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let length = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..length]);
            assert!(request.starts_with("GET /?subnet=1 HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n8.8.8.0/24\n",
                )
                .await
                .unwrap();
        });

        assert_eq!(
            discover_client_subnet(&reqwest::Client::new(), &url)
                .await
                .unwrap(),
            [8, 8, 8]
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn relay_discovery_reports_http_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let error = discover_client_subnet(&reqwest::Client::new(), &url)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("503"));
        server.await.unwrap();
    }

    #[test]
    fn discovered_subnet_change_clears_cache_once() {
        let cache = Arc::new(new_cache());
        let query = mock_query_google();
        let key = cache_key_from_query(query).unwrap();
        let (_, qname_end) = parse_domain(query, 12).unwrap();
        let answer = craft_redirect_response(query, qname_end, vec!["8.8.8.8"]).unwrap();
        cache_store(&cache, key.clone(), &answer);
        let state = RwLock::new(None);

        assert!(replace_discovered_subnet(&state, &cache, Some([8, 8, 8])));
        assert!(cache.lock().unwrap().is_empty());

        cache_store(&cache, key, &answer);
        assert!(!replace_discovered_subnet(&state, &cache, Some([8, 8, 8])));
        assert!(!cache.lock().unwrap().is_empty());
    }
}
