# DNS Resolver Crate And Relay Proxy Checkpoint

**Updated:** 2026-08-27

## Current State

- Design approved in chat.
- Written specification prepared for user review.
- Written specification approved by the user.
- First-release package verification ordering clarified before planning.
- Checkpointed implementation plan written and self-reviewed.
- Task 1 packaging probe confirmed Cargo requires the internal crate to exist in
  the registry before it will prepare the public crate.
- Inline execution on both `master` branches was explicitly approved.
- Task 1 is complete: both manifests are publishable, the internal crate
  verifies independently, and the first-publication ordering is documented.
- Task 2 is complete: `dns_relay` exposes a programmatic IPv4 resolver,
  constructs transports once, supports the existing optional relay config,
  and no longer performs an external public-IP lookup during bootstrap.
- Programmatic lookups now omit ECS for the unspecified client address instead
  of advertising the invalid `0.0.0/24` subnet.
- Task 3 is complete: the library client reuses the daemon's bounded DNS cache,
  in-flight miss coalescing, and transport-selection path.

## Verified Commits

- `dns-relay`: the latest commit containing this checkpoint; Task 1 is the
  `chore: make dns resolver crates publishable` commit.
- `relay-proxy`: no task commits yet.

## Verification

- Pre-design baseline: `cargo test --workspace` in `dns-relay` passed 69 tests;
  1 test was ignored.
- Pre-design baseline: `cargo test --all-targets` in `relay-proxy` passed; the
  project currently contains no tests.
- Task 1 RED: `cargo package -p dns_relay --allow-dirty --no-verify` failed
  because `shared` had no registry version.
- Task 1 diagnostic: standalone `dns-relay-shared` packaging exposed missing
  Tokio features hidden by workspace feature unification.
- Task 1 GREEN: `cargo check --workspace` passed.
- Task 1 GREEN: `cargo package -p dns-relay-shared --allow-dirty` packaged and
  verified 15 files.
- Task 1 expected bootstrap constraint: public packaging now reaches crates.io
  lookup and stops because `dns-relay-shared` has not been published yet.
- Task 2 RED: the public API tests failed because `DnsResolver` and
  `ResolverConfig` did not exist.
- Task 2 regression RED: the unspecified-address ECS test failed because the
  shared encoder added `0.0.0/24`.
- Task 2 GREEN: `cargo test --workspace` passed 72 tests; 1 test was ignored.
- Task 3 RED: repeated and 32-way concurrent client lookups produced duplicate
  upstream DNS queries.
- Task 3 GREEN: client tests passed with one upstream query for cached and
  concurrent lookups; `cargo test --workspace` passed 74 tests with 1 ignored;
  strict workspace Clippy passed.

## Resume Here

1. Read the design specification at
   `docs/superpowers/specs/2026-08-27-dns-resolver-crate-relay-proxy-design.md`.
2. Read
   `docs/superpowers/plans/2026-08-27-dns-resolver-crate-relay-proxy.md`.
3. Start Task 4 by adding the tag-gated crates.io publication workflow and
   copy-ready library documentation.

## Remaining Phases

1. crates.io GitHub Action and library documentation.
2. `relay-proxy` dependency, DNS configuration, and fail-closed upstream lookup.
3. Direct TLS tunnel and conditional CA initialization.
4. Catch-all rules and relay hot-path cleanup.
5. Final cross-repository verification and documentation.
