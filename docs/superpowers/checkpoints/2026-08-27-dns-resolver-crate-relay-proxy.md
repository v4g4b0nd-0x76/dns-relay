# DNS Resolver Crate And Relay Proxy Checkpoint

**Updated:** 2026-08-27

## Current State

- Design approved in chat.
- Written specification prepared for user review.
- Implementation has not started.

## Verified Commits

- `dns-relay`: the commit containing this checkpoint and the design
  specification.
- `relay-proxy`: no task commits yet.

## Verification

- Pre-design baseline: `cargo test --workspace` in `dns-relay` passed 69 tests;
  1 test was ignored.
- Pre-design baseline: `cargo test --all-targets` in `relay-proxy` passed; the
  project currently contains no tests.

## Resume Here

1. Read the design specification at
   `docs/superpowers/specs/2026-08-27-dns-resolver-crate-relay-proxy-design.md`.
2. Confirm the user approved the written specification.
3. Create the implementation plan with the writing-plans workflow.
4. Do not edit production code before the plan is approved.

## Remaining Phases

1. Publishable manifests, license, and package dry-runs.
2. `DnsResolver` API and DNS-server integration.
3. crates.io GitHub Action and library documentation.
4. `relay-proxy` dependency, DNS configuration, and fail-closed upstream lookup.
5. Direct TLS tunnel and conditional CA initialization.
6. Catch-all rules and relay hot-path cleanup.
7. Final cross-repository verification and documentation.
