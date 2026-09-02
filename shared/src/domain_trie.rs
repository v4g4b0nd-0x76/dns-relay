use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum DomainTriePolicy {
    #[default]
    None,
    Drop,
    Redirect(Vec<String>),
}

#[derive(Default)]
pub struct DomainTrie {
    children: HashMap<String, DomainTrie>,
    policy: DomainTriePolicy,
    // The common `*.example.com` form is represented by the trie itself.
    // Keep the small number of label-glob rules (for example
    // `ad-*.doubleclick.net`) separate, so ordinary list lookups stay O(labels)
    // rather than scanning every rule.
    label_globs: Vec<(String, DomainTriePolicy)>,
}

impl DomainTrie {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walks/creates the path for `pattern` and sets the policy on the
    /// resulting leaf node - NOT on `self`. This is the fix for the bug
    /// in the draft: `node` (the leaf found by the loop) must be the
    /// thing mutated, since `self` is still the trie root at this point.
    fn insert_with_policy(&mut self, pattern: &str, policy: DomainTriePolicy) {
        let pattern = pattern.trim_end_matches('.').to_lowercase();
        if pattern.contains('*') && !is_suffix_wildcard(&pattern) {
            self.label_globs.push((pattern, policy));
            return;
        }
        let pattern = pattern.strip_prefix("*.").unwrap_or(&pattern);

        let mut node = self;
        for label in pattern.rsplit('.') {
            node = node.children.entry(label.to_string()).or_default();
        }
        node.policy = policy;
    }

    pub fn insert_drop(&mut self, pattern: &str) {
        self.insert_with_policy(pattern, DomainTriePolicy::Drop);
    }

    /// `ip_with_port` matches your existing redirect_list's second tuple
    /// element, e.g. "10.0.0.5,10.0.0.6" or "10.0.0.5:53" - split on both
    /// separators the same way your old craft_redirect_response call site did.
    pub fn insert_redirect(&mut self, pattern: &str, ip_with_port: &str) {
        let ips: Vec<String> = ip_with_port
            .split(',')
            .map(|entry| entry.split(':').next().unwrap_or(entry).to_string())
            .collect();
        self.insert_with_policy(pattern, DomainTriePolicy::Redirect(ips));
    }

    pub fn build(drop_list: &[String], redirect_list: &[(String, String)]) -> Self {
        let mut trie = Self::new();

        let is_file_reference = |pattern: &str| {
            pattern.starts_with('/') || pattern.starts_with("./") || pattern.starts_with("../")
        };

        let read_list_file = |path: &str| -> Vec<String> {
            match std::fs::read_to_string(path) {
                Ok(content) => parse_blocklist(&content),
                Err(err) => {
                    tracing::error!("failed to read list file {}: {}", path, err);
                    Vec::new()
                }
            }
        };

        let mut seen_drop_patterns = HashSet::new();
        for entry in drop_list {
            let pattern = entry.trim();
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }
            if is_file_reference(pattern) {
                let lines = read_list_file(pattern);
                tracing::info!("loaded {} drop entries from {}", lines.len(), pattern);
                for domain in lines {
                    if seen_drop_patterns.insert(domain.clone()) {
                        trie.insert_drop(&domain);
                    }
                }
            } else if seen_drop_patterns.insert(pattern.to_string()) {
                trie.insert_drop(pattern);
            }
        }

        for (pattern, target) in redirect_list {
            let pattern = pattern.trim();
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }
            if is_file_reference(pattern) {
                let lines = read_list_file(pattern);
                tracing::info!("loaded {} redirect entries from {}", lines.len(), pattern);
                for line in &lines {
                    match line.split_once(':') {
                        Some((from, to)) if !from.trim().is_empty() && !to.trim().is_empty() => {
                            trie.insert_redirect(from.trim(), to.trim());
                        }
                        _ => tracing::warn!(
                            "skipping malformed redirect line in {}: {:?} (expected domain:ip1,ip2)",
                            pattern,
                            line
                        ),
                    }
                }
            } else {
                trie.insert_redirect(pattern, target);
            }
        }

        trie
    }

    /// Returns the policy at the closest matching boundary, walking TLD-down.
    /// A non-`None` policy on an ancestor short-circuits the walk, matching
    /// "*.example.com blocks all subdomains" semantics.
    pub fn lookup(&self, domain: &str) -> &DomainTriePolicy {
        let domain = domain.trim_end_matches('.').to_lowercase();
        let mut node = self;
        for label in domain.rsplit('.') {
            match node.children.get(label) {
                Some(next) => {
                    if next.policy != DomainTriePolicy::None {
                        return &next.policy;
                    }
                    node = next;
                }
                None => break,
            }
        }
        self.label_globs
            .iter()
            .find(|(pattern, _)| label_glob_matches(&domain, pattern))
            .map(|(_, policy)| policy)
            .unwrap_or(&DomainTriePolicy::None)
    }
}

fn is_suffix_wildcard(pattern: &str) -> bool {
    pattern.starts_with("*.") && !pattern[2..].contains('*')
}

fn label_glob_matches(domain: &str, pattern: &str) -> bool {
    let domain_labels: Vec<_> = domain.split('.').collect();
    let pattern_labels: Vec<_> = pattern.split('.').collect();
    domain_labels.len() == pattern_labels.len()
        && domain_labels
            .iter()
            .zip(pattern_labels)
            .all(|(domain_label, pattern_label)| wildcard_matches(domain_label, pattern_label))
}

fn wildcard_matches(value: &str, pattern: &str) -> bool {
    let value = value.as_bytes();
    let pattern = pattern.as_bytes();
    let (mut value_index, mut pattern_index) = (0usize, 0usize);
    let (mut star_index, mut restart_value) = (None, 0usize);

    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            value_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            restart_value = value_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            restart_value += 1;
            value_index = restart_value;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

/// Returns the external files referenced by rule entries.  Keeping this in
/// the rule module lets configuration watchers reload a list when the list is
/// edited, not only when conf.toml itself changes.
pub fn referenced_rule_files(
    drop_list: &[String],
    redirect_list: &[(String, String)],
) -> Vec<PathBuf> {
    let mut files = HashSet::new();
    for entry in drop_list {
        let entry = entry.trim();
        if is_file_reference(entry) {
            files.insert(PathBuf::from(entry));
        }
    }
    for (entry, _) in redirect_list {
        let entry = entry.trim();
        if is_file_reference(entry) {
            files.insert(PathBuf::from(entry));
        }
    }
    files.into_iter().collect()
}

fn is_file_reference(pattern: &str) -> bool {
    pattern.starts_with('/') || pattern.starts_with("./") || pattern.starts_with("../")
}

/// Accept the common plain-domain, hosts, and Adblock Plus list forms.  A
/// list can therefore be placed directly in `drop_list` without an expensive
/// conversion step.  DNSMasq `server=/.../` files are deliberately ignored:
/// they describe forwarding rules, not blocks.
fn parse_blocklist_line(raw_line: &str) -> Option<String> {
    let line = raw_line.trim().trim_end_matches('\r');
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }

    let candidate = if let Some(rest) = line.strip_prefix("||") {
        rest.split(['^', '$', '/']).next()?
    } else if let Some(rest) = line.strip_prefix("address=/") {
        rest.split('/').next()?
    } else if line.starts_with("server=/") {
        return None;
    } else if let Some(rest) = line.strip_prefix("local-zone:") {
        // Unbound entries look like: local-zone: "example.com" static
        if !(rest.contains("always_nxdomain")
            || rest.contains("static")
            || rest.contains("redirect"))
        {
            return None;
        }
        rest.split('"').nth(1)?
    } else {
        let mut parts = line.split_whitespace();
        let first = parts.next()?;
        if first.parse::<std::net::IpAddr>().is_ok() {
            parts.next()?
        } else {
            first
        }
    };

    let candidate = candidate
        .trim()
        .trim_end_matches('.')
        .strip_prefix("*.")
        .unwrap_or(candidate.trim().trim_end_matches('.'));
    if is_dns_name(candidate) {
        Some(candidate.to_ascii_lowercase())
    } else {
        None
    }
}

pub fn parse_blocklist(content: &str) -> Vec<String> {
    content.lines().filter_map(parse_blocklist_line).collect()
}

fn is_dns_name(name: &str) -> bool {
    // Blocklists are public DNS lists, not local search domains. Requiring a
    // dot excludes headings such as "blocked" while still accepting normal
    // domain names and underscore service records.
    name.len() <= 253
        && name.contains('.')
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
}
pub enum RuleMatch {
    Drop,
    Redirect(Vec<String>),
    None,
}

pub fn check_rules(domain: &str, trie: &DomainTrie) -> RuleMatch {
    match trie.lookup(domain) {
        DomainTriePolicy::Drop => RuleMatch::Drop,
        DomainTriePolicy::Redirect(ips) => RuleMatch::Redirect(ips.clone()),
        DomainTriePolicy::None => RuleMatch::None,
    }
}
