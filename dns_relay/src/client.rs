use crate::{
    Error,
    cache::{
        ResponseCache, cache_key_from_query, cache_lookup, cache_lookup_stale, cache_store,
        new_cache,
    },
    conf::RelayConf,
    dns::{build_lookup_query, parse_a_records, with_txid},
    handler::{Flight, InFlightQueries},
    relay::RelayPicker,
    resolver::{DoqPool, ResolverPicker, UdpDispatcher},
};
use shared::build_http_client;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

#[derive(Clone, serde::Deserialize)]
pub struct ResolverConfig {
    pub resolvers: Vec<String>,
    #[serde(default)]
    pub relay: Option<RelayConf>,
}

pub struct DnsResolver {
    http: reqwest::Client,
    picker: ResolverPicker,
    relay_picker: Option<RelayPicker>,
    doq_pool: Arc<DoqPool>,
    udp_dispatcher: Arc<UdpDispatcher>,
    cache: Arc<ResponseCache>,
    in_flight: Arc<InFlightQueries>,
}

impl DnsResolver {
    pub async fn new(config: ResolverConfig) -> Result<Self, Error> {
        if config.resolvers.is_empty() {
            return Err(Error::Config("at least one resolver is required".into()));
        }

        let http = build_http_client()?;
        let doq_pool = Arc::new(DoqPool::new());
        let udp_dispatcher = Arc::new(UdpDispatcher::new()?);
        let picker =
            ResolverPicker::new(config.resolvers, http.clone(), &doq_pool, &udp_dispatcher).await?;
        let relay_picker = match config.relay.filter(|relay| relay.enable) {
            Some(relay) => {
                Some(RelayPicker::new(&relay, &picker, &http, &doq_pool, &udp_dispatcher).await?)
            }
            None => None,
        };

        Ok(Self {
            http,
            picker,
            relay_picker,
            doq_pool,
            udp_dispatcher,
            cache: Arc::new(new_cache()),
            in_flight: Arc::new(InFlightQueries::new()),
        })
    }

    pub async fn resolve_ipv4(&self, domain: &str) -> Result<Vec<Ipv4Addr>, Error> {
        let query = build_lookup_query(domain);
        let cache_key = cache_key_from_query(&query)
            .ok_or_else(|| Error::Other(format!("invalid DNS query for {domain}")))?;
        let parse = |reply: &[u8]| {
            let addresses = parse_a_records(reply);
            if addresses.is_empty() {
                Err(Error::Other(format!("no A records for {domain}")))
            } else {
                Ok(addresses)
            }
        };

        let leader = loop {
            if let Some(cached) = cache_lookup(&self.cache, &cache_key) {
                return parse(&cached);
            }
            match self.in_flight.join(cache_key.clone()) {
                Flight::Leader(leader) => break leader,
                Flight::Follower(follower) => {
                    if let Some(reply) = follower.wait().await {
                        return parse(&reply);
                    }
                }
            }
        };

        let reply = match resolve_transport(
            domain,
            &query,
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            false,
            &self.picker,
            self.relay_picker.as_ref(),
            &self.http,
            &self.doq_pool,
            &self.udp_dispatcher,
        )
        .await
        {
            Ok(reply) => {
                cache_store(&self.cache, cache_key.clone(), &reply);
                reply
            }
            Err(error) => match cache_lookup_stale(&self.cache, &cache_key) {
                Some(reply) => reply,
                None => return Err(error),
            },
        };
        let reply = with_txid(reply, [0, 0]);
        leader.publish(reply.clone());
        parse(&reply)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_transport(
    domain: &str,
    payload: &[u8],
    src_addr: SocketAddr,
    prefer_doh: bool,
    picker: &ResolverPicker,
    relay_picker: Option<&RelayPicker>,
    http: &reqwest::Client,
    doq_pool: &DoqPool,
    udp_dispatcher: &UdpDispatcher,
) -> Result<Vec<u8>, Error> {
    if let Some(relay_picker) = relay_picker {
        tokio::time::timeout(
            relay_picker.timeout_duration(),
            relay_picker.pick().resolve(domain, payload),
        )
        .await
        .map_err(|_| Error::ResolveTimeout)?
    } else {
        picker
            .resolve_packet(
                payload,
                src_addr,
                prefer_doh,
                http,
                doq_pool,
                udp_dispatcher,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{DnsResolver, ResolverConfig};
    use crate::dns::{craft_redirect_response, parse_a_records, parse_domain};
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tokio::{net::UdpSocket, sync::Barrier, task::JoinHandle};

    async fn mock_udp_resolver(ip: Ipv4Addr) -> (SocketAddr, Arc<AtomicUsize>, JoinHandle<()>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let queries = Arc::new(AtomicUsize::new(0));
        let server_queries = Arc::clone(&queries);
        let server = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let ip = ip.to_string();
            while let Ok((len, peer)) = socket.recv_from(&mut buf).await {
                server_queries.fetch_add(1, Ordering::Relaxed);
                let (_, qname_end) = parse_domain(&buf[..len], 12).unwrap();
                let response =
                    craft_redirect_response(&buf[..len], qname_end, vec![ip.as_str()]).unwrap();
                assert_eq!(
                    parse_a_records(&response),
                    vec![ip.parse::<Ipv4Addr>().unwrap()]
                );
                socket.send_to(&response, peer).await.unwrap();
            }
        });
        (address, queries, server)
    }

    #[tokio::test]
    async fn resolves_ipv4_with_programmatic_config() {
        let expected = Ipv4Addr::new(127, 0, 0, 42);
        let (upstream, queries, server) = mock_udp_resolver(expected).await;
        let resolver = DnsResolver::new(ResolverConfig {
            resolvers: vec![upstream.to_string()],
            relay: None,
        })
        .await
        .unwrap();
        queries.store(0, Ordering::Relaxed);

        assert_eq!(
            resolver.resolve_ipv4("example.test").await.unwrap(),
            vec![expected]
        );
        assert_eq!(queries.load(Ordering::Relaxed), 1);
        server.abort();
    }

    #[tokio::test]
    async fn caches_repeated_lookups() {
        let expected = Ipv4Addr::new(127, 0, 0, 42);
        let (upstream, queries, server) = mock_udp_resolver(expected).await;
        let resolver = DnsResolver::new(ResolverConfig {
            resolvers: vec![upstream.to_string()],
            relay: None,
        })
        .await
        .unwrap();
        queries.store(0, Ordering::Relaxed);

        let first = resolver.resolve_ipv4("cached.test").await.unwrap();
        let second = resolver.resolve_ipv4("cached.test").await.unwrap();
        assert_eq!(first, second);
        assert_eq!(queries.load(Ordering::Relaxed), 1);
        server.abort();
    }

    #[tokio::test]
    async fn coalesces_concurrent_lookups() {
        let expected = Ipv4Addr::new(127, 0, 0, 42);
        let (upstream, queries, server) = mock_udp_resolver(expected).await;
        let resolver = Arc::new(
            DnsResolver::new(ResolverConfig {
                resolvers: vec![upstream.to_string()],
                relay: None,
            })
            .await
            .unwrap(),
        );
        queries.store(0, Ordering::Relaxed);

        let barrier = Arc::new(Barrier::new(33));
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let resolver = Arc::clone(&resolver);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                resolver.resolve_ipv4("concurrent.test").await.unwrap()
            }));
        }
        barrier.wait().await;
        for task in tasks {
            assert_eq!(task.await.unwrap(), vec![expected]);
        }
        assert_eq!(queries.load(Ordering::Relaxed), 1);
        server.abort();
    }

    #[tokio::test]
    async fn rejects_empty_resolver_configuration() {
        let error = DnsResolver::new(ResolverConfig {
            resolvers: Vec::new(),
            relay: None,
        })
        .await
        .err()
        .expect("empty resolver configuration must fail");

        assert!(error.to_string().contains("at least one resolver"));
    }
}
