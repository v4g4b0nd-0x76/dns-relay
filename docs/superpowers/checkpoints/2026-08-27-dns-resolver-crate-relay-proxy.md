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

## Resume Here

1. Read the design specification at
   `docs/superpowers/specs/2026-08-27-dns-resolver-crate-relay-proxy-design.md`.
2. Read
   `docs/superpowers/plans/2026-08-27-dns-resolver-crate-relay-proxy.md`.
3. Start Task 2 by writing the failing `DnsResolver` API tests in
   `dns_relay/src/client.rs`.

## Remaining Phases

1. `DnsResolver` API and DNS-server integration.
2. crates.io GitHub Action and library documentation.
3. `relay-proxy` dependency, DNS configuration, and fail-closed upstream lookup.
4. Direct TLS tunnel and conditional CA initialization.
5. Catch-all rules and relay hot-path cleanup.
6. Final cross-repository verification and documentation.
