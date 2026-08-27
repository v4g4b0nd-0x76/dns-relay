# Resolver Performance Checkpoint

- Last green: `2fc613e` (`cargo test -p dns_relay`: 53 passed, 1 ignored).
- Current state: intentional TDD red for concurrent cache-miss coalescing.
- Expected error: `HandleQueryParams` lacks `udp_dispatcher` and `in_flight`.
- Next: add those fields, wire shared state in `main.rs`, use `resolve_packet`, then rerun `concurrent_identical_misses_share_one_upstream_query`.
- Keep the existing `Cargo.lock` change unstaged.
- Plan: `docs/superpowers/plans/2026-08-27-resolver-performance.md`.
