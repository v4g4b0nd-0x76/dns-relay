use crate::{
    Error,
    cache::{
        ResponseCache, cache_key_from_query_for_subnet, cache_lookup, cache_lookup_stale,
        cache_store, new_cache,
    },
    conf::RelayConf,
    dns::{Ipv4Subnet, build_lookup_query, parse_a_records, set_ecs_ipv4_subnet, with_txid},
    handler::{Flight, InFlightQueries},
    relay::RelayPicker,
    resolver::{DoqPool, ResolverPicker, UdpDispatcher, is_secure_resolver},
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
        Self::build(config, false, None).await
    }

    pub async fn new_secure(
        config: ResolverConfig,
        client_subnet: Option<Ipv4Subnet>,
    ) -> Result<Self, Error> {
        Self::build(config, true, client_subnet).await
    }

    async fn build(
        config: ResolverConfig,
        secure_only: bool,
        client_subnet: Option<Ipv4Subnet>,
    ) -> Result<Self, Error> {
        let relay_enabled = config.relay.as_ref().is_some_and(|relay| relay.enable);
        let has_secure_resolver = config
            .resolvers
            .iter()
            .any(|resolver| is_secure_resolver(resolver));
        if secure_only && !relay_enabled && !has_secure_resolver {
            return Err(Error::Config(
                "secure_only requires an authenticated resolver or relay".into(),
            ));
        }
        if secure_only
            && config
                .relay
                .as_ref()
                .is_some_and(|relay| relay.enable && relay.resolve_manual)
            && !has_secure_resolver
        {
            return Err(Error::Config(
                "secure manual relay bootstrap requires an authenticated resolver".into(),
            ));
        }
        if config.resolvers.is_empty() && !relay_enabled {
            return Err(Error::Config("at least one resolver is required".into()));
        }

        let http = build_http_client()?;
        let doq_pool = Arc::new(DoqPool::new());
        let udp_dispatcher = Arc::new(UdpDispatcher::new()?);
        let cache = Arc::new(new_cache());
        let picker = if secure_only {
            ResolverPicker::new_secure(config.resolvers, http.clone(), &doq_pool, &udp_dispatcher)
                .await?
        } else {
            ResolverPicker::new(config.resolvers, http.clone(), &doq_pool, &udp_dispatcher).await?
        };
        let relay_picker = match config.relay.filter(|relay| relay.enable) {
            Some(relay) if secure_only => Some(
                RelayPicker::new_secure(
                    &relay,
                    &picker,
                    &http,
                    &doq_pool,
                    &udp_dispatcher,
                    client_subnet,
                    Arc::clone(&cache),
                )
                .await?,
            ),
            Some(relay) => Some(
                RelayPicker::new(&relay, &picker, &http, &doq_pool, &udp_dispatcher).await?,
            ),
            None => None,
        };

        Ok(Self {
            http,
            picker,
            relay_picker,
            doq_pool,
            udp_dispatcher,
            cache,
            in_flight: Arc::new(InFlightQueries::new()),
        })
    }

    pub async fn resolve_ipv4(&self, domain: &str) -> Result<Vec<Ipv4Addr>, Error> {
        let query = build_lookup_query(domain);
        let source = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
        let effective_subnet = self
            .relay_picker
            .as_ref()
            .and_then(|picker| picker.effective_subnet(source));
        let cache_key = cache_key_from_query_for_subnet(&query, effective_subnet)
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
            effective_subnet,
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
    effective_subnet: Option<Ipv4Subnet>,
    prefer_doh: bool,
    picker: &ResolverPicker,
    relay_picker: Option<&RelayPicker>,
    http: &reqwest::Client,
    doq_pool: &DoqPool,
    udp_dispatcher: &UdpDispatcher,
) -> Result<Vec<u8>, Error> {
    if let Some(relay_picker) = relay_picker {
        let payload = set_ecs_ipv4_subnet(payload, effective_subnet)
            .ok_or_else(|| Error::Other("failed to add ECS to relay query".into()))?;
        tokio::time::timeout(
            relay_picker.timeout_duration(),
            relay_picker.pick().resolve(domain, &payload),
        )
        .await
        .map_err(|_| Error::ResolveTimeout)?
    } else {
        picker
            .resolve_packet_for_subnet(
                payload,
                effective_subnet,
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
    use super::{DnsResolver, ResolverConfig, resolve_transport};
    use crate::conf::{Relay, RelayConf, RelayTransport};
    use crate::dns::{craft_redirect_response, parse_a_records, parse_domain};
    use crate::resolver::{DoqPool, UdpDispatcher};
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
    async fn secure_constructor_rejects_udp_only_configuration() {
        let error = DnsResolver::new_secure(
            ResolverConfig {
                resolvers: vec!["1.1.1.1:53".into()],
                relay: None,
            },
            None,
        )
        .await;

        assert!(error.is_err());
    }

    #[tokio::test]
    async fn secure_constructor_rejects_insecure_manual_relay_bootstrap() {
        let result = DnsResolver::new_secure(
            ResolverConfig {
                resolvers: vec!["1.1.1.1:53".into()],
                relay: Some(RelayConf {
                    enable: true,
                    resolve_manual: true,
                    relay_instances: vec![Relay {
                        relay_key: String::new(),
                        relay_url: "https://relay.example/".into(),
                        transport: RelayTransport::Direct,
                    }],
                    ..Default::default()
                }),
            },
            None,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn transport_uses_the_effective_subnet_for_ecs() {
        let upstream = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut query = [0; 512];
            let (length, peer) = upstream.recv_from(&mut query).await.unwrap();
            assert!(query[..length].ends_with(&[8, 8, 8]));
            let (_, qname_end) = parse_domain(&query[..length], 12).unwrap();
            let response = craft_redirect_response(
                &query[..length],
                qname_end,
                vec!["8.8.4.4"],
            )
            .unwrap();
            upstream.send_to(&response, peer).await.unwrap();
        });
        let picker = crate::resolver::ResolverPicker::from_healthy(vec![(
            upstream_addr.to_string(),
            std::time::Duration::ZERO,
        )]);
        let dispatcher = UdpDispatcher::new().unwrap();
        let query = crate::dns::build_lookup_query("example.test");

        let response = resolve_transport(
            "example.test",
            &query,
            Some([8, 8, 8]),
            false,
            &picker,
            None,
            &reqwest::Client::new(),
            &DoqPool::new(),
            &dispatcher,
        )
        .await
        .unwrap();

        assert_eq!(parse_a_records(&response), [Ipv4Addr::new(8, 8, 4, 4)]);
        server.await.unwrap();
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
