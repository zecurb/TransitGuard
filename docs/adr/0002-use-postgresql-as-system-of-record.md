# ADR 0002: Use PostgreSQL as the Initial System of Record

- Status: Accepted
- Date: 2026-07-31
- Decision owners: TransitGuard maintainers
- Related documents:
  - `docs/architecture/SYSTEM_ARCHITECTURE.md`
  - `docs/architecture/DOMAIN_GLOSSARY.md`
  - `docs/security/THREAT_MODEL.md`
  - `docs/adr/0001-use-modular-rust-workspace.md`

## Context

TransitGuard requires durable storage for several related categories of state:

- Transit accounts
- Stored-value balances
- Fare credentials
- Credential lifecycle events
- Reader-equipment registrations
- Reader synchronization progress
- Fare policies
- Transit products
- Fare transactions
- Idempotency records
- Offline synchronization batches
- Reconciliation results
- Audit events
- Background jobs

Many TransitGuard operations modify several related records as one logical
business operation.

Examples include:

- Processing a fare transaction and updating stored value
- Issuing a credential and associating it with an account
- Revoking a credential and recording an audit event
- Registering a reader and provisioning its metadata
- Accepting an offline synchronization batch
- Recording reconciliation discrepancies
- Applying an approved balance adjustment

Partial persistence of these operations could violate business invariants.

TransitGuard therefore needs a persistence system that provides:

- Durable storage
- Atomic transactions
- Referential integrity
- Unique constraints
- Concurrent-access control
- Schema migrations
- Reliable query semantics
- Operational backup and restoration support
- Mature Rust integration
- Sufficient support for structured and relational data

Potential persistence approaches include:

1. PostgreSQL
2. SQLite
3. A document database
4. An event store
5. Multiple databases separated by subsystem
6. In-memory storage during early development

## Decision

TransitGuard will use PostgreSQL as its initial backend system of record.

PostgreSQL will hold the durable authoritative state managed by the
TransitGuard API and Worker.

The initial local development environment will use one PostgreSQL instance.

Logical data ownership will remain separated by domain and repository
boundaries even when the data shares one physical database.

The use of one physical PostgreSQL database does not permit unrestricted
cross-domain data access.

## Initial data ownership

The initial logical data areas are:

### Transit-account data

Includes:

- Transit accounts
- Account status
- Stored-value balances
- Eligibility classifications
- Account-level metadata

### Fare-credential data

Includes:

- Fare credential identifiers
- Credential type
- Credential status
- Credential expiration
- Account association
- Signing-key identifier
- Signing-key version
- Revocation metadata
- Replacement relationships

Private signing keys will not be stored as ordinary credential records.

### Reader-equipment data

Includes:

- Reader identifier
- Registration status
- Equipment metadata
- Reader software version
- Supported protocol version
- Last synchronization time
- Last acknowledged sequence
- Installed fare-policy version
- Installed revocation version

### Fare-policy data

Includes:

- Immutable fare-policy versions
- Effective time
- Activation status
- Fare rules
- Transfer rules
- Fare-cap rules
- Product rules
- Offline-operation limits

### Fare-transaction data

Includes:

- Fare transaction identifier
- Transit account identifier
- Fare credential identifier
- Reader identifier
- Event time
- Receipt time
- Processing time
- Fare-policy version
- Fare decision
- Simulated fare amount
- Idempotency information
- Online or offline processing mode

### Synchronization data

Includes:

- Synchronization batch identifier
- Reader identifier
- Sequence range
- Batch processing state
- Transaction-entry outcomes
- Durable acknowledgement
- Retry state
- Failure classification

### Reconciliation data

Includes:

- Reconciliation run
- Reconciliation scope
- Matched transactions
- Discrepancies
- Evidence references
- Resolution state
- Operator resolution metadata

### Audit data

Includes:

- Audit event identifier
- Actor category
- Actor identifier
- Operation
- Target resource
- Timestamp
- Correlation identifier
- Outcome
- Non-sensitive metadata

### Background-job data

Includes:

- Job identifier
- Job type
- Payload reference
- Attempt count
- Retry time
- Completion state
- Failure category
- Dead-letter or manual-review state

## Transaction requirements

Database transactions will be used whenever partial persistence could violate a
domain invariant.

Examples include:

### Fare processing

The following changes must commit or roll back together:

- Record fare transaction
- Update account stored value
- Update fare-cap progress
- Record journey or transfer state
- Record idempotency outcome
- Create required audit or domain-event records

### Credential issuance

The following changes must commit or roll back together:

- Create credential metadata
- Associate the credential with an account
- Update credential lifecycle state
- Record audit event

### Credential revocation

The following changes must commit or roll back together:

- Change credential status
- Create revocation record
- Advance revocation version where applicable
- Record audit event

### Reader synchronization

The following changes may require one or more controlled transaction
boundaries:

- Validate batch identity
- Record batch receipt
- Process transaction entries
- Advance reader synchronization cursor
- Persist acknowledgement
- Record reconciliation evidence

Large batches may be processed in bounded units rather than one unbounded
database transaction.

Partial-batch behavior must be explicit and recoverable.

## Domain separation

PostgreSQL rows and tables are persistence representations.

They are not the TransitGuard domain model.

The persistence crate will translate between:

- Database records
- Domain entities
- Domain value objects
- Application repository interfaces

The following types must not leak into the core domain crate:

- SQL query types
- PostgreSQL connection types
- Database row structs
- Migration framework types
- Database error types
- Connection-pool types

The domain crate must remain testable without PostgreSQL.

## Repository boundary

Database access will occur through implementations in:

```text
crates/persistence
```

Application and domain components will use repository or transaction
interfaces.

Deployable applications must not introduce unrelated ad hoc SQL outside the
persistence boundary.

Exceptions require documented architectural justification.

Initial repository concepts may include:

- TransitAccountRepository
- FareCredentialRepository
- ReaderRepository
- FarePolicyRepository
- FareTransactionRepository
- ReconciliationRepository
- AuditRepository
- IdempotencyRepository
- BackgroundJobRepository

Final interface names will follow the domain glossary and implementation needs.

## Schema design principles

The PostgreSQL schema will follow these principles:

- Use explicit domain-oriented names
- Use stable domain identifiers
- Use foreign-key constraints where appropriate
- Use unique constraints for protected identities
- Use check constraints where they reinforce invariants
- Use immutable records for historical events where practical
- Store timestamps with timezone-aware semantics
- Avoid storing secrets in ordinary business tables
- Keep migrations versioned in the repository
- Avoid destructive migration behavior without an explicit recovery plan

Potential table names include:

```text
transit_accounts
fare_credentials
credential_revocations
reader_equipment
fare_policies
transit_products
fare_transactions
synchronization_batches
synchronization_entries
reconciliation_runs
reconciliation_discrepancies
audit_events
idempotency_records
background_jobs
```

These names are provisional until the persistence phase defines the schema.

## Identifier strategy

Domain identifiers will be distinct from authentication secrets.

Identifiers must support:

- Stable references
- Idempotency
- Auditing
- Reconciliation
- External API use where approved
- Distributed creation where required

The project may use UUID-compatible identifiers or strongly typed wrappers
around another selected identifier representation.

The final identifier format will be documented separately.

The schema should avoid exposing internal sequential database keys as public
resource identifiers when a domain identifier is available.

## Idempotency storage

Protected write operations will use durable idempotency records.

An idempotency record may include:

- Idempotency key
- Actor identity or scope
- Operation type
- Request fingerprint
- Processing state
- Result reference
- Error category
- Creation time
- Expiration or retention time

A repeated request with the same key and equivalent request must return an
equivalent result without producing a duplicate business effect.

Reuse of the same key with a materially different request must produce an
idempotency conflict.

Idempotency checks and protected business mutations must use transaction
boundaries that prevent concurrent duplicate effects.

## Concurrency control

TransitGuard must protect against conflicting concurrent updates.

Examples include:

- Two simultaneous fare transactions reducing one balance
- A fare transaction racing with account suspension
- Credential revocation racing with fare processing
- Two workers processing the same job
- Multiple synchronization attempts for the same reader batch
- Concurrent fare-policy activation

The persistence implementation may use:

- Row-level locking
- Optimistic concurrency versions
- Unique constraints
- Transaction isolation
- Atomic update statements
- Advisory locks for narrowly justified cases

Concurrency behavior must be tested.

Correctness must not depend solely on requests arriving one at a time.

## Audit storage

Audit records will be append-oriented.

Ordinary application behavior must not silently rewrite or delete existing
audit records.

Audit-storage controls will include:

- Restricted update behavior
- Restricted deletion behavior
- Actor attribution
- Correlation identifiers
- Non-sensitive metadata
- Retention policy
- Backup protection

A future decision may add tamper-evident chaining, signing, or external audit
storage.

PostgreSQL is the initial storage mechanism, but this ADR does not claim that
ordinary relational storage alone makes audit records tamper-proof.

## Reconciliation evidence

Reconciliation records must preserve enough evidence to explain discrepancies.

A discrepancy record should reference:

- Reconciliation run
- Fare transaction
- Reader
- Synchronization batch
- Source records
- Classification
- Detection time
- Resolution state

Manual resolution must not destroy the original discrepancy evidence.

Resolution should be recorded as additional state or events.

## Background jobs

PostgreSQL may initially store durable background jobs.

This avoids introducing a separate message broker before workload and
availability requirements justify one.

Initial job storage must support:

- Durable job identity
- Job type
- Scheduled execution time
- Attempt count
- Retry delay
- Lease or claim ownership
- Completion state
- Failure state
- Manual-review state

Workers must claim jobs safely so that multiple worker processes do not
intentionally execute the same non-idempotent effect.

A future message broker may be introduced through another ADR when justified
by throughput, delivery, isolation, or operational requirements.

## Migrations

Database schema changes will be managed through versioned migrations committed
to the repository.

Migration requirements include:

- Deterministic ordering
- Reviewable SQL or migration code
- Continuous-integration validation
- Local reproducibility
- Forward migration testing
- Rollback or recovery planning
- Protection against accidental data loss
- Documentation for long-running migrations

Production-like migrations must not assume that all tables are empty.

A future implementation decision will select the Rust migration tooling.

## Backup and restoration

PostgreSQL backup and restoration procedures will be documented before a
production-oriented release.

The procedures must cover:

- Backup creation
- Backup storage
- Access control
- Retention
- Restoration testing
- Recovery-point expectations
- Recovery-time expectations
- Schema-version compatibility
- Encryption requirements for protected environments

A backup that has never been restored in a test does not provide adequate
evidence of recoverability.

## Security requirements

PostgreSQL deployments must use:

- Authenticated database access
- Least-privilege roles
- Separate credentials for distinct application responsibilities where useful
- Parameterized queries
- Bounded connection pools
- Query timeouts
- Secret-safe configuration
- Protected backups
- Error redaction

Network-deployed PostgreSQL instances must use appropriate transport
protection.

The database must not be directly exposed to passenger, administration, web,
mobile, or reader clients.

Only approved backend processes may access the database.

## Privacy requirements

TransitGuard should use synthetic test data.

Persistence design must minimize unnecessary personal information.

The database must not store:

- Real payment-card numbers
- Payment-card security codes
- Real banking credentials
- Private cryptographic keys in ordinary account tables
- Authentication tokens in plaintext where protected storage is required

Journey and account data will receive an explicit retention policy before
production-oriented release quality is claimed.

## Operational requirements

The PostgreSQL integration must expose operational signals for:

- Connection-pool usage
- Connection wait time
- Query duration
- Transaction duration
- Transaction rollback count
- Constraint violations
- Deadlocks
- Migration status
- Database availability
- Background-job queue depth
- Reconciliation backlog

Sensitive SQL parameters must not appear in general telemetry.

## Failure behavior

TransitGuard must define behavior for:

- Database unavailable
- Connection pool exhausted
- Transaction timeout
- Serialization conflict
- Deadlock
- Constraint violation
- Migration failure
- Disk exhaustion
- Read-only database state
- Backup failure

The API must not report success before required durable changes have committed.

The Worker must not mark a job complete before required durable changes have
committed.

Retryable database failures must use bounded retries.

Permanent failures must be surfaced through explicit errors, failed jobs,
alerts, or manual-review workflows.

## Alternatives considered

### Alternative 1: SQLite

SQLite would provide simple setup and transactional relational storage.

It remains useful for isolated reader-local storage or focused tests.

It was not selected as the central backend system of record because
TransitGuard is intended to model:

- Concurrent API and worker processes
- Multiple database connections
- Production-oriented operational behavior
- Server-based backup and restoration
- Strong multi-user database administration
- Scalable relational queries

This ADR does not prohibit SQLite for the reader simulator's local durable
offline queue.

Reader-local storage will receive a separate decision.

### Alternative 2: Document database

A document database could store flexible records and nested transaction
documents.

It was not selected because TransitGuard has strongly related data and requires
transactional consistency across accounts, credentials, transactions,
idempotency records, and audit evidence.

PostgreSQL can also support selected semi-structured fields without replacing
relational integrity.

### Alternative 3: Event store as the primary database

Event sourcing could preserve a complete history of domain changes.

It was not selected as the initial persistence model because it would add
complexity involving:

- Event-schema evolution
- Projection rebuilding
- Event versioning
- Eventual consistency
- Operational debugging
- Snapshotting
- Idempotent event handling

TransitGuard may still use domain events and append-oriented historical
records without making event sourcing the initial system of record.

A future event-sourcing proposal requires a separate ADR.

### Alternative 4: One database per crate or subsystem

Multiple databases would create stronger physical separation.

It was not selected initially because it would introduce:

- Distributed transactions
- Cross-database consistency problems
- Additional deployment complexity
- Additional backup procedures
- More credentials and connection pools
- More complex local development

Logical ownership and repository boundaries will be established before
physical database separation is considered.

### Alternative 5: In-memory persistence

In-memory repositories are useful for unit tests and early demonstrations.

They were not selected as the system of record because they do not provide
durability, realistic concurrency behavior, migrations, backup, or restoration.

In-memory implementations may still be used for tests.

## Positive consequences

Using PostgreSQL provides:

- Mature transactional behavior
- Referential integrity
- Unique and check constraints
- Reliable concurrent-access semantics
- Strong query capabilities
- Schema migration support
- Backup and restoration tooling
- Mature Rust client libraries
- One consistent backend persistence platform
- A practical path toward production-oriented operations

## Negative consequences

The decision introduces:

- Database setup requirements
- Schema migration responsibility
- Connection-pool management
- Backup and restoration responsibility
- Query-performance monitoring
- Transaction-design complexity
- Potential coupling through shared physical storage
- Operational dependency on one central database

These costs are accepted because durable relational consistency is central to
TransitGuard.

## Consequences for Rust code

The persistence crate will own concrete PostgreSQL integration.

The domain crate must not depend on PostgreSQL libraries.

Application use cases will depend on repository and transaction abstractions.

PostgreSQL-specific errors will be translated into stable application or
infrastructure error categories.

SQL and database-record definitions must not become public domain APIs.

## Consequences for testing

Testing will include:

- Domain tests without PostgreSQL
- Repository integration tests with PostgreSQL
- Migration tests
- Constraint tests
- Transaction rollback tests
- Concurrency tests
- Idempotency tests
- Connection-failure tests
- Reconciliation persistence tests
- Backup and restoration exercises before release quality

Continuous integration may use an ephemeral PostgreSQL service once database
implementation begins.

## Consequences for deployment

The initial deployment requires:

- PostgreSQL instance
- Database credentials
- Migration execution
- Connection-pool configuration
- Backup strategy
- Health monitoring

API and Worker processes may use different database roles when distinct
permissions are implemented.

## Review conditions

This decision should be reviewed when:

- One subsystem requires independent scaling
- A subsystem requires stronger data isolation
- Database load becomes a system bottleneck
- A separate analytical store is required
- A message broker becomes operationally necessary
- Event sourcing is proposed
- Geographic distribution changes consistency requirements
- Regulatory requirements require physical data separation
- PostgreSQL no longer satisfies availability or performance requirements

## Current outcome

PostgreSQL is TransitGuard's initial backend system of record.

It will provide durable relational storage for accounts, credentials, readers,
fare policies, transactions, synchronization state, reconciliation evidence,
audit events, idempotency records, and background jobs.

Domain boundaries will remain explicit even while the initial implementation
uses one physical PostgreSQL database.
