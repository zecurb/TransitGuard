# TransitGuard Domain Glossary

## 1. Purpose

This glossary defines the official terminology used throughout TransitGuard.

These definitions apply to:

- Rust type names
- Rust module names
- Database schemas
- API resources
- Protocol messages
- Tests
- Metrics
- Logs
- Architecture documents
- Operational documentation
- Web and mobile interfaces

Consistent terminology prevents the same concept from being represented by
multiple conflicting names.

TransitGuard is a fictional and project-owned platform. Its terminology does
not claim compatibility with any real transit authority, fare system, payment
network, transit card, reader, or transportation protocol.

## 2. Naming principles

TransitGuard terminology follows these principles:

- One important domain concept should have one preferred name.
- Type names should describe business meaning rather than storage format.
- Database terminology must not replace domain terminology.
- Protocol terminology must remain separate from domain terminology.
- Security-sensitive values must use names that clearly communicate their
  sensitivity.
- Simulated financial values must not be presented as actual money movement.
- Terms borrowed from real transit systems must be used only in a generic
  descriptive sense.
- Project-owned credentials and protocols must never be described as certified
  or production-authority credentials.

## 3. Organizational concepts

### TransitGuard

The complete fictional transit fare-processing platform implemented by this
repository.

TransitGuard includes:

- Backend APIs
- Background workers
- Reader simulators
- Passenger-facing clients
- Administrative clients
- Domain libraries
- Security abstractions
- Persistence components
- Reconciliation components
- Observability components

### Fictional Transit Authority

The simulated organization operating TransitGuard.

The fictional transit authority:

- Defines fare policies
- Issues project-owned credentials
- Registers project-owned reader equipment
- Manages rider accounts
- Reviews reconciliation results
- Operates the administrative interface

The project must not reuse the identity, branding, private infrastructure,
credentials, or protected protocols of a real transit authority.

### Administrator

An authenticated actor authorized to perform administrative operations.

Examples include:

- Issuing credentials
- Revoking credentials
- Registering readers
- Disabling readers
- Activating fare policies
- Reviewing discrepancies
- Performing approved balance adjustments

An administrator is not automatically authorized to perform every
administrative action. Authorization depends on assigned permissions or roles.

### Operator

A person responsible for operating or monitoring simulated transit equipment.

An operator may:

- View reader status
- Start or stop a reader simulator
- Review synchronization failures
- Inspect non-sensitive operational telemetry

An operator is distinct from an administrator unless explicitly granted
administrative permissions.

### Passenger

A person using the fictional transit platform.

A passenger may own or use:

- A transit account
- A simulated card credential
- A simulated mobile credential
- Stored value
- Transit products
- Journey history

### Actor

Any authenticated or identified party performing an operation.

Actor categories include:

- Passenger
- Administrator
- Operator
- Reader equipment
- Background worker
- Internal service
- Development operator

An actor identifier must identify the source of an auditable action without
exposing secret authentication material.

## 4. Rider and account concepts

### Rider

The domain representation of a passenger recognized by TransitGuard.

A rider may be associated with:

- One transit account
- Multiple historical credentials
- One or more active credentials
- Eligibility classifications
- Journey history
- Transit products

The term `Rider` is preferred for core domain types.

The term `Passenger` is preferred when describing a human actor or user
interface.

### Transit Account

The aggregate that owns a rider's simulated fare-related state.

A transit account may contain:

- Stored-value balance
- Active transit products
- Credential associations
- Fare-cap progress
- Journey history
- Account status

Preferred Rust name:

```text
TransitAccount
```

Avoid using the generic term `Account` where ambiguity could exist with
administrator accounts, system accounts, or database accounts.

### Account Identifier

A stable identifier assigned to a transit account.

Preferred Rust name:

```text
TransitAccountId
```

An account identifier is not a credential secret and must not be used as proof
of authentication.

### Account Status

The current operational state of a transit account.

Initial values may include:

- Active
- Suspended
- Closed

A suspended or closed account must not be treated as active merely because one
of its credentials appears structurally valid.

### Eligibility Classification

A project-owned classification that may affect fare calculation.

Examples may include:

- Standard
- Youth
- Senior
- Reduced Fare
- Employee Test Account

Eligibility classifications are simulated and must not claim official
qualification under a real transportation program.

## 5. Credential concepts

### Fare Credential

A project-owned representation used to identify a rider or transit account
during a simulated fare interaction.

A fare credential may be represented through:

- A simulated physical card
- A simulated mobile credential
- A development-only test token

Preferred Rust name:

```text
FareCredential
```

A fare credential is not a bank card, payment card, government credential, or
real transit credential.

### Card Credential

A fare credential represented by a project-owned simulated transit card.

Preferred Rust name:

```text
CardCredential
```

A card credential must not reproduce a proprietary card format or protected
real-world transit credential.

### Mobile Credential

A fare credential represented by the TransitGuard passenger mobile
application.

Preferred Rust name:

```text
MobileCredential
```

The mobile credential may display or transmit project-owned test data.

It must not impersonate a real transit credential or mobile-wallet payment
credential.

### Credential Identifier

A stable, non-secret identifier assigned to a fare credential.

Preferred Rust name:

```text
FareCredentialId
```

A credential identifier may appear in internal records after appropriate
redaction or tokenization.

It must not be treated as a private signing key or authentication secret.

### Credential Material

The data required to represent, authenticate, sign, or verify a fare
credential.

Credential material may include:

- Public identifiers
- Key identifiers
- Public verification data
- Signed claims
- Expiration information

Sensitive credential material must not appear in logs, metrics, public error
messages, screenshots, or repository commits.

### Credential Status

The current lifecycle state of a fare credential.

Initial states may include:

- Pending
- Active
- Suspended
- Revoked
- Expired
- Replaced

A revoked, expired, or replaced credential must not be accepted as active.

### Credential Issuance

The controlled process that creates and activates a new fare credential.

Issuance includes:

- Generating a credential identifier
- Associating the credential with an account
- Creating or signing credential material
- Persisting credential metadata
- Recording an audit event

### Credential Replacement

The process that creates a new credential to replace an existing credential.

Replacement normally includes:

- Revoking or retiring the previous credential
- Issuing a new credential
- Preserving the account relationship
- Recording an audit event

### Credential Revocation

The process of making a credential invalid before its natural expiration.

Revocation must be:

- Idempotent
- Auditable
- Versioned for reader distribution
- Visible in synchronization status

### Revocation Reason

The documented reason a credential was revoked.

Initial examples may include:

- Reported Lost
- Reported Stolen
- Replaced
- Account Suspended
- Administrative Action
- Security Incident
- Test Cleanup

### Revocation List

A versioned collection or representation of revoked credential information
distributed to reader equipment.

Preferred Rust name:

```text
RevocationList
```

A revocation list must not expose unnecessary credential secrets.

### Revocation Version

A monotonically increasing or otherwise ordered identifier representing a
specific revocation-list state.

Preferred Rust name:

```text
RevocationVersion
```

Readers report their installed revocation version during synchronization.

## 6. Reader and equipment concepts

### Reader Equipment

A project-owned simulated device that accepts fare-credential presentations
and produces fare transactions.

Preferred Rust name:

```text
ReaderEquipment
```

Reader equipment may operate online or offline.

It does not represent certified, installed, or production transportation
equipment.

### Reader Simulator

The TransitGuard executable that simulates reader-equipment behavior.

Repository location:

```text
apps/reader-simulator
```

The reader simulator:

- Accepts simulated taps
- Uses project-owned policies
- Creates fare transactions
- Maintains an offline queue
- Synchronizes with the backend
- Reports health and version information

### Reader Identifier

A stable identifier assigned to registered reader equipment.

Preferred Rust name:

```text
ReaderId
```

A reader identifier is not an equipment private key.

### Equipment Identity

The identity and associated authentication material used to establish which
registered reader is communicating with the backend.

Preferred Rust concept:

```text
EquipmentIdentity
```

Equipment identity verification must be separate from ordinary passenger or
administrator authentication.

### Reader Registration

The controlled process that establishes a reader as recognized equipment.

Registration may include:

- Generating a reader identifier
- Provisioning development equipment credentials
- Recording equipment metadata
- Recording protocol compatibility
- Creating an audit event

### Reader Disablement

The administrative process that prevents a registered reader from submitting
accepted transactions.

Disablement may be used for:

- Decommissioning
- Suspected compromise
- Lost equipment
- Invalid configuration
- Test cleanup

### Reader Status

The backend's current understanding of a reader's operational state.

Initial states may include:

- Pending Registration
- Active
- Offline
- Disabled
- Revoked
- Decommissioned

### Reader Health

Operational information reported by or inferred about a reader.

Reader health may include:

- Last synchronization time
- Offline queue depth
- Oldest queued transaction age
- Installed software version
- Installed fare-policy version
- Installed revocation version
- Authentication failures
- Synchronization failures

### Reader Software Version

The version of TransitGuard reader-simulator software reported by a reader.

Preferred Rust name:

```text
ReaderSoftwareVersion
```

### Local Sequence Number

A reader-generated monotonically increasing number assigned to ordered local
transactions.

Preferred Rust name:

```text
LocalSequenceNumber
```

Local sequence numbers support:

- Ordering
- Gap detection
- Duplicate detection
- Replay detection
- Reconciliation

A local sequence number is scoped to one reader identity unless explicitly
designed otherwise.

## 7. Fare-policy concepts

### Fare Policy

A versioned collection of rules used to evaluate simulated transit fares.

Preferred Rust name:

```text
FarePolicy
```

A fare policy may define:

- Base fares
- Zone rules
- Transfer rules
- Fare caps
- Product validity
- Eligibility discounts
- Offline limits
- Effective dates

### Fare Policy Version

The immutable identifier of a fare-policy revision.

Preferred Rust name:

```text
FarePolicyVersion
```

Every fare decision must record the policy version used to produce it.

### Fare Rule

One specific rule within a fare policy.

Examples include:

- Charge a base fare
- Permit a transfer within a defined window
- Apply a daily cap
- Reject an expired product
- Apply a simulated eligibility discount

### Active Fare Policy

The fare-policy version currently designated for new fare decisions.

Activation must be:

- Authorized
- Audited
- Explicit
- Reversible through another versioned policy change

Existing transaction records must continue to identify the exact policy
version originally used.

### Base Fare

The starting simulated fare amount before transfers, discounts, products, or
caps are applied.

### Fare Adjustment

A positive or negative change applied during fare calculation.

Examples include:

- Transfer credit
- Eligibility discount
- Fare-cap reduction
- Administrative correction
- Reconciliation adjustment

### Fare Cap

A policy rule limiting the cumulative simulated fare charged over a period.

Examples may include:

- Daily fare cap
- Weekly fare cap

Preferred Rust concepts:

```text
FareCap
FareCapProgress
```

### Transfer Window

A time interval in which a subsequent eligible journey leg may receive a
transfer benefit.

Preferred Rust name:

```text
TransferWindow
```

### Zone

A project-owned geographic or logical fare area used for simulated zone-based
fare calculation.

Preferred Rust name:

```text
FareZone
```

TransitGuard zones must not copy proprietary fare-zone data when such data is
not legitimately available for reuse.

## 8. Value and product concepts

### Stored Value

A simulated fare balance held by a transit account.

Preferred Rust name:

```text
StoredValueBalance
```

Stored value does not represent a bank deposit, actual wallet balance, or
regulated financial account.

### Simulated Funds

Project-owned test value added to a transit account for development and
demonstration.

Simulated funds:

- Have no cash value
- Cannot be withdrawn
- Cannot be transferred outside TransitGuard
- Must not be represented as an actual financial payment

### Balance Adjustment

An authorized change to stored value that does not originate from normal fare
processing.

Examples include:

- Development account setup
- Test correction
- Approved reconciliation correction
- Administrative demonstration

Balance adjustments must be auditable.

### Transit Product

A project-owned entitlement affecting fare eligibility.

Examples may include:

- Single Ride
- Day Pass
- Weekly Pass
- Monthly Pass
- Development Test Pass

Preferred Rust name:

```text
TransitProduct
```

### Product Instance

A purchased or assigned transit product associated with a specific account.

Preferred Rust name:

```text
TransitProductInstance
```

A product instance may include:

- Product identifier
- Validity period
- Activation status
- Remaining uses
- Purchase or assignment source

### Product Validity

The rules determining whether a transit product may be used for a fare
transaction.

Validity may depend on:

- Activation time
- Expiration time
- Route or zone eligibility
- Usage limit
- Account status
- Credential status

## 9. Journey and transaction concepts

### Tap

The simulated act of presenting a fare credential to reader equipment.

A tap is an interaction event.

A tap does not automatically imply approval, charging, or journey creation.

### Tap Request

The input collected by a reader when a credential is presented.

A tap request may contain:

- Reader identifier
- Credential reference
- Event time
- Local sequence number
- Fare-policy version
- Protocol version
- Correlation identifier

### Fare Transaction

The authoritative domain record describing the result of processing a tap.

Preferred Rust name:

```text
FareTransaction
```

A fare transaction may represent:

- Approved fare
- Rejected fare
- Zero-value transfer
- Product-covered fare
- Offline provisional approval
- Reconciliation adjustment

### Fare Transaction Identifier

A globally unique or otherwise collision-resistant identifier assigned to a
fare transaction.

Preferred Rust name:

```text
FareTransactionId
```

The identifier supports:

- Idempotency
- Synchronization
- Reconciliation
- Auditing
- Investigation

### Fare Decision

The deterministic result produced by the fare engine.

Preferred Rust name:

```text
FareDecision
```

A fare decision may include:

- Approval status
- Fare amount
- Applied product
- Applied transfer
- Applied discount
- Fare-cap effect
- Policy version
- Decision reason

### Approved Transaction

A fare transaction whose fare decision permits the simulated journey event.

Approval does not imply that a real payment occurred.

### Rejected Transaction

A fare transaction whose fare decision denies the simulated journey event.

Initial rejection reasons may include:

- Credential Revoked
- Credential Expired
- Account Suspended
- Reader Disabled
- Invalid Signature
- Unsupported Protocol
- Insufficient Stored Value
- Product Invalid
- Offline Limit Exceeded
- Stale Policy
- Stale Revocation Data

### Journey

A logical passenger movement represented by one or more eligible journey legs.

Preferred Rust name:

```text
Journey
```

A journey may be assembled after transaction processing when sufficient
transaction context exists.

### Journey Leg

One segment of a journey associated with a fare transaction or pair of transit
events.

Preferred Rust name:

```text
JourneyLeg
```

### Transfer

A relationship between eligible journey legs that modifies fare calculation.

Preferred Rust name:

```text
Transfer
```

### Event Time

The time at which the reader reports that a simulated tap occurred.

Preferred Rust name:

```text
EventTime
```

Event time is distinct from backend receipt time.

### Receipt Time

The time at which the backend receives a transaction.

Preferred Rust name:

```text
ReceivedAt
```

Offline transactions may have a significant difference between event time and
receipt time.

### Processing Time

The time at which TransitGuard performs a particular processing operation.

Processing time may differ from both event time and receipt time.

## 10. Online and offline concepts

### Online Mode

Reader operation while the backend is reachable and the transaction can be
submitted during the tap workflow.

### Offline Mode

Reader operation while the backend is unavailable or designated offline
operation is active.

Offline mode uses:

- Cached fare policy
- Cached revocation information
- Durable local transaction queue
- Local sequence numbers
- Offline limits

### Offline Transaction

A fare transaction created while the reader is operating offline.

Preferred Rust name:

```text
OfflineFareTransaction
```

An offline transaction may remain provisional until backend synchronization
and reconciliation are complete.

### Offline Approval

A provisional approval produced by a reader using cached information.

Offline approval must be bounded by configured risk controls.

### Offline Exposure

The maximum simulated fare or transaction risk accumulated while a reader is
offline.

Preferred Rust name:

```text
OfflineExposure
```

### Offline Duration

The period during which a reader has not successfully synchronized with the
backend.

### Offline Queue

The durable local collection of transactions awaiting synchronization.

Preferred Rust name:

```text
OfflineTransactionQueue
```

The queue must survive reader-process restarts.

### Queue Depth

The number of transactions currently awaiting synchronization.

### Oldest Queued Transaction Age

The elapsed time since the oldest unsynchronized transaction was created.

This is an important operational metric.

### Stale Fare Policy

A cached fare-policy version older than the maximum permitted age or version
distance.

### Stale Revocation Data

Cached revocation information older than the maximum permitted age or version
distance.

A reader with stale policy or revocation data may be required to reduce or
stop offline approvals.

## 11. Synchronization concepts

### Synchronization

The process of exchanging reader state, queued transactions, acknowledgements,
fare-policy metadata, and revocation metadata with the backend.

Preferred shortened term:

```text
Sync
```

Use `Synchronization` in formal documentation and domain type names when
clarity is important.

### Synchronization Batch

An ordered, bounded collection of reader transactions submitted together.

Preferred Rust name:

```text
SynchronizationBatch
```

A synchronization batch includes:

- Batch identifier
- Reader identifier
- Protocol version
- First local sequence number
- Last local sequence number
- Transaction entries
- Reader version metadata

### Batch Identifier

A stable identifier assigned to a synchronization batch.

Preferred Rust name:

```text
SynchronizationBatchId
```

### Batch Acknowledgement

The backend response describing the processing result for a synchronization
batch.

Preferred Rust name:

```text
BatchAcknowledgement
```

An acknowledgement may contain:

- Accepted transactions
- Duplicate transactions
- Rejected transactions
- Retryable failures
- Permanent failures
- Latest policy version
- Latest revocation version

### Partial Batch Failure

A synchronization result where some entries succeed and others fail.

Partial failures must be explicit.

The system must not report the entire batch as successful when individual
entries were rejected.

### Sequence Gap

A missing local sequence number detected during synchronization.

A sequence gap may indicate:

- Missing transaction
- Reader data loss
- Out-of-order batch
- Duplicate batch
- Implementation defect
- Tampering attempt

### Synchronization Cursor

The backend's durable record of reader synchronization progress.

Preferred Rust name:

```text
SynchronizationCursor
```

### Last Acknowledged Sequence

The highest reader-local sequence number durably accepted or otherwise
resolved by the backend.

Preferred Rust name:

```text
LastAcknowledgedSequence
```

## 12. Idempotency and replay concepts

### Idempotency

The property that repeating an operation with the same identity does not
produce an unintended duplicate effect.

Examples include:

- Charging stored value only once
- Issuing one credential for one issuance request
- Recording one transaction for one transaction identifier
- Applying one balance adjustment once

### Idempotency Key

A caller-supplied or system-generated identifier used to recognize repeated
requests.

Preferred Rust name:

```text
IdempotencyKey
```

An idempotency key must be scoped to a defined operation and actor.

### Idempotency Record

A durable record of an operation's idempotency key, request identity, status,
and result reference.

Preferred Rust name:

```text
IdempotencyRecord
```

### Duplicate Submission

A repeated submission of an already known transaction or request.

A duplicate submission is not automatically malicious.

It may result from:

- Network timeout
- Lost acknowledgement
- Reader retry
- User retry
- Worker retry

### Duplicate Effect

An incorrect second business-state change caused by a repeated request.

TransitGuard must prevent duplicate effects for protected operations.

### Replay

The reuse of a previously valid message in a context where it should no longer
be accepted.

Replay protection may use:

- Sequence numbers
- Nonces
- Timestamps
- Expiration
- Message identifiers
- Durable processed-message records

### Replay Detection

The process of determining that a message has already been accepted or is no
longer valid for its claimed sequence or time.

### Nonce

A single-use or context-bounded value used to reduce replay risk.

Preferred Rust name:

```text
Nonce
```

A nonce is not a substitute for complete authentication, authorization, or
idempotency design.

## 13. Reconciliation concepts

### Reconciliation

The process of comparing related transaction records from multiple sources to
detect agreement or discrepancy.

Sources may include:

- Reader transaction queue
- Backend fare transactions
- Batch acknowledgements
- Audit records
- Account ledger entries

Preferred Rust name:

```text
Reconciliation
```

### Reconciliation Run

One bounded execution of reconciliation over a defined reader, account, batch,
or time range.

Preferred Rust name:

```text
ReconciliationRun
```

### Reconciliation Result

The complete outcome of a reconciliation run.

Preferred Rust name:

```text
ReconciliationResult
```

### Matched Transaction

A transaction whose relevant source records agree.

### Missing Transaction

A transaction present in one expected source but absent from another.

### Duplicate Transaction

Multiple records representing what should be one transaction.

### Conflicting Transaction

Records sharing an identity but containing incompatible values.

Examples include disagreement in:

- Fare amount
- Credential identifier
- Reader identifier
- Event time
- Policy version
- Approval status

### Late-Arriving Transaction

A transaction received after the normal synchronization or reconciliation
window.

Late arrival does not automatically make the transaction invalid.

### Invalid Ordering

A reader transaction sequence that violates expected local sequence ordering.

### Discrepancy

A reconciliation outcome requiring classification, investigation, correction,
or explicit acceptance.

Preferred Rust name:

```text
ReconciliationDiscrepancy
```

### Discrepancy Identifier

A stable identifier assigned to a reconciliation discrepancy.

Preferred Rust name:

```text
DiscrepancyId
```

### Discrepancy Classification

The category assigned to a discrepancy.

Initial classifications may include:

- Missing
- Duplicate
- Conflicting
- Invalid Ordering
- Invalid Signature
- Unknown Reader
- Late Arrival
- Unsupported Version
- Manual Review

### Resolution

The documented action that closes or otherwise addresses a discrepancy.

Initial resolution outcomes may include:

- Confirmed Correct
- Corrected
- Duplicate Removed
- Accepted Late Arrival
- Reader Data Loss
- Administrative Adjustment
- Security Escalation
- No Action Required

### Manual Review

A state indicating that automated reconciliation cannot safely determine the
correct outcome.

Manual review must preserve the evidence that caused the escalation.

## 14. Security concepts

### Authentication

The process of establishing an actor's identity.

Authentication answers:

```text
Who or what is making this request?
```

### Authorization

The process of determining whether an authenticated actor may perform a
specific operation.

Authorization answers:

```text
May this actor perform this action on this resource?
```

Authentication and authorization are separate requirements.

### Signing Key

A private cryptographic key used to produce a signature.

Preferred Rust concept:

```text
SigningKey
```

Signing keys are sensitive secret material.

They must not be committed, logged, displayed, or returned through public APIs.

### Verification Key

A key used to verify a signature.

Preferred Rust concept:

```text
VerificationKey
```

Verification keys may be public depending on the selected cryptographic design.

### Key Identifier

A non-secret identifier used to select a cryptographic key.

Preferred Rust name:

```text
KeyId
```

### Key Version

The version of a cryptographic key within a managed key lifecycle.

Preferred Rust name:

```text
KeyVersion
```

### Key Rotation

The controlled process of introducing a new key version and retiring an older
version.

Rotation must preserve enough metadata to verify historical records where
required.

### Development Key

A clearly marked project-owned key used only for local development, automated
testing, or demonstration.

Development keys must not be described as production-safe merely because they
use real cryptographic algorithms.

### Secret

Sensitive information whose disclosure could undermine authentication,
authorization, confidentiality, or integrity.

Examples include:

- Private keys
- Authentication tokens
- Database passwords
- Session secrets
- Credential secrets

### Redaction

The removal or replacement of sensitive data before information is logged,
displayed, stored in diagnostics, or returned in an error.

Preferred Rust concept:

```text
Redacted
```

### Trust Boundary

A point where data or control moves between components with different trust
assumptions.

Every trust-boundary crossing requires deliberate validation and security
controls.

## 15. Audit and observability concepts

### Audit Event

An immutable or append-oriented record of a security-relevant or
business-significant action.

Preferred Rust name:

```text
AuditEvent
```

Audit events describe:

- Who acted
- What action occurred
- Which resource was affected
- When it occurred
- Whether it succeeded
- Why it occurred when justification is required

### Audit Identifier

A stable identifier assigned to an audit event.

Preferred Rust name:

```text
AuditEventId
```

### Correlation Identifier

An identifier used to associate logs, traces, requests, jobs, transactions,
and audit events belonging to the same broader operation.

Preferred Rust name:

```text
CorrelationId
```

### Request Identifier

An identifier assigned to one API request.

Preferred Rust name:

```text
RequestId
```

A request identifier may be part of a larger correlation context.

### Job Identifier

An identifier assigned to one background job.

Preferred Rust name:

```text
JobId
```

### Structured Log

A machine-readable log event composed of defined fields rather than only free
text.

### Metric

A numeric observation aggregated for operational monitoring.

Examples include:

- Request duration
- Error count
- Queue depth
- Reconciliation discrepancy count
- Reader policy-version lag

### Trace

A linked representation of operations performed across components during one
request, transaction, or background workflow.

### Health Check

A check indicating whether a process is running and able to report basic
status.

### Readiness Check

A check indicating whether a process is prepared to receive and correctly
process work.

A process may be healthy but not ready.

## 16. Application architecture concepts

### Entity

A domain object defined by a stable identity across state changes.

Examples include:

- Transit account
- Fare credential
- Reader equipment
- Fare transaction

### Value Object

An immutable domain concept defined by its validated value rather than an
independent identity.

Examples may include:

- Fare amount
- Policy version
- Reader identifier
- Credential identifier
- Time window

### Aggregate

A consistency boundary containing one or more related domain objects.

Changes to an aggregate must preserve its invariants.

### Aggregate Root

The entity through which modifications to an aggregate are coordinated.

Potential TransitGuard aggregate roots include:

- Transit account
- Reader equipment
- Fare policy
- Reconciliation case

Final aggregate boundaries will be documented through implementation and
Architecture Decision Records.

### Domain Invariant

A rule that must remain true for a domain object or aggregate.

Examples include:

- Stored value cannot be reduced twice for one idempotent transaction.
- A revoked credential cannot return to Active status without an explicit
  replacement or reissuance workflow.
- A disabled reader cannot submit accepted transactions.
- A fare decision records the policy version used.

### Domain Event

A fact describing something meaningful that occurred in the domain.

Examples may include:

- CredentialIssued
- CredentialRevoked
- ReaderRegistered
- ReaderDisabled
- FareTransactionApproved
- FareTransactionRejected
- FarePolicyActivated
- ReconciliationDiscrepancyDetected

### Application Service

A component that coordinates one or more domain operations to complete a use
case.

Application services:

- Load required domain state
- Invoke domain behavior
- Coordinate persistence
- Publish appropriate events
- Return use-case results

### Repository

An abstraction for retrieving and persisting domain aggregates.

A repository interface belongs near the application or domain boundary.

A PostgreSQL repository implementation belongs in the persistence crate.

### Port

An interface representing a capability required by the application or domain.

Examples include:

- Account repository
- Credential repository
- Clock
- Key provider
- Audit sink
- Transaction manager

### Adapter

A concrete implementation connecting a port to infrastructure.

Examples include:

- PostgreSQL repository
- System clock
- Development key provider
- HTTP handler
- Telemetry exporter

### Infrastructure

Technical components supporting the application without defining the central
business rules.

Infrastructure includes:

- PostgreSQL
- HTTP server
- Configuration loader
- Logging provider
- Metrics exporter
- Key-management adapter

## 17. Persistence concepts

### System of Record

The authoritative source used to determine current durable system state.

PostgreSQL is TransitGuard's initial system of record.

Reader offline queues are authoritative for unsynchronized reader-local
transactions until backend acknowledgement is durably recorded.

### Database Record

A persistence representation stored in a database.

A database record is not automatically a domain entity.

Translation between persistence records and domain types must remain explicit.

### Database Transaction

A group of database operations committed or rolled back as one atomic unit.

Database transactions are required when partial persistence would violate a
domain invariant.

### Schema Migration

A versioned change to database structure or data representation.

Preferred project location will be defined when persistence implementation
begins.

### Optimistic Concurrency

A strategy that detects conflicting updates through versions or comparable
state checks.

### Ledger Entry

A durable record representing a simulated value movement or adjustment.

Preferred Rust name:

```text
LedgerEntry
```

Ledger entries must not be described as real financial settlement records.

## 18. Error concepts

### Domain Error

An error produced because a requested domain operation violates a business
rule or invariant.

Examples include:

- Credential Already Revoked
- Reader Disabled
- Product Expired
- Insufficient Stored Value

### Validation Error

An error produced because input is structurally or semantically invalid.

### Authentication Error

An error produced because actor identity cannot be established.

### Authorization Error

An error produced because an authenticated actor lacks permission.

### Conflict Error

An error produced because the requested operation conflicts with current
durable state.

### Idempotency Conflict

An error produced when an idempotency key is reused with an incompatible
request.

### Dependency Error

An error produced because an infrastructure dependency is unavailable or
failed.

### Internal Error

An unexpected system failure that cannot be safely represented through a more
specific public category.

Internal errors must not expose sensitive implementation details.

### Retryable Error

An error for which another attempt may succeed without changing the request.

### Permanent Error

An error that should not be retried without correcting input, permissions,
configuration, or system state.

## 19. Terms to avoid

The following terms should not be used inaccurately.

### Payment Card

Do not call a TransitGuard fare credential a payment card.

TransitGuard does not process actual payment-card transactions.

### Bank Account

Do not call stored value a bank account or bank balance.

### Certified Equipment

Do not describe the reader simulator as certified equipment.

### Production Key

Do not describe a local development key as a production key.

### Real-Time

Use `real-time` only when an explicit latency requirement supports the claim.

Prefer:

- Near-real-time
- Synchronous
- Asynchronous
- Periodic
- Event-driven

### Guaranteed

Do not use `guaranteed` unless the relevant invariant, test, or formal property
actually provides that guarantee.

### Secure

Do not describe a component as secure without identifying the implemented
controls and remaining assumptions.

Prefer precise statements such as:

- Signatures are verified before acceptance.
- Secrets are redacted from structured logs.
- Administrative actions require authorization.
- Duplicate transaction effects are prevented through idempotency records.

### Production Ready

Do not describe TransitGuard as production ready merely because it follows
production-oriented practices.

Preferred description:

```text
Production-oriented portfolio system
```

### Compliant

Do not claim regulatory, payment, transportation, accessibility, privacy, or
security compliance unless a formal assessment has established it.

## 20. Rust naming conventions

Core domain types should use singular PascalCase names.

Examples:

```text
TransitAccount
FareCredential
ReaderEquipment
FarePolicy
FareTransaction
ReconciliationResult
```

Identifiers should use the domain concept followed by `Id`.

Examples:

```text
TransitAccountId
FareCredentialId
ReaderId
FareTransactionId
DiscrepancyId
```

Collections should use plural variable names.

Examples:

```text
transactions
credentials
readers
discrepancies
```

Boolean names should describe a true condition.

Examples:

```text
is_active
is_revoked
is_offline
has_valid_product
requires_manual_review
```

Time values should identify their meaning.

Examples:

```text
created_at
updated_at
event_time
received_at
processed_at
expires_at
revoked_at
```

Version values should identify what is versioned.

Examples:

```text
fare_policy_version
revocation_version
protocol_version
reader_software_version
```

Avoid generic names such as:

```text
data
info
item
object
manager
helper
thing
record
```

Use the specific domain concept whenever possible.

## 21. API naming conventions

API resources should use stable domain terminology.

Preferred examples:

```text
/transit-accounts
/fare-credentials
/readers
/fare-policies
/fare-transactions
/reconciliation-runs
/audit-events
```

API request and response fields should use consistent names across endpoints.

Examples:

```text
transit_account_id
fare_credential_id
reader_id
fare_transaction_id
fare_policy_version
correlation_id
```

Public APIs must not expose internal database primary-key terminology when a
domain identifier is available.

## 22. Database naming conventions

Database tables should use plural snake_case names.

Potential examples:

```text
transit_accounts
fare_credentials
reader_equipment
fare_policies
fare_transactions
reconciliation_runs
reconciliation_discrepancies
audit_events
idempotency_records
```

Database columns should use snake_case.

Domain identifiers should retain their domain meaning.

Avoid naming unrelated identifiers only:

```text
id
```

when explicit naming improves joins, migrations, and operational investigation.

Preferred examples include:

```text
transit_account_id
fare_credential_id
reader_id
fare_transaction_id
```

## 23. Glossary governance

This glossary is part of the Phase 0 architecture baseline.

A terminology change should be made when:

- A concept gains a clearer domain name
- An existing term is ambiguous
- Implementation reveals two concepts previously treated as one
- A new bounded context introduces a distinct meaning
- Security or operational clarity requires a more precise name

A terminology change that affects public APIs, persisted data, protocol
messages, or major architectural boundaries should be documented through an
Architecture Decision Record.

New code should follow the current glossary unless an intentional,
documented terminology change is being introduced.
