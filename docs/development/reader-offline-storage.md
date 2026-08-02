# Reader-Local Offline Storage

## Purpose

TransitGuard reader simulators use reader-local SQLite storage to preserve
fictional offline fare transactions and synchronization state across:

- normal process restarts;
- unexpected process termination;
- temporary network loss;
- interrupted synchronization;
- lost acknowledgement delivery;
- partial backend outcomes.

PostgreSQL remains the simulated backend system of record. SQLite represents
the local operational authority for transactions that have not yet completed
synchronization.

All readers, credentials, transactions, protocols, acknowledgements, and
backend systems described here are fictional and project-owned.

## Durability boundary

A reader must not report a successful offline fare decision until both of the
following changes have committed in one SQLite transaction:

1. the reader-local sequence has advanced;
2. the offline transaction has been inserted durably.

A failed transaction rolls back both changes.

This prevents:

- sequence reuse;
- a displayed approval without a durable transaction;
- a durable transaction without its corresponding sequence allocation.

## Database ownership

Each SQLite database is permanently bound to:

- one `ReaderId`;
- one environment identifier;
- one reader software version;
- one project-owned protocol version.

Ordinary startup rejects a database when the configured reader identity or
environment does not match the durable binding.

A reader database must never be copied between independent reader identities
and then started as though it belonged to both readers.

## SQLite configuration

Reader databases use:

- foreign-key enforcement;
- write-ahead logging;
- full synchronous durability;
- a configured busy timeout;
- a single-connection SQLx pool;
- embedded, versioned migrations.

The single-connection pool preserves predictable write ordering for the
reader-local state machine.

## Durable schema

### `reader_state`

Stores the reader database binding and sequence state:

- reader identity;
- environment identity;
- software version;
- protocol version;
- next reader-local sequence;
- last contiguous acknowledged sequence;
- creation and update timestamps.

Only one row is permitted.

### `offline_transactions`

Stores each fictional offline fare transaction:

- stable `FareTransactionId`;
- reader identity;
- reader-local sequence;
- credential identity;
- fare event time;
- fare-policy version;
- provisional fare decision;
- project-owned transaction envelope;
- queue state;
- submission attempt count;
- next retry time;
- sanitized failure category;
- creation and update timestamps.

The combination of reader identity and local sequence is unique.

Transactions are retained after acknowledgement, permanent failure, and manual
review. They are not silently deleted by synchronization processing.

### `synchronization_batches`

Stores durable upload attempts:

- stable `SynchronizationBatchId`;
- reader identity;
- project-owned protocol version;
- first and last sequence;
- lifecycle state;
- submission attempt count;
- retry scheduling;
- sanitized failure category;
- creation and update timestamps.

A retry of the same interrupted submission reuses the original batch identity.

### `synchronization_entries`

Associates ordered transaction entries with a synchronization batch.

A transaction can appear in a future batch after an earlier batch has
completed with a retryable per-entry outcome. Earlier batch history remains
available.

The same transaction cannot be assigned to multiple active batches.

### `synchronization_acknowledgements`

Stores the validated backend acknowledgement envelope before queue outcomes
are applied.

The stored acknowledgement preserves:

- batch identity;
- reader identity;
- protocol version;
- sequence range;
- receipt time;
- canonical payload;
- application time.

An identical replay is idempotent. A conflicting replay for the same batch is
rejected.

### `synchronization_acknowledgement_entries`

Stores the independent outcome for each transaction:

- acknowledged;
- retryable failure;
- permanent failure;
- manual review.

Failure outcomes preserve sanitized failure categories. Retryable outcomes
also preserve the earliest retry time.

## Queue state machine

### `pending`

The transaction is durable and has not been assigned to an active batch.

### `in_flight`

The transaction belongs to a durable prepared or submitted synchronization
batch.

### `acknowledged`

The backend accepted the transaction.

This is a retained final state.

### `retryable_failure`

The transaction remains durable and becomes eligible for a future batch after
its retry time.

### `permanent_failure`

The backend returned a final non-retryable outcome.

This is retained for diagnostics, auditing, and reconciliation review.

### `manual_review`

Automated processing cannot safely resolve the transaction.

This is retained until an explicit future operator workflow resolves it.

## Batch state machine

### `prepared`

The batch and its ordered entries have committed locally but have not yet been
submitted.

### `in_flight`

The batch has been submitted and awaits an acknowledgement or transport
resolution.

### `retryable_failure`

The same stable batch can be resubmitted after a temporary submission failure.

### `acknowledged`

A valid acknowledgement was applied atomically.

The historical batch and its entries remain stored.

### `permanent_failure`

The complete batch reached a final failure state.

### `manual_review`

The complete batch requires explicit investigation.

## Batch creation

Batch creation executes atomically:

1. select an ordered and bounded set of eligible transactions;
2. generate one stable batch identity;
3. insert the durable batch;
4. associate ordered entries;
5. move selected transactions to `in_flight`;
6. commit before network submission.

Candidate ordering uses reader-local sequence order.

Transactions already associated with an active batch are excluded.

## Submission retries

Submission attempt counters increase when a batch is submitted, not when it is
merely prepared.

A temporary transport failure moves the batch to `retryable_failure` while
preserving:

- batch identity;
- transaction identities;
- local sequence values;
- entry order;
- previous attempt count.

When a reader restarts with a batch left `in_flight`, recovery moves that batch
to `retryable_failure`. It does not create a replacement batch or replacement
transaction identities.

## Acknowledgement validation

Before an acknowledgement is stored, TransitGuard validates:

- reader identity;
- batch identity;
- project-owned protocol version;
- first and last sequence;
- entry count;
- exact transaction identity at every position;
- exact local sequence at every position;
- absence of duplicate transaction identities;
- absence of duplicate sequences;
- failure metadata;
- retry timestamps.

A new acknowledgement is accepted only while the batch is `in_flight`.

An identical acknowledgement replay returns success without duplicating rows.

A different replay for the same batch is rejected.

## Atomic acknowledgement application

Acknowledgement application executes in one SQLite transaction:

1. load the stored acknowledgement;
2. decode and validate each entry;
3. apply every per-entry queue outcome;
4. complete the historical batch;
5. advance the contiguous resolved-sequence high-water mark;
6. mark the acknowledgement as applied;
7. commit all changes together.

Any failed entry transition rolls back the complete application.

An already-applied acknowledgement returns an idempotent replay result without
modifying queue state or incrementing counters again.

## Partial outcomes

One batch may contain a mixture of:

- acknowledged entries;
- retryable failures;
- permanent failures;
- manual-review outcomes.

Each outcome is stored and applied independently.

A retryable transaction may later join a new batch while the original completed
batch remains available as historical evidence.

Permanent failures and manual-review records remain in the queue and are not
silently discarded.

## Contiguous acknowledged sequence

`last_acknowledged_sequence` represents the highest contiguous reader-local
sequence whose transactions have reached a final automated state.

The final automated states are:

- `acknowledged`;
- `permanent_failure`.

The high-water mark stops before:

- `pending`;
- `in_flight`;
- `retryable_failure`;
- `manual_review`.

A later successful retry can close an earlier sequence gap and advance the
high-water mark across already-resolved later transactions.

## Startup recovery order

Reader startup should perform recovery in this order:

1. connect to SQLite;
2. run embedded migrations;
3. validate SQLite durability configuration;
4. bind or validate reader database identity;
5. recover interrupted synchronization batches;
6. recover standalone interrupted queue entries;
7. load ready batches;
8. load ready unbatched transactions;
9. publish the reader queue-health snapshot;
10. begin ordinary reader processing.

Batch recovery occurs before standalone queue recovery so transactions assigned
to durable active batches are not detached from their batch identity.

## Operational health

`load_reader_queue_health` exposes:

- next local sequence;
- last contiguous acknowledged sequence;
- count for every queue state;
- count of prepared batches;
- count of in-flight batches;
- count of retryable batches;
- count of unapplied acknowledgements;
- lowest unresolved sequence;
- age of the oldest unresolved transaction;
- earliest scheduled retry;
- whether pending synchronization work exists;
- whether retained failures require operator attention.

Useful alert conditions include:

- oldest unresolved age exceeds the operational threshold;
- unapplied acknowledgement count is greater than zero;
- in-flight batches remain after startup recovery;
- permanent-failure count increases;
- manual-review count increases;
- next local sequence approaches storage limits;
- the reader database cannot pass integrity checks.

## Failure categories

Stored failure categories must be:

- sanitized;
- stable across versions;
- suitable for logs and metrics;
- free of credentials, keys, personal data, and raw backend responses.

Examples used by the simulator include:

- `network_timeout`;
- `reader_restart`;
- `backend_timeout`;
- `invalid_envelope`;
- `sequence_investigation`.

## Backup

A consistent backup must capture the complete SQLite database state.

Do not copy only the main database file while the reader is actively writing
in WAL mode.

Use an SQLite-aware backup procedure or stop the reader cleanly before copying
the database and related SQLite files.

A restored database remains bound to its original reader and environment.

## Corruption response

When SQLite reports corruption or an integrity failure:

1. stop fare processing;
2. preserve the database and related files for investigation;
3. do not delete unresolved transaction records;
4. do not generate replacement business identities;
5. create an operator-visible incident;
6. restore from a verified backup or follow a reviewed recovery procedure;
7. reconcile restored local state with the fictional backend before resuming.

Automatic destructive repair is outside Phase 6.

## Disk exhaustion

When the database cannot commit because storage is exhausted:

1. fail the offline transaction operation;
2. do not report a successful offline approval;
3. stop accepting additional offline transactions;
4. preserve the existing database;
5. publish an unhealthy operational state;
6. require storage remediation before resuming.

Silent queue truncation is prohibited.

## Database file permissions

The reader database should be accessible only to the operating-system account
running the fictional reader process.

Production-style deployments should additionally protect:

- the parent directory;
- backups;
- diagnostic copies;
- core dumps;
- log files containing database paths.

TransitGuard does not store real payment-card data, real transit credentials,
or real transit-authority keys.

## Scope boundary

Phase 6 implements reader-local durability and synchronization state machines.

It does not implement:

- a real network transport;
- compatibility with transit-authority protocols;
- real payment processing;
- real credential signing;
- production key management;
- automated destructive database repair;
- final retention and archival policy;
- a human operator review application.

## Validation commands

Run persistence tests:

    nix develop -c cargo test \
      -p transitguard-persistence \
      --all-features

Run workspace compilation:

    nix develop -c cargo check --workspace

Run strict Clippy validation:

    nix develop -c cargo clippy \
      --workspace \
      --all-targets \
      --all-features \
      -- \
      -D warnings

Run all workspace tests:

    nix develop -c cargo test \
      --workspace \
      --all-features

Run complete Nix validation:

    nix flake check
