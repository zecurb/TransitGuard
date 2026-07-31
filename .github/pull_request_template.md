## Summary

Describe what this pull request changes.

## Motivation

Explain why this change is needed and which problem it solves.

## Scope

List the primary areas changed.

- [ ] Domain model
- [ ] Application services
- [ ] Fare engine
- [ ] Device protocol
- [ ] Security
- [ ] PostgreSQL persistence
- [ ] Reader-local SQLite storage
- [ ] Reconciliation
- [ ] API
- [ ] Background worker
- [ ] Reader simulator
- [ ] Web client
- [ ] Mobile client
- [ ] Configuration
- [ ] Telemetry
- [ ] Documentation
- [ ] CI/CD or development tooling

## Implementation

Describe the important implementation decisions.

Include information such as:

- New domain types or invariants
- New application use cases
- Dependency-direction changes
- Persistence or migration changes
- Protocol changes
- Retry, timeout, or idempotency behavior
- Offline-reader behavior
- Error-handling behavior
- Compatibility considerations

## Testing

Describe the tests added or updated.

Commands executed:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
nix flake check
```

Additional tests:

- [ ] Unit tests
- [ ] Integration tests
- [ ] PostgreSQL tests
- [ ] SQLite durability tests
- [ ] API tests
- [ ] Protocol tests
- [ ] Synchronization tests
- [ ] Reconciliation tests
- [ ] Security regression tests
- [ ] Failure or recovery tests
- [ ] No additional tests were required

## Architecture review

- [ ] Domain rules remain independent from infrastructure.
- [ ] Applications do not duplicate domain or fare-engine rules.
- [ ] Database records remain separate from domain types.
- [ ] Crate dependency direction remains valid.
- [ ] No circular crate dependencies were introduced.
- [ ] Public interfaces use terminology from the domain glossary.
- [ ] A new ADR was added for a significant architectural decision.
- [ ] No ADR was required.

Architecture notes:

## Security review

- [ ] No secrets, private keys, passwords, or tokens were committed.
- [ ] Authentication implications were considered.
- [ ] Authorization implications were considered.
- [ ] Sensitive information is redacted from logs and errors.
- [ ] Replay and duplicate-effect risks were considered.
- [ ] Input validation and resource limits were considered.
- [ ] Threat-model documentation was updated.
- [ ] No threat-model update was required.

Security notes:

## Persistence and data review

- [ ] Database changes are represented through versioned migrations.
- [ ] Transaction boundaries preserve domain invariants.
- [ ] Concurrency behavior was considered.
- [ ] Idempotency behavior was considered.
- [ ] Failure and rollback behavior were tested.
- [ ] No persistence changes were made.

Migration or data notes:

## Offline-reader review

- [ ] Reader-local sequence continuity is preserved.
- [ ] Offline transactions are durably stored before approval.
- [ ] Synchronization retries reuse stable identifiers.
- [ ] Acknowledgements are durably applied before queue cleanup.
- [ ] Queue, storage, policy-age, and revocation-age limits were considered.
- [ ] Reader restart and recovery behavior were tested.
- [ ] No offline-reader behavior was changed.

Offline behavior notes:

## Observability review

- [ ] Structured logs were added or updated.
- [ ] Metrics were added or updated.
- [ ] Tracing and correlation identifiers were considered.
- [ ] Health or readiness behavior was considered.
- [ ] Telemetry does not expose sensitive values.
- [ ] No observability changes were required.

Observability notes:

## Documentation

Documentation added or updated:

- [ ] README
- [ ] Project charter
- [ ] System architecture
- [ ] Domain glossary
- [ ] Threat model
- [ ] Architecture Decision Record
- [ ] API documentation
- [ ] Protocol documentation
- [ ] Operational documentation
- [ ] Contribution or security policy
- [ ] No documentation changes were required

## Known limitations

Describe remaining limitations, follow-up work, or intentionally deferred
functionality.

## Safety boundary

Confirm that this change remains within TransitGuard's fictional,
project-owned environment.

- [ ] No real transit cards are used.
- [ ] No real transit authority is accessed.
- [ ] No proprietary transit protocol is reproduced.
- [ ] No real payment-card data is processed.
- [ ] No production transportation equipment is accessed.
- [ ] No real authority credentials or cryptographic keys are used.
- [ ] No security controls belonging to a third party are bypassed.

## Final checklist

- [ ] The branch is based on the latest `main`.
- [ ] The pull request contains one coherent workstream.
- [ ] Formatting passes.
- [ ] Compilation passes.
- [ ] Clippy passes with warnings denied.
- [ ] All tests pass.
- [ ] Nix flake validation passes.
- [ ] The working tree is clean.
- [ ] Documentation is current.
- [ ] Security implications are documented.
- [ ] Known limitations are stated.
- [ ] The change is ready for review.
