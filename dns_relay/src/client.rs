use crate::{
    Error,
    conf::RelayConf,
    dns::{build_lookup_query, parse_a_records},
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
        })
    }

    pub async fn resolve_ipv4(&self, domain: &str) -> Result<Vec<Ipv4Addr>, Error> {
        let query = build_lookup_query(domain);
        let reply = if let Some(relay_picker) = &self.relay_picker {
            tokio::time::timeout(
                relay_picker.timeout_duration(),
                relay_picker.pick().resolve(domain, &query),
            )
            .await
            .map_err(|_| Error::ResolveTimeout)??
        } else {
            self.picker
                .resolve_packet(
                    &query,
                    SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
                    false,
                    &self.http,
                    &self.doq_pool,
                    &self.udp_dispatcher,
                )
                .await?
        };
        let addresses = parse_a_records(&reply);
        if addresses.is_empty() {
            return Err(Error::Other(format!("no A records for {domain}")));
        }
        Ok(addresses)
    }
}

#[cfg(test)]
mod tests {
    use super::{DnsResolver, ResolverConfig};
    use crate::dns::{craft_redirect_response, parse_a_records, parse_domain};
    use std::net::{Ipv4Addr, SocketAddr};
    use tokio::{net::UdpSocket, task::JoinHandle};

    async fn mock_udp_resolver(
        ip: Ipv4Addr,
        response_count: usize,
    ) -> (SocketAddr, JoinHandle<()>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let ip = ip.to_string();
            for _ in 0..response_count {
                let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
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
        (address, server)
    }

    #[tokio::test]
    async fn resolves_ipv4_with_programmatic_config() {
        let expected = Ipv4Addr::new(127, 0, 0, 42);
        let (upstream, server) = mock_udp_resolver(expected, 2).await;
        let resolver = DnsResolver::new(ResolverConfig {
            resolvers: vec![upstream.to_string()],
            relay: None,
        })
        .await
        .unwrap();

        assert_eq!(
            resolver.resolve_ipv4("example.test").await.unwrap(),
            vec![expected]
        );
        server.await.unwrap();
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
