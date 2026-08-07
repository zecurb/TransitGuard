# ADR 0004: Use Versioned HTTP and JSON for Reader Synchronization

- Status: Accepted
- Date: 2026-08-02
- Decision owners: TransitGuard maintainers
- Related documents:
  - `docs/PROJECT_CHARTER.md`
  - `docs/architecture/SYSTEM_ARCHITECTURE.md`
  - `docs/security/THREAT_MODEL.md`
  - `docs/adr/0002-use-postgresql-as-system-of-record.md`
  - `docs/adr/0003-use-sqlite-for-reader-offline-storage.md`
  - `docs/development/reader-offline-storage.md`

## Context

TransitGuard reader simulators can create fictional fare transactions while
the backend API is unavailable.

Phase 6 established durable reader-local SQLite storage for:

- reader-local sequence state;
- offline transactions;
- synchronization batches;
- batch entries;
- retry state;
- backend acknowledgements;
- partial outcomes;
- restart recovery.

The reader now requires a transport for submitting those durable batches to
the project-owned TransitGuard backend.

The transport must preserve:

- reader identity;
- environment identity;
- protocol version;
- software version;
- synchronization batch identity;
- transaction identity;
- local sequence values;
- entry order;
- transaction envelope;
- acknowledgement identity;
- per-entry outcome;
- retry behavior;
- idempotency.

The transport must also be understandable, testable, observable, and suitable
for a public Rust portfolio project.

Potential approaches include:

1. HTTP with JSON
2. HTTP with Protocol Buffers
3. gRPC
4. A persistent message broker
5. Raw TCP
6. WebSockets
7. Direct database access from readers

## Decision

TransitGuard will initially use a versioned HTTP and JSON protocol for reader
synchronization.

The reader will submit a complete durable synchronization batch to the
project-owned TransitGuard API.

The backend will validate and process the complete batch transactionally.

The backend will return a complete acknowledgement only after its database
transaction has committed.

This protocol is fictional and project-owned.

It does not reproduce, implement, or claim compatibility with a real transit
authority, farebox, card network, payment network, or device protocol.

## Endpoint

The initial endpoint will be:

    POST /v1/reader-synchronization/batches

The endpoint accepts one complete synchronization batch.

The endpoint does not accept individual fare transactions outside a batch.

The API will also expose:

    GET /health/live
    GET /health/ready

The liveness endpoint reports whether the API process is running.

The readiness endpoint reports whether required backend dependencies are
available for synchronization processing.

## Transport security

Production-style deployment documentation will require HTTPS.

Local development may use unencrypted loopback HTTP.

TransitGuard must not claim transport encryption when a local test server is
running without TLS.

Authentication, project-owned certificates, and credential provisioning will
be added through a separate security-focused phase.

Phase 7 must not use:

- real transit-authority certificates;
- real payment credentials;
- real device credentials;
- copied proprietary authentication schemes.

## Request headers

The synchronization request will use:

    Content-Type: application/json
    Accept: application/json
    Idempotency-Key: <synchronization-batch-id>
    X-TransitGuard-Protocol-Version: <protocol-version>

The `Idempotency-Key` value must match the batch identifier in the request
body.

The protocol-version header must match the protocol version in the request
body.

A mismatch is rejected before batch processing.

## Request identity

Every request contains:

- protocol version;
- environment identifier;
- reader identifier;
- reader software version;
- synchronization batch identifier;
- first local sequence number;
- last local sequence number;
- submission time;
- ordered batch entries.

Every entry contains:

- fare transaction identifier;
- reader-local sequence number;
- canonical transaction envelope.

The request body must not contain database connection information, backend
credentials, private signing keys, or administrator credentials.

## Entry ordering

Entries are ordered by reader-local sequence number.

Sequence values must be strictly increasing within one request.

The first request entry must match the declared first sequence number.

The last request entry must match the declared last sequence number.

Sequence values do not need to be contiguous.

A retried transaction may appear in a later batch after an earlier batch
received a retryable per-entry result.

Entry order is part of the idempotency contract.

Reordering entries while reusing the same batch identity creates a conflicting
replay.

## Protocol limits

Initial limits are:

- maximum 256 entries per batch;
- maximum 64 KiB per transaction envelope;
- maximum 1 MiB decoded request body;
- maximum 1 MiB acknowledgement body;
- nonempty environment identifier;
- nonempty software version;
- positive protocol version;
- positive reader-local sequence values.

These limits are part of protocol version 1.

Changing them incompatibly requires a protocol-version decision.

## Canonical representation

Protocol DTOs will serialize through a single defined Rust representation.

The backend will calculate a deterministic request fingerprint from the
validated protocol values.

The fingerprint must include:

- protocol version;
- environment identifier;
- reader identifier;
- reader software version;
- batch identifier;
- sequence range;
- ordered transaction identifiers;
- ordered local sequence values;
- exact canonical transaction envelopes.

The fingerprint will not depend on:

- HTTP header ordering;
- JSON whitespace;
- JSON object field ordering supplied by an arbitrary client;
- network connection identity;
- request arrival time.

The backend stores the fingerprint with the received batch.

## Idempotency

The synchronization batch identifier is the primary idempotency identity.

For a new batch identifier, the backend:

1. validates the request;
2. calculates the request fingerprint;
3. processes the batch;
4. stores the batch;
5. stores each entry result;
6. stores the acknowledgement;
7. commits the transaction;
8. returns the acknowledgement.

For an existing batch identifier with the same reader and fingerprint, the
backend returns the original acknowledgement.

It does not process the business effects again.

For an existing batch identifier with a different reader, fingerprint, range,
entry order, transaction identity, or transaction envelope, the backend
rejects the request as a conflicting replay.

A request timeout does not authorize the reader to generate a replacement
batch identity.

## Backend transaction boundary

Backend synchronization ingest must atomically:

1. validate the reader identity;
2. validate the reader operational state;
3. validate the environment;
4. validate the protocol version;
5. validate the batch identity;
6. validate the sequence range;
7. validate entry order;
8. detect an existing batch;
9. compare the canonical fingerprint;
10. store the received batch;
11. process every transaction entry;
12. store every per-entry result;
13. store the acknowledgement;
14. commit all changes.

A failed database transaction must not leave:

- a batch without entries;
- entries without a batch;
- a partial acknowledgement;
- partially applied business effects;
- a replay record without its original response.

The API must not return a successful acknowledgement before the transaction
commits.

## Acknowledgement contract

A successful HTTP response contains:

- protocol version;
- environment identifier;
- reader identifier;
- synchronization batch identifier;
- first local sequence number;
- last local sequence number;
- backend receipt time;
- whether the response came from an identical replay;
- ordered per-entry outcomes.

Every acknowledgement entry contains:

- fare transaction identifier;
- local sequence number;
- outcome;
- optional sanitized failure category;
- optional next retry time.

The acknowledgement entry order must match the request entry order.

## Per-entry outcomes

Protocol version 1 supports:

- `acknowledged`
- `retryable_failure`
- `permanent_failure`
- `manual_review`

An acknowledged entry contains no failure category or retry time.

A retryable failure contains:

- a nonempty sanitized failure category;
- a nonnegative next retry time.

A permanent failure contains:

- a nonempty sanitized failure category;
- no retry time.

A manual-review outcome contains:

- a nonempty sanitized failure category;
- no retry time.

A mixed batch still returns one complete acknowledgement after all outcomes
have committed.

## HTTP status behavior

The initial API behavior is:

### `200 OK`

The batch was processed successfully or was an identical replay.

The response contains a complete acknowledgement.

Per-entry retryable, permanent, and manual-review results are represented
inside the acknowledgement rather than as transport errors.

### `400 Bad Request`

The HTTP or JSON representation is malformed.

### `404 Not Found`

The endpoint does not exist.

Reader-registration failures are not represented as missing HTTP routes.

### `409 Conflict`

The request reuses an existing batch identity with conflicting content.

### `413 Content Too Large`

The request exceeds the configured body-size limit.

### `415 Unsupported Media Type`

The request does not use the required media type.

### `422 Unprocessable Content`

The JSON representation is syntactically valid but violates protocol
invariants.

Examples include:

- invalid sequence range;
- duplicate transaction identity;
- duplicate local sequence;
- entry-order mismatch;
- header and body identity mismatch;
- unsupported protocol version;
- invalid outcome metadata.

### `503 Service Unavailable`

A required backend dependency is temporarily unavailable before processing can
commit.

The reader may retry the same durable batch according to its bounded retry
policy.

## Reader retry behavior

The reader submits only a batch already stored durably in SQLite.

Before submission, the reader transitions the durable batch into its
submission state.

For retryable transport failures, the reader preserves:

- batch identifier;
- transaction identifiers;
- local sequence values;
- entry order;
- canonical request body;
- attempt count;
- retry schedule.

Retryable transport failures include:

- connection failure;
- request timeout;
- temporary name-resolution failure;
- HTTP 503;
- incomplete response body;
- temporary response decoding failure.

The reader must not create a new business identity because a response was
lost.

## Reader acknowledgement behavior

After receiving an acknowledgement, the reader:

1. validates the HTTP response;
2. validates the protocol version;
3. validates the reader identity;
4. validates the environment identity;
5. validates the batch identity;
6. validates the sequence range;
7. validates the entry count;
8. validates entry order;
9. validates transaction identities;
10. validates local sequence values;
11. stores the acknowledgement durably;
12. applies its per-entry outcomes atomically.

An invalid acknowledgement causes no queue mutation.

An identical acknowledgement replay is idempotent.

## Failure categories

Failure categories must be:

- stable;
- bounded;
- sanitized;
- safe for logs and metrics;
- free of secrets;
- free of raw database errors;
- free of complete transaction payloads.

Initial categories include:

- `network_timeout`
- `connection_failure`
- `response_decode_failure`
- `payload_too_large`
- `unsupported_protocol`
- `reader_not_registered`
- `reader_not_operational`
- `environment_mismatch`
- `batch_identity_conflict`
- `batch_range_mismatch`
- `entry_order_mismatch`
- `transaction_identity_conflict`
- `backend_temporarily_unavailable`
- `backend_validation_failure`
- `manual_review_required`

## Logging

The API may log:

- request correlation identity;
- reader identity;
- batch identity;
- protocol version;
- entry count;
- sequence range;
- request duration;
- replay status;
- aggregate outcome counts;
- stable error category.

The API must not log:

- raw credentials;
- private keys;
- authentication tokens;
- complete transaction envelopes;
- raw database connection strings;
- unrestricted response bodies;
- personal payment information.

## Metrics

Initial synchronization metrics will include:

- synchronization requests;
- new batches;
- identical replays;
- conflicting replays;
- request validation failures;
- request duration;
- batch entry count;
- acknowledged entries;
- retryable entries;
- permanent failures;
- manual-review entries;
- backend transaction failures;
- reader transport retries;
- acknowledgement validation failures.

Metric labels must be bounded.

Transaction identifiers and batch identifiers must not be used as unbounded
metric labels.

## Time handling

All protocol timestamps use Unix milliseconds.

Time is supplied explicitly to domain and application operations.

Protocol validation rejects negative timestamps.

Tests must not depend on the system clock when deterministic input can be
supplied.

## Versioning

Protocol version 1 is represented by `DeviceProtocolVersion::CURRENT`.

Backward-incompatible changes require a new protocol version.

The backend may support multiple versions during a migration window.

A reader must not silently downgrade its protocol representation.

An unsupported version returns a stable protocol failure.

## Alternatives considered

### gRPC

gRPC provides generated schemas and efficient binary transport.

It was not selected initially because HTTP/JSON is easier to inspect, exercise,
document, and demonstrate in a portfolio environment.

A later version may introduce gRPC through a separate architecture decision.

### Message broker

A message broker can provide durable asynchronous delivery.

It was not selected for the initial reader transport because the reader already
owns a durable local queue and the project first needs an explicit
request/acknowledgement contract.

A broker may later be introduced behind the API for backend processing.

### Direct PostgreSQL access

Readers will not connect directly to PostgreSQL.

Direct database access would expose backend credentials, bypass the application
boundary, complicate network security, and tightly couple readers to backend
schema.

### Raw TCP

Raw TCP would require TransitGuard to design framing, routing, retries,
observability, compatibility, and operational tooling already provided by
HTTP infrastructure.

## Consequences

### Positive

- Requests are easy to inspect and test.
- The protocol is explicit and versioned.
- Standard HTTP tooling can be used locally.
- Reader retries can remain deterministic.
- Backend idempotency is directly testable.
- The API remains the authoritative application boundary.
- The project demonstrates realistic distributed-system failure handling.

### Negative

- JSON is larger than a compact binary protocol.
- Canonicalization requires careful implementation.
- HTTP status handling and application outcomes must remain distinct.
- Request limits must be enforced at multiple layers.
- Authentication and TLS deployment remain separate work.

## Implementation sequence

Phase 7 will proceed in this order:

1. Define validated protocol DTOs.
2. Define canonical request fingerprinting.
3. Add PostgreSQL ingest migrations.
4. Implement the transactional backend ingest service.
5. Add the HTTP API and health endpoints.
6. Implement the reader HTTP client.
7. Integrate acknowledgement storage and application.
8. Add restart, timeout, replay, and conflict tests.
9. Add observability and operational documentation.

## Safety boundary

All protocol messages, readers, transactions, acknowledgements, credentials,
endpoints, and infrastructure are fictional and project-owned.

This ADR does not authorize integration with:

- real transit cards;
- real transit equipment;
- real transit-authority APIs;
- proprietary transportation protocols;
- real payment networks;
- real device certificates;
- real production credentials.
