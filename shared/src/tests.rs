use std::time::{Duration, Instant};

use crate::cache::{
    CacheKey, cache_key_from_query, cache_key_from_query_for_client, cache_lookup,
    cache_lookup_stale, cache_store, clamp_cache_ttl,
};
use crate::constants::{CACHE_TTL_MAX, CACHE_TTL_MIN};
use crate::dns::{
    age_response_ttls, craft_redirect_response, craft_servfail_response, min_answer_ttl,
    parse_domain, response_cache_ttl, with_txid,
};
use crate::domain_trie::{DomainTrie, DomainTriePolicy};
use crate::{empty_cache, mock_query_google};
use std::io::Write;
use std::net::SocketAddr;
use tempfile::NamedTempFile;

#[test]
fn clamp_cache_ttl_bounds() {
    assert_eq!(clamp_cache_ttl(1), CACHE_TTL_MIN);
    assert_eq!(clamp_cache_ttl(60), Duration::from_secs(60));
    assert_eq!(clamp_cache_ttl(10_000), CACHE_TTL_MAX);
}

#[test]
fn cache_store_and_lookup_rewrites_txid_on_serve() {
    let cache = empty_cache();
    let query = mock_query_google();
    let key = cache_key_from_query(query).unwrap();
    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let mut answer = craft_redirect_response(query, qname_end, vec!["9.9.9.9"]).unwrap();
    answer[0] = 0x11;
    answer[1] = 0x22;

    cache_store(&cache, key.clone(), &answer);
    let cached = cache_lookup(&cache, &key).expect("cached");
    assert_eq!(&cached[..2], &[0, 0]);
    let served = with_txid(cached, [0xAB, 0xCD]);
    assert_eq!(&served[..2], &[0xAB, 0xCD]);
    assert_eq!(&served[served.len() - 4..], &[9, 9, 9, 9]);
    let _: CacheKey = key;
}

#[test]
fn cached_response_ttls_age_and_stale_ttls_are_zero() {
    let query = mock_query_google();
    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let mut answer = craft_redirect_response(query, qname_end, vec!["9.9.9.9"]).unwrap();

    assert!(age_response_ttls(
        &mut answer,
        Duration::from_secs(10),
        false
    ));
    assert_eq!(min_answer_ttl(&answer), Some(50));

    assert!(age_response_ttls(&mut answer, Duration::from_secs(0), true));
    assert_eq!(min_answer_ttl(&answer), None);
}

#[test]
fn expired_cache_entry_is_stale_then_evicted() {
    let cache = empty_cache();
    let query = mock_query_google();
    let key = cache_key_from_query(query).unwrap();
    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let answer = craft_redirect_response(query, qname_end, vec!["9.9.9.9"]).unwrap();
    cache_store(&cache, key.clone(), &answer);

    {
        let mut guard = cache.lock().unwrap();
        let entry = guard.get_mut(&key).unwrap();
        entry.inserted_at = Instant::now() - Duration::from_secs(61);
        entry.fresh_until = Instant::now() - Duration::from_secs(1);
        entry.stale_until = Instant::now() + Duration::from_secs(60);
    }
    assert!(cache_lookup(&cache, &key).is_none());
    assert!(cache_lookup_stale(&cache, &key).is_some());

    {
        let mut guard = cache.lock().unwrap();
        guard.get_mut(&key).unwrap().stale_until = Instant::now() - Duration::from_secs(1);
    }
    assert!(cache_lookup_stale(&cache, &key).is_none());
    assert!(cache.lock().unwrap().peek(&key).is_none());
}

fn negative_response_with_soa(rcode: u8) -> Vec<u8> {
    let mut packet = mock_query_google().to_vec();
    packet[2] = 0x81;
    packet[3] = 0x80 | rcode;
    packet[6..8].copy_from_slice(&0u16.to_be_bytes());
    packet[8..10].copy_from_slice(&1u16.to_be_bytes());
    packet[10..12].copy_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&[
        0xC0, 0x0C, // owner: google.com
        0x00, 0x06, // SOA
        0x00, 0x01, // IN
        0x00, 0x00, 0x00, 0x78, // RR TTL: 120
        0x00, 0x16, // RDLENGTH: root MNAME + root RNAME + five u32 fields
        0x00, // MNAME: root
        0x00, // RNAME: root
        0x00, 0x00, 0x00, 0x01, // SERIAL
        0x00, 0x00, 0x00, 0x02, // REFRESH
        0x00, 0x00, 0x00, 0x03, // RETRY
        0x00, 0x00, 0x00, 0x04, // EXPIRE
        0x00, 0x00, 0x00, 0x1E, // MINIMUM: 30
    ]);
    packet
}

#[test]
fn negative_cache_ttl_uses_soa_minimum() {
    assert_eq!(response_cache_ttl(&negative_response_with_soa(3)), Some(30));
    assert_eq!(response_cache_ttl(&negative_response_with_soa(0)), Some(30));

    let mut without_soa = negative_response_with_soa(3);
    without_soa[8..10].copy_from_slice(&0u16.to_be_bytes());
    assert_eq!(response_cache_ttl(&without_soa), None);
}

#[test]
fn cache_does_not_store_servfail_or_truncated_responses() {
    let cache = empty_cache();
    let query = mock_query_google();
    let key = cache_key_from_query(query).unwrap();

    let servfail = craft_servfail_response(query).unwrap();
    cache_store(&cache, key.clone(), &servfail);
    assert!(cache_lookup(&cache, &key).is_none());

    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let mut truncated = craft_redirect_response(query, qname_end, vec!["9.9.9.9"]).unwrap();
    truncated[2] |= 0x02; // TC
    cache_store(&cache, key.clone(), &truncated);
    assert!(cache_lookup(&cache, &key).is_none());
}

#[test]
fn direct_cache_key_is_scoped_to_client_subnet() {
    let query = mock_query_google();
    let first: SocketAddr = "192.0.2.25:53000".parse().unwrap();
    let same_subnet: SocketAddr = "192.0.2.99:53001".parse().unwrap();
    let other_subnet: SocketAddr = "192.0.3.25:53000".parse().unwrap();

    let key = cache_key_from_query_for_client(query, Some(first)).unwrap();
    assert_eq!(
        key,
        cache_key_from_query_for_client(query, Some(same_subnet)).unwrap()
    );
    assert_ne!(
        key,
        cache_key_from_query_for_client(query, Some(other_subnet)).unwrap()
    );
}

#[test]
fn domain_trie_reads_hosts_and_adblock_formats() {
    let mut list = NamedTempFile::new().unwrap();
    writeln!(list, "# comment\n0.0.0.0 ads.example.com\n||tracker.example.net^\nblocked\nserver=/not-a-block.example/").unwrap();
    let drop_list = vec![list.path().display().to_string()];
    let trie = DomainTrie::build(&drop_list, &[]);

    assert_eq!(trie.lookup("ads.example.com"), &DomainTriePolicy::Drop);
    assert_eq!(trie.lookup("tracker.example.net"), &DomainTriePolicy::Drop);
    assert_eq!(trie.lookup("not-a-block.example"), &DomainTriePolicy::None);
    assert_eq!(trie.lookup("blocked"), &DomainTriePolicy::None);
}

#[test]
fn parse_domain_rejects_compression_pointer_loop() {
    let packet = [
        0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, // header
        0xC0, 0x0C, // name points back to itself
        0, 1, 0, 1,
    ];
    assert!(parse_domain(&packet, 12).is_none());
}

#[test]
fn drop_and_redirect_coexist() {
    let drop_list = vec!["ads.example.com".to_string()];
    let redirect_list = vec![("internal.corp".to_string(), "10.0.0.5,10.0.0.6".to_string())];
    let trie = DomainTrie::build(&drop_list, &redirect_list);

    assert_eq!(trie.lookup("ads.example.com"), &DomainTriePolicy::Drop);
    assert_eq!(
        trie.lookup("tracker.ads.example.com"),
        &DomainTriePolicy::Drop
    );
    assert_eq!(
        trie.lookup("app.internal.corp"),
        &DomainTriePolicy::Redirect(vec!["10.0.0.5".to_string(), "10.0.0.6".to_string()])
    );
    assert_eq!(trie.lookup("unrelated.com"), &DomainTriePolicy::None);
}
