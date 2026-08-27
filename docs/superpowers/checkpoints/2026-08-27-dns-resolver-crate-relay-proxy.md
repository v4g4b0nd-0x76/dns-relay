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
- Task 4 is complete: version tags verify the workspace and publish the internal
  crate before the public crate; CI now checks all workspace targets; release
  setup and programmatic usage are documented.
- Task 5 is complete: `relay-proxy` constructs one shared `DnsResolver`, uses it
  for every domain upstream, bypasses it for socket-address literals, and has no
  operating-system DNS fallback.
- Task 6 is complete: direct HTTPS now tunnels untouched TLS bytes, while CA
  generation/trust installation and TLS termination remain only for HTTP relay
  rules.
- Task 7 is complete: catch-all rules cover configured listeners with specific
  rules taking priority, duplicate per-port catch-alls are rejected, relay
  providers are sorted once, and small per-connection allocations/logging were
  removed.
- Task 8 is complete: both repositories pass their final verification sequences
  and were clean before this final checkpoint update.
- Post-plan relay performance follow-up is complete: equal-priority providers
  now share requests round-robin, transport/HTTP failures cool down for 15
  seconds, lower-priority providers remain fallback, and per-provider
  in-flight/success/failure/latency state is recorded in structured logs.

## Verified Commits

- `dns-relay`: the latest commit containing this checkpoint; Task 1 is the
  `chore: make dns resolver crates publishable` commit.
- `relay-proxy`: Task 5 is commit `4bf6f8a`; Task 6 is commit `adfb9d8`; Task 7
  is commit `943b2b7`; relay-pool balancing is commit `00be5c8`.

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
- Task 4 RED: `.github/workflows/publish-crates.yml` did not exist.
- Task 4 local parser adjustment: macOS Ruby 2.6 does not accept Psych's newer
  `aliases:` keyword, so syntax validation used `YAML.load_file(f)`; no workflow
  uses aliases.
- Task 4 GREEN: workflow YAML and `scripts/bump.sh` parsed; formatting, 74 tests
  with 1 ignored, strict Clippy, internal-crate packaging, and Cargo metadata all
  passed. No tag was pushed and no local publication was attempted.
- Task 5 RED: relay-proxy tests failed because the dns dependency, `[dns]`
  configuration, and resolver-backed `resolve_upstream` did not exist.
- Task 5 dependency correction: reqwest 0.12 has no `query` feature, so the
  invalid feature was removed while retaining its query API.
- Task 5 GREEN: `cargo test --all-targets` passed 2 tests, strict Clippy passed,
  and `cargo tree -d` showed one reqwest version (0.12.28). The local source path
  remains paired with `version = "1"` for registry portability.
- Task 6 GREEN: direct TLS bytes round-tripped unchanged, direct rules reported
  no CA requirement, and a relay TLS acceptor completed a trusted handshake;
  all 5 tests, strict Clippy, and the `release-perf` build passed.
- Task 7 RED: catch-all validation was absent and provider priority was not
  stored in startup order.
- Task 7 GREEN: all 10 tests passed, including specific-over-catch-all routing,
  duplicate validation, sorted providers, and provider fallback; strict Clippy
  and the `release-perf` build passed.
- Task 8 dns-relay GREEN: formatting passed, 74 tests passed with 1 ignored,
  strict Clippy passed, `dns-relay-shared` packaged and verified 15 files,
  metadata completed, and diff/status checks were clean.
- Task 8 relay-proxy GREEN: formatting passed, all 10 tests passed, strict
  Clippy passed, the cached `release-perf` build completed, only reqwest 0.12.28
  was present, and diff/status checks were clean.
- Relay-pool RED: tests could not compile because no shared provider pool or
  pool-aware relay boundary existed.
- Relay-pool GREEN: loopback integration tests observed `abcabc` distribution
  across three equal-priority Workers, a failed Worker was contacted once
  during cooldown, and a higher-priority failure reached the lower-priority
  provider. All 11 tests, strict Clippy, formatting, diff checks, and the
  `release-perf` build passed.

## Resume Here

1. Add a crates.io publish token to the GitHub repository secret
   `CARGO_REGISTRY_TOKEN`.
2. From `dns-relay`, run `PUSH=1 make patch` when ready to publish the first
   internal/public crate pair through GitHub Actions.
3. If that registry publication fails permanently, vendor the dns-relay source
   under `relay-proxy` and change its existing path dependency; the current
   sibling path already keeps local builds independent of crates.io.

## Remaining Phases

None.
