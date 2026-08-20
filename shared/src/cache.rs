use std::{
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use lru::LruCache;

use crate::{
    constants::{CACHE_CAPACITY, CACHE_TTL_MAX, CACHE_TTL_MIN},
    dns::{find_opt_record, matches_domain_pattern, min_answer_ttl, parse_domain},
};

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct CacheKey {
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
    // RD and CD can change the valid upstream response.  The direct relay
    // additionally inserts EDNS Client Subnet, so its cache key is scoped to
    // the client's /24 rather than leaking a location-specific answer to a
    // different network.
    pub query_flags: u8,
    pub dnssec_ok: bool,
    pub client_subnet: Option<[u8; 3]>,
}

pub struct CacheEntry {
    pub packet: Vec<u8>,
    pub expires_at: Instant,
}

pub type ResponseCache = Mutex<LruCache<CacheKey, CacheEntry>>;

pub fn new_cache() -> ResponseCache {
    Mutex::new(LruCache::new(
        NonZeroUsize::new(CACHE_CAPACITY).expect("cache capacity > 0"),
    ))
}

pub fn cache_key_from_query(payload: &[u8]) -> Option<CacheKey> {
    cache_key_from_query_for_client(payload, None)
}

pub fn cache_key_from_query_for_client(
    payload: &[u8],
    client_addr: Option<SocketAddr>,
) -> Option<CacheKey> {
    if payload.len() < 12 {
        return None;
    }
    let (name, qname_end) = parse_domain(payload, 12)?;
    if qname_end + 4 > payload.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([payload[qname_end], payload[qname_end + 1]]);
    let qclass = u16::from_be_bytes([payload[qname_end + 2], payload[qname_end + 3]]);
    let query_flags = payload[2] & 0x01 | payload[3] & 0x10; // RD | CD
    let dnssec_ok = find_opt_record(payload)
        .map(|opt| opt.flags & 0x8000 != 0)
        .unwrap_or(false);
    let client_subnet = client_addr.and_then(|addr| match addr.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => {
            let octets = ip.octets();
            Some([octets[0], octets[1], octets[2]])
        }
        _ => None,
    });
    Some(CacheKey {
        name,
        qtype,
        qclass,
        query_flags,
        dnssec_ok,
        client_subnet,
    })
}
pub fn remove_domains_from_cache(cache: &ResponseCache, patterns: &[String]) {
    let mut cache = cache.lock().unwrap();

    let keys: Vec<CacheKey> = cache
        .iter()
        .filter(|(key, _)| {
            patterns
                .iter()
                .any(|pattern| matches_domain_pattern(&key.name, pattern))
        })
        .map(|(key, _)| key.clone())
        .collect();

    for key in keys {
        cache.pop(&key);
    }
}
pub fn clamp_cache_ttl(ttl_secs: u32) -> Duration {
    let ttl = Duration::from_secs(u64::from(ttl_secs));
    if ttl < CACHE_TTL_MIN {
        CACHE_TTL_MIN
    } else if ttl > CACHE_TTL_MAX {
        CACHE_TTL_MAX
    } else {
        ttl
    }
}

pub fn cache_lookup(cache: &ResponseCache, key: &CacheKey) -> Option<Vec<u8>> {
    let mut guard = cache.lock().ok()?;
    let entry = guard.get(key)?;
    if Instant::now() >= entry.expires_at {
        guard.pop(key);
        return None;
    }
    Some(entry.packet.clone())
}

pub fn cache_store(cache: &ResponseCache, key: CacheKey, packet: &[u8]) {
    // Never cache transport failures, SERVFAIL/REFUSED responses, truncated
    // replies, or answerless replies.  The former poisoned the old cache for
    // a minute after a transient upstream problem; the latter need authority
    // SOA parsing for RFC-compliant negative caching, so leaving them uncached
    // is the safe choice.
    if !is_cacheable_answer(packet) {
        return;
    }
    let Some(ttl) = min_answer_ttl(packet).map(clamp_cache_ttl) else {
        return;
    };
    let mut stored = packet.to_vec();
    if stored.len() >= 2 {
        stored[0] = 0;
        stored[1] = 0;
    }
    if let Ok(mut guard) = cache.lock() {
        guard.put(
            key,
            CacheEntry {
                packet: stored,
                expires_at: Instant::now() + ttl,
            },
        );
    }
}

fn is_cacheable_answer(packet: &[u8]) -> bool {
    if packet.len() < 12 {
        return false;
    }
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    let answer_count = u16::from_be_bytes([packet[6], packet[7]]);
    let rcode = flags & 0x000F;

    flags & 0x8000 != 0 // QR: response
        && flags & 0x0200 == 0 // TC: not truncated
        && rcode == 0 // NOERROR
        && answer_count > 0
}

pub type DomainCache = Mutex<LruCache<String, Vec<Ipv4Addr>>>;

pub fn new_domain_cache() -> Arc<DomainCache> {
    Arc::new(Mutex::new(LruCache::new(
        NonZeroUsize::new(CACHE_CAPACITY).expect("cache capacity > 0"),
    )))
}

pub fn cache_url_ip(cache: &DomainCache, domain: &str, ipv4: Vec<Ipv4Addr>) {
    if let Ok(mut guard) = cache.lock() {
        guard.put(domain.to_string(), ipv4);
    }
}

pub fn get_cached_domain(cache: &DomainCache, domain: &str) -> Option<Vec<Ipv4Addr>> {
    match cache.lock() {
        Ok(mut guard) => {
            if let Some(cached) = guard.get(domain) {
                return Some(cached.clone());
            }
        }
        _ => return None,
    }
    None
}
