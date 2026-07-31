# ADR 0003: Use SQLite for Durable Reader Offline Storage

- Status: Accepted
- Date: 2026-07-31
- Decision owners: TransitGuard maintainers
- Related documents:
  - `docs/architecture/SYSTEM_ARCHITECTURE.md`
  - `docs/architecture/DOMAIN_GLOSSARY.md`
  - `docs/security/THREAT_MODEL.md`
  - `docs/adr/0001-use-modular-rust-workspace.md`
  - `docs/adr/0002-use-postgresql-as-system-of-record.md`

## Context

TransitGuard Reader Simulators must continue operating for a bounded period
when the central TransitGuard API is unavailable.

During offline operation, a reader may need to preserve:

- Reader identity metadata
- Reader-local sequence state
- Offline fare transactions
- Transaction processing outcomes
- Synchronization batch state
- Backend acknowledgements
- Retry state
- Cached fare-policy metadata
- Cached revocation metadata
- Software and protocol versions
- Operational recovery information

This state must survive:

- Reader process restarts
- Operating-system restarts
- Temporary network loss
- Application crashes
- Interrupted synchronization
- Lost acknowledgements
- Retry attempts
- Partial synchronization failures

An in-memory queue is insufficient because queued transactions would disappear
when the reader process exits.

A collection of loosely managed files would make atomic updates, ordering,
crash recovery, schema evolution, and consistency validation difficult.

The reader therefore requires a small, embedded, transactional persistence
system that does not depend on the central PostgreSQL database or continuous
network connectivity.

Potential approaches include:

1. In-memory queues
2. JSON or binary files
3. SQLite
4. An embedded key-value database
5. A local PostgreSQL instance
6. A dedicated local message broker

## Decision

Each TransitGuard Reader Simulator will use SQLite as its initial durable local
storage engine.

SQLite will preserve reader-local state while the reader operates online or
offline.

The SQLite database is local to one reader identity.

A reader must not share one writable SQLite database file with another reader
identity.

The reader-local SQLite database is not the backend system of record.

PostgreSQL remains the authoritative backend system of record after reader
transactions are successfully synchronized and durably acknowledged.

Before acknowledgement, the reader-local database is authoritative for its own
unsynchronized transactions.

## Scope of reader-local storage

SQLite will initially store:

- Reader-local sequence state
- Offline transaction queue
- Synchronization batch records
- Synchronization entry records
- Backend acknowledgements
- Retry metadata
- Cached fare-policy metadata
- Cached revocation metadata
- Reader operational metadata
- Local schema version

SQLite will not be used as the central account, credential, fare-policy,
reconciliation, or audit database.

## Reader-local data ownership

The reader owns only the state required to perform bounded reader operations.

The reader does not own authoritative backend account state.

Reader-local state may include cached or provisional representations of:

- Credential validity
- Fare-policy data
- Revocation information
- Offline fare outcomes
- Synchronization progress

Cached data must always identify:

- Its version
- When it was retrieved
- When it becomes stale
- Which backend environment issued it
- Whether it passed integrity validation

## Initial logical tables

The final schema will be created during the reader implementation phase.

The initial logical tables are expected to include the following concepts.

### `reader_state`

Stores durable reader-specific state.

Potential fields include:

- Reader identifier
- Next local sequence number
- Last successfully acknowledged sequence number
- Reader software version
- Device-protocol version
- Current environment identifier
- Database schema version
- Created time
- Updated time

There must be exactly one active reader-state record for one reader database.

### `offline_transactions`

Stores reader-created transactions awaiting final backend resolution.

Potential fields include:

- Fare transaction identifier
- Reader identifier
- Local sequence number
- Credential reference
- Event time
- Processing mode
- Fare-policy version
- Revocation version
- Provisional fare decision
- Serialized transaction envelope
- Integrity metadata
- Queue state
- Attempt count
- Next retry time
- Created time
- Updated time

The combination of reader identifier and local sequence number must be unique.

The fare transaction identifier must also be unique.

### `synchronization_batches`

Stores each durable synchronization attempt.

Potential fields include:

- Synchronization batch identifier
- Reader identifier
- First local sequence number
- Last local sequence number
- Protocol version
- Batch state
- Attempt count
- Created time
- Sent time
- Acknowledged time
- Last error category

A synchronization batch must reference a bounded sequence range.

### `synchronization_entries`

Associates offline transactions with synchronization batches.

Potential fields include:

- Batch identifier
- Fare transaction identifier
- Local sequence number
- Entry state
- Backend result category
- Backend result reference
- Retryable flag
- Updated time

### `acknowledgements`

Stores backend acknowledgements before queued transactions are removed or
archived.

Potential fields include:

- Batch identifier
- Reader identifier
- Acknowledgement version
- Acknowledged sequence range
- Acknowledgement payload
- Integrity metadata
- Received time
- Applied time

An acknowledgement must be bound to:

- The expected reader identity
- The expected batch identifier
- The expected sequence range
- The expected protocol version

### `cached_fare_policy`

Stores the currently installed project-owned fare-policy representation.

Potential fields include:

- Fare-policy version
- Effective time
- Retrieved time
- Maximum permitted age
- Integrity metadata
- Serialized policy representation
- Activation state

### `cached_revocation_data`

Stores the currently installed project-owned revocation representation.

Potential fields include:

- Revocation version
- Retrieved time
- Maximum permitted age
- Integrity metadata
- Serialized revocation representation

### `reader_events`

Stores bounded local operational evidence required for recovery and
investigation.

Potential events include:

- Reader started
- Reader stopped
- Offline mode entered
- Offline mode exited
- Queue limit approached
- Queue limit exceeded
- Database recovery attempted
- Synchronization started
- Synchronization failed
- Synchronization acknowledged
- Policy updated
- Revocation data updated

This table is not a substitute for the backend audit system.

Retention must be bounded.

## Queue-state model

Offline transactions will use explicit queue states.

Initial states are:

- `pending`
- `in_flight`
- `acknowledged`
- `retryable_failure`
- `permanent_failure`
- `manual_review`

### `pending`

The transaction is durably stored and eligible for synchronization.

### `in_flight`

The transaction belongs to a synchronization attempt that has been durably
recorded.

An `in_flight` transaction must be recoverable after a reader restart.

A restart must not cause the transaction to disappear or become permanently
stuck.

### `acknowledged`

The backend durably accepted the transaction or returned another final,
idempotent resolution.

Acknowledged records may be retained for a bounded period before archival or
deletion.

### `retryable_failure`

The backend or network returned a failure that may succeed on another attempt.

Retry metadata must identify:

- Attempt count
- Last failure category
- Next permitted attempt time
- Maximum attempt policy

### `permanent_failure`

The backend rejected the transaction for a non-retryable reason.

Examples may include:

- Invalid signature
- Unsupported protocol version
- Unknown reader identity
- Structurally invalid transaction
- Invalid sequence state

Permanent failures must not be silently deleted.

### `manual_review`

The transaction cannot be safely resolved automatically.

Evidence must be retained for operator investigation.

## Atomic sequence assignment

Reader-local sequence assignment and transaction insertion must occur in one
SQLite transaction.

The operation must:

1. Read or atomically advance the next sequence value.
2. Assign that sequence to the new fare transaction.
3. Insert the transaction into the offline queue.
4. Persist the updated sequence state.
5. Commit all changes together.

The reader must not report a successful offline decision until the transaction
has been durably committed to the local database.

A crash must not produce:

- A reported approval with no durable transaction
- Two transactions with the same reader-local sequence number
- A sequence rollback
- A partially written transaction

Sequence gaps may occur only through an explicitly documented recovery or
rejection process.

## Synchronization-batch creation

Batch creation must be transactional.

The reader will:

1. Select a bounded ordered set of eligible transactions.
2. Generate a stable synchronization batch identifier.
3. Record the batch.
4. Associate each selected transaction with the batch.
5. Change selected transaction states to `in_flight`.
6. Commit the batch and state changes together.
7. Submit the already-recorded batch to the backend.

The batch must be reproducible after a process restart.

The reader must not generate a different batch identity merely because the
network response was lost.

## Acknowledgement processing

Acknowledgement processing must be transactional.

The reader will:

1. Validate acknowledgement integrity.
2. Verify the reader identifier.
3. Verify the batch identifier.
4. Verify the acknowledged sequence range.
5. Verify the protocol version.
6. Store the acknowledgement durably.
7. Apply per-transaction outcomes.
8. Advance the last acknowledged sequence where valid.
9. Mark resolved transactions as acknowledged or permanently failed.
10. Commit the changes together.

Queued transaction data must not be deleted before the corresponding
acknowledgement has been validated and durably applied.

A crash after acknowledgement receipt but before queue cleanup must be
recoverable without producing a duplicate business effect.

## Partial batch outcomes

The backend may accept some entries and reject others.

The reader must record each transaction outcome independently.

A partial result must not be represented as complete success.

Possible per-entry outcomes include:

- Accepted
- Already processed
- Retryable failure
- Permanent rejection
- Manual review required

Transactions with final outcomes may be resolved.

Transactions with retryable failures remain eligible for another bounded
attempt.

## Idempotency

Every offline fare transaction will have a stable fare transaction identifier.

Every synchronization batch will have a stable batch identifier.

Retries must reuse the same identifiers.

The reader must not create a new transaction identifier for the same queued
transaction merely because:

- A request timed out
- An acknowledgement was lost
- The process restarted
- The network disconnected
- The backend returned a retryable error

The backend remains responsible for durable duplicate detection.

The reader-local database ensures that retry identity survives local failures.

## SQLite operating mode

The reader database will use SQLite transaction and integrity features
appropriate for durable local queue processing.

Initial configuration requirements include:

- Foreign-key enforcement enabled
- Write-ahead logging enabled
- Busy timeout configured
- Explicit transaction boundaries
- Schema migrations
- Integrity checks during controlled recovery
- Bounded database growth
- File permissions restricted to the reader process

Durability settings must favor preservation of accepted offline transactions.

The initial implementation will use a synchronous durability setting that
does not acknowledge a queued fare transaction before required local database
writes are durably committed.

Any relaxation of durability settings requires:

- Measured performance evidence
- Documented data-loss implications
- A separate architecture decision or amendment

## Write-ahead logging

SQLite write-ahead logging will be used to improve crash recovery and permit
controlled read access while writes occur.

The reader must manage:

- WAL checkpoints
- Database growth
- Shutdown behavior
- Disk-space monitoring
- Recovery testing

The existence of a WAL file must be considered when backing up or moving the
reader database.

Copying only the primary database file while an active WAL contains committed
changes is not a valid backup procedure.

## Database migrations

Reader-local schema changes will use versioned migrations committed to the
repository.

Migration requirements include:

- Deterministic ordering
- Explicit schema version
- Upgrade testing
- Restart testing
- Failure recovery
- Protection against silent data deletion
- Compatibility with queued transactions

A reader software upgrade must not silently discard transactions created by an
older supported schema version.

Destructive migrations require an explicit recovery and rollback strategy.

## Queue limits

Offline operation must be bounded.

Configuration will define:

- Maximum queued transaction count
- Maximum queue database size
- Maximum age of a pending transaction
- Maximum offline duration
- Maximum synchronization batch size
- Maximum retry count
- Maximum simulated offline fare exposure

When a limit is reached, the reader must enter a documented controlled state.

Possible behavior includes:

- Stop accepting offline transactions
- Permit only lower-risk test operations
- Require online connectivity
- Alert an operator
- Preserve existing queued transactions

The reader must not delete unresolved transactions merely to create room for
new transactions.

## Disk-space handling

The reader must detect low or exhausted storage conditions.

Required behavior includes:

- Measure available storage where practical
- Expose queue and database-size metrics
- Warn before reaching the configured critical threshold
- Stop accepting new offline transactions before writes become unsafe
- Preserve already queued transactions
- Record an operational event
- Avoid an infinite retry loop

A fare approval must not be reported when the reader cannot durably store its
transaction.

## Corruption detection and recovery

Reader startup will validate the local database sufficiently to detect obvious
schema or integrity problems.

Recovery procedures must distinguish between:

- Temporary database lock
- Interrupted migration
- Recoverable WAL state
- Damaged database
- Unsupported schema version
- Missing database file
- Reader identity mismatch

The reader must not automatically create a new empty database over an existing
unreadable database without preserving the original evidence.

A corrupted or unsupported database should be quarantined for investigation.

Automated recovery must not silently discard unresolved transactions.

## Reader identity binding

The SQLite database must be bound to one reader identity.

At startup, the reader must verify that:

- The configured reader identifier matches stored reader state
- The environment identifier matches
- The database belongs to the expected reader
- The sequence state is valid
- The protocol metadata is supported

A database copied from another reader must not be accepted as ordinary local
state without an explicit migration or recovery workflow.

## Cached-policy handling

Cached fare policies must include:

- Fare-policy version
- Integrity metadata
- Retrieval time
- Effective time
- Expiration or maximum-age information
- Environment binding

A cached policy must not be used when:

- Integrity validation fails
- Its environment does not match
- Its version is unsupported
- It exceeds configured offline staleness limits
- Its effective-time rules prohibit use

Policy replacement must be atomic.

The reader must not expose a partially written fare policy to fare processing.

## Cached-revocation handling

Cached revocation data must include:

- Revocation version
- Integrity metadata
- Retrieval time
- Maximum-age information
- Environment binding

Revocation replacement must be atomic.

The reader must preserve its last known valid revocation representation when a
new update fails validation.

The reader must not replace valid cached data with an invalid or partially
written update.

## Sensitive-data handling

The reader database must minimize sensitive data.

The database must not store:

- Backend private signing keys
- Administrator credentials
- Passenger passwords
- Raw authentication tokens beyond strictly justified local needs
- Real payment-card information
- Real transit-authority credentials

Credential references stored in the queue must be limited to what the
project-owned protocol requires.

Logs and diagnostic output must not dump complete transaction payloads when
those payloads contain sensitive credential material.

## Encryption considerations

SQLite does not provide transparent database-file encryption in its standard
configuration.

The initial local simulation will rely on:

- Operating-system user separation
- Restricted file permissions
- Controlled development environments
- Data minimization
- Synthetic test data

A production-like deployment requiring database-at-rest encryption must add an
approved encryption mechanism through a separate design decision.

TransitGuard must not claim that the SQLite database is encrypted merely
because file permissions are restricted.

## Backup and recovery

Reader-local backup is secondary to successful backend synchronization.

The preferred recovery path is:

1. Synchronize all eligible transactions.
2. Confirm durable backend acknowledgement.
3. Preserve required local evidence.
4. Back up or replace reader-local state through a controlled process.

Any backup taken while unresolved transactions exist must include all SQLite
files required for a consistent snapshot.

Backup and restoration tests must verify:

- Reader identity binding
- Sequence continuity
- Pending transaction preservation
- Batch-state preservation
- Acknowledgement preservation
- Policy and revocation metadata

## Observability

The reader will expose metrics for:

- Queue depth
- Oldest pending transaction age
- SQLite database size
- WAL size
- Synchronization attempts
- Synchronization failures
- Retryable failures
- Permanent failures
- Manual-review count
- Last successful synchronization time
- Last acknowledged sequence
- Installed fare-policy version
- Installed revocation version
- Database-recovery events
- Queue-limit events

Telemetry must not expose private credential material.

## Testing requirements

Reader-storage testing will include:

- Transaction insertion
- Atomic sequence assignment
- Process restart
- Operating-system-style abrupt termination simulation
- Lost acknowledgement
- Duplicate batch retry
- Partial batch result
- Retryable entry failure
- Permanent entry rejection
- Sequence continuity
- Sequence-gap detection
- Queue-limit handling
- Disk-full behavior where practical
- Migration upgrade
- Interrupted migration
- Reader identity mismatch
- Cached-policy replacement
- Cached-revocation replacement
- Corruption detection
- WAL recovery
- Concurrent access restrictions

Tests must verify durable outcomes after reopening the SQLite database.

In-memory-only tests are insufficient for persistence guarantees.

## Alternatives considered

### Alternative 1: In-memory queue

An in-memory queue is simple and fast.

It was rejected because queued transactions, sequence state, and retry identity
would be lost after process or operating-system restart.

In-memory queues may still be used as temporary buffers in front of durable
storage, but they are not the authoritative offline queue.

### Alternative 2: JSON or binary files

The reader could append one file per transaction or maintain a serialized queue
file.

This was rejected because it would require custom implementation for:

- Atomic multi-record updates
- Sequence management
- Crash recovery
- Schema migration
- Partial acknowledgement
- Concurrent access
- Integrity checking
- Querying unresolved state

### Alternative 3: Embedded key-value database

An embedded key-value database could provide durable local storage.

It was not selected initially because TransitGuard's reader state has clear
relational and transactional requirements involving:

- Ordered sequences
- Batches
- Batch entries
- Acknowledgements
- Retry state
- Cached metadata

A key-value database may be reconsidered if measured reader constraints make
SQLite unsuitable.

### Alternative 4: Local PostgreSQL

A local PostgreSQL instance would provide strong relational capabilities.

It was rejected because it would add unnecessary operational complexity to
each simulated reader:

- Separate database server
- Service management
- Credentials
- Ports
- Upgrade procedures
- Higher resource use

### Alternative 5: Local message broker

A local message broker could retain queued transactions.

It was rejected because the reader also requires durable sequence,
acknowledgement, cached-policy, revocation, retry, and recovery state.

A message broker alone would not eliminate the need for local structured
storage.

## Positive consequences

SQLite provides:

- Durable embedded storage
- Atomic transactions
- Unique constraints
- Ordered queries
- Schema migrations
- Crash-recovery support
- Simple local deployment
- No separate database server
- Strong Rust ecosystem support
- Reproducible tests using real files

## Negative consequences

The decision introduces:

- SQLite schema maintenance
- WAL management
- File-permission responsibility
- Disk-space handling
- Corruption-recovery procedures
- Migration responsibility
- One-writer concurrency constraints
- Lack of built-in standard database-file encryption
- Potential platform-specific filesystem behavior

These costs are accepted because reader-local durability is necessary for
credible offline operation.

## Consequences for Rust code

The Reader Simulator will depend on a reader-local storage adapter.

Core fare and domain crates must not depend directly on SQLite.

Reader persistence types must remain distinct from domain types and protocol
types.

SQLite-specific errors must be translated into stable reader-storage error
categories.

Expected error categories include:

- Database unavailable
- Database locked
- Disk full
- Corrupt database
- Unsupported schema
- Migration failure
- Constraint violation
- Identity mismatch
- Serialization failure
- Internal storage failure

## Consequences for synchronization

Synchronization logic must load previously recorded batches rather than
reconstructing new identities after every retry.

Backend acknowledgements must be durably applied before queue cleanup.

Reader restarts must preserve:

- Pending transactions
- In-flight batches
- Retry state
- Last acknowledged sequence
- Cached policy version
- Cached revocation version

## Consequences for deployment

Each Reader Simulator instance will require:

- A dedicated local data directory
- A dedicated SQLite database file
- Restricted file permissions
- Storage-capacity limits
- Migration execution
- Recovery procedures
- Reader identity configuration

Multiple simulated readers on one machine must use separate data directories.

## Review conditions

This decision should be reviewed when:

- Reader hardware constraints are introduced
- Measured SQLite performance is inadequate
- Strong encrypted-at-rest storage becomes mandatory
- The reader requires multi-process writers
- The local queue grows beyond practical SQLite limits
- An embedded key-value store provides a clear measured advantage
- Certified hardware or secure elements become part of a separate fictional
  deployment model
- Platform filesystem behavior prevents the required durability guarantees

## Current outcome

TransitGuard Reader Simulators will use SQLite for durable local state.

SQLite will preserve reader-local sequence numbers, offline fare transactions,
synchronization batches, acknowledgements, retry state, fare-policy metadata,
and revocation metadata across process and system restarts.

PostgreSQL remains the backend system of record after transactions are
successfully synchronized and durably acknowledged.
