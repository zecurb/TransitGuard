# Backend Synchronization and Recovery

TransitGuard Phase 7 implements a fictional, project-owned reader-to-backend synchronization path. It does not connect to real transit authorities, real fare equipment, payment networks, or proprietary transit protocols.

## Architecture

The implemented path is:

Reader-local SQLite -> reader synchronization coordinator -> versioned HTTP/JSON -> TransitGuard API -> transactional PostgreSQL ingest -> acknowledgement -> durable reader-local acknowledgement application.

SQLite remains the durable reader-local store for offline transactions, synchronization batches, retry state, and acknowledgements. PostgreSQL is the backend system of record. Readers never connect directly to PostgreSQL.

## Synchronization endpoint

Reader batches are submitted to:

`POST /v1/reader-synchronization/batches`

Required request metadata includes:

- `Content-Type: application/json`
- `Accept: application/json`
- `Idempotency-Key: <synchronization-batch-id>`
- `X-TransitGuard-Protocol-Version: <protocol-version>`

The idempotency key must match the batch identifier in the body, and the protocol-version header must match the request protocol version.

## Protocol limits

Protocol version 1 enforces bounded input: at most 256 entries per batch, at most 64 KiB per transaction envelope, and at most 1 MiB for synchronization request and acknowledgement bodies. Reader-local sequence numbers must be positive and strictly increasing within a request. Sequence gaps are allowed.

## Durable retries and idempotency

A retry does not create a replacement business identity. The reader reconstructs the same synchronization request from durable SQLite state, preserving reader identity, environment, protocol version, software version, batch identity, original submission time, sequence range, ordered transaction identities, ordered local sequence values, and canonical transaction envelopes.

The synchronization batch identifier is the primary replay identity. A new batch is processed transactionally. An identical replay returns the original acknowledgement without duplicating backend processing. Reusing a batch identity with conflicting reader identity, range, entry order, transaction identity, or canonical payload is rejected.

## Backend transaction boundary

Backend ingest validates reader registration and state, protocol and environment identity, sequence range, ordering, fingerprint, and replay status inside a PostgreSQL transaction. The batch, entries, outcomes, and acknowledgement are committed atomically before success is returned.

A failed transaction must not leave partial synchronization state.

## Acknowledgement handling

The reader stores a valid acknowledgement durably before applying queue changes atomically.

Supported entry outcomes are:

- `acknowledged`
- `retryable_failure`
- `permanent_failure`
- `manual_review`

Identical acknowledgement replay is idempotent. If acknowledgement storage succeeds but application is interrupted, the stored acknowledgement can be applied after restart.

## Lost-response recovery

A timeout does not prove the backend failed to commit. TransitGuard preserves the original durable batch identity so a reader can recover safely:

1. The reader submits batch A.
2. PostgreSQL commits batch A.
3. The HTTP response is lost.
4. The reader records a retryable transport failure.
5. The reader process stops and later restarts from the same SQLite database.
6. The reader reconstructs and resubmits batch A.
7. The backend detects an identical replay and returns the original acknowledgement.
8. The reader stores and applies the acknowledgement.
9. PostgreSQL still contains one batch and one copy of each entry.

Phase 7 includes an isolated PostgreSQL integration test for this recovery path.

## Failure categories

Synchronization uses bounded, sanitized categories such as `network_timeout`, `connection_failure`, `response_decode_failure`, `payload_too_large`, `unsupported_protocol`, `reader_not_registered`, `reader_not_operational`, `environment_mismatch`, `batch_identity_conflict`, `batch_range_mismatch`, `entry_order_mismatch`, `transaction_identity_conflict`, `backend_temporarily_unavailable`, `backend_validation_failure`, and `manual_review_required`.

Telemetry does not record transaction envelopes, credentials, private keys, database connection strings, raw database errors, unrestricted HTTP response bodies, or stack traces.

## Health and telemetry

The API exposes:

- `GET /health/live`
- `GET /health/ready`
- `GET /health/synchronization`

Liveness reports process health. Readiness checks the PostgreSQL dependency. Synchronization health exposes structured process-local counters for requests, acknowledgements, entries, outcomes, and bounded failure categories.

## Integration validation

The reader integration suite includes:

- `reader_http_postgres_round_trip_is_durable`
- `lost_response_restart_replays_without_duplicate_ingest`

The first verifies the uninterrupted SQLite -> HTTP -> API -> PostgreSQL -> acknowledgement -> SQLite path. The second verifies backend commit -> lost response -> durable retry -> restart -> identical replay -> original acknowledgement -> reader recovery.

These PostgreSQL tests require an isolated database and remain ignored during the normal workspace test run.

## Repository validation

Before Phase 7 is merged, run the repository validation gate inside the project Nix development shell:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `nix flake check`

Ignored PostgreSQL integration tests are validated separately against disposable isolated databases.

## Security boundary

Phase 7 intentionally does not implement real transit-authority connectivity, real fare-card compatibility, real payment processing, production reader credentials, production signing keys, real device certificates, or proprietary transit protocols. Production TLS, authentication, and credential provisioning belong to separate project work.
