# TransitGuard Initial Threat Model

## 1. Purpose

This document identifies the initial security threats, trust boundaries,
protected assets, assumptions, and security controls for TransitGuard.

TransitGuard is a fictional, project-owned transit fare-processing platform.
It does not connect to real transit authorities, real fare equipment, real
payment systems, or real transit credentials.

This threat model is an engineering baseline. It will evolve as the system
gains implemented APIs, persistence, reader synchronization, cryptographic
operations, web interfaces, and mobile functionality.

## 2. Security objectives

TransitGuard must protect:

- Fare-transaction integrity
- Transit-account state
- Stored-value state
- Credential lifecycle state
- Reader-equipment identity
- Fare-policy integrity
- Revocation information
- Administrative operations
- Reconciliation evidence
- Audit-record integrity
- Secrets and private key material
- System availability
- Operational telemetry

The primary security objectives are:

### Authentication

TransitGuard must determine which passenger, administrator, reader, worker, or
internal service is making a request.

### Authorization

TransitGuard must verify that an authenticated actor is permitted to perform
the requested action on the requested resource.

### Integrity

TransitGuard must detect unauthorized modification of credentials,
transactions, policies, synchronization batches, acknowledgements, and audit
records.

### Confidentiality

TransitGuard must prevent disclosure of secrets, private keys, authentication
tokens, protected credential material, database credentials, and sensitive
account information.

### Availability

TransitGuard must remain usable during expected dependency failures and must
degrade safely when full service is unavailable.

### Accountability

Security-relevant and business-significant actions must be attributable to an
identified actor through protected audit records.

### Replay resistance

Previously accepted messages must not produce duplicate or unauthorized
business effects when retransmitted.

### Non-repudiation within the simulation

TransitGuard should preserve enough signed or authenticated evidence to
determine which project-owned reader submitted a transaction.

This is a project-level assurance objective and not a legal claim of
non-repudiation.

## 3. Scope

This threat model covers:

- TransitGuard API
- TransitGuard Worker
- TransitGuard Reader Simulator
- PostgreSQL persistence
- Administration web application
- Passenger mobile application
- Project-owned fare credentials
- Reader synchronization protocol
- Development key providers
- Fare-policy distribution
- Revocation distribution
- Reconciliation workflows
- Audit records
- Logs, metrics, and traces
- Local and continuous-integration environments

This threat model does not cover:

- Real transit authority infrastructure
- Real fare cards
- Real buses or fareboxes
- Real payment-card processing
- Banking systems
- Certified hardware security modules
- Real mobile-wallet credentials
- Proprietary transit protocols
- Production transportation networks

## 4. System actors

### Passenger

A human using a simulated TransitGuard account and fare credential.

A passenger is trusted only to access operations permitted for their own
account and credentials.

### Administrator

A human authorized to perform selected administrative operations.

Administrator authentication does not automatically grant permission for every
administrative action.

### Operator

A human monitoring reader and platform operations.

Operators may have read-only or narrowly scoped operational permissions.

### Reader Equipment

A project-owned simulated fare reader.

A reader is authenticated through a distinct equipment identity.

A reader must not be trusted merely because it uses the reader protocol.

### Background Worker

An internal process that executes asynchronous jobs and reconciliation work.

The worker receives only the permissions required for its assigned workloads.

### Development Operator

A developer running TransitGuard in a local or test environment.

Development access must not be treated as equivalent to production
administrative authority.

### External Attacker

An unauthenticated or improperly authenticated party attempting to access,
modify, disrupt, replay, inspect, or misuse TransitGuard.

### Compromised Actor

A legitimate passenger, administrator, operator, reader, or internal process
whose credentials or execution environment have been compromised.

TransitGuard must not assume that authenticated actors are always benign.

## 5. Protected assets

### Private signing keys

Private keys used to sign project-owned credentials, device messages, or
acknowledgements.

Required protections:

- Never committed to Git
- Never written to logs
- Never returned through public APIs
- Never embedded in web or mobile client source
- Versioned and rotatable
- Loaded through a controlled key-provider interface

### Authentication credentials

Credentials used by passengers, administrators, readers, workers, or internal
services.

Required protections:

- Stored using appropriate one-way hashing or protected secret storage
- Redacted from logs and errors
- Scoped to the actor type
- Revocable
- Expirable where appropriate

### Fare credentials

Project-owned card or mobile credentials used during simulated transit taps.

Required protections:

- Unique identifiers
- Lifecycle status
- Signature or authentication verification
- Expiration handling
- Revocation handling
- Replacement handling
- Protection against cloning inside the simulation

### Equipment identities

Credentials and identifiers proving which registered reader submitted a
request.

Required protections:

- Unique reader identity
- Separate private credential per reader
- Disablement and revocation
- Rotation
- Replay protection
- Protocol-version validation

### Transit-account state

Account balances, products, credentials, eligibility classifications, and
journey history.

Required protections:

- Authorized access only
- Transactional updates
- Idempotent mutations
- Auditability for administrative changes
- Concurrency protection

### Fare transactions

Records describing approved, rejected, offline, and reconciled fare activity.

Required protections:

- Stable transaction identifiers
- Reader attribution
- Policy-version attribution
- Duplicate detection
- Integrity validation
- Durable persistence
- Reconciliation evidence

### Fare policies

Versioned rules used to calculate fares.

Required protections:

- Authorized creation and activation
- Immutable policy versions
- Audit records
- Integrity validation
- Controlled reader distribution
- Rollback through a new explicit version

### Revocation information

Versioned information identifying credentials that must no longer be accepted.

Required protections:

- Authorized modification
- Integrity validation
- Version ordering
- Reader synchronization tracking
- Staleness limits

### Audit records

Records of administrative, security-relevant, and significant business
operations.

Required protections:

- Append-oriented persistence
- Actor attribution
- Timestamping
- Correlation identifiers
- Tamper detection where practical
- Restricted deletion
- Sensitive-data redaction

### Reconciliation evidence

Records used to explain missing, duplicate, conflicting, late, or invalid
transactions.

Required protections:

- Durable persistence
- Stable identifiers
- Source references
- Resolution history
- Restricted modification
- Audit trails for manual resolution

## 6. Trust boundaries

### Boundary 1: Passenger application to API

Data crossing this boundary includes:

- Authentication credentials
- Account requests
- Credential-management requests
- Product-purchase requests
- Simulated funding requests
- Journey-history requests

Required controls:

- Transport protection
- Authentication
- Account-level authorization
- Input validation
- Request-size limits
- Rate limiting
- Idempotency for protected writes
- Secret-safe error responses

### Boundary 2: Administration application to API

Data crossing this boundary includes:

- Administrative authentication
- Credential issuance requests
- Credential revocation requests
- Reader-registration requests
- Fare-policy operations
- Reconciliation-resolution operations

Required controls:

- Strong administrator authentication
- Role- or permission-based authorization
- Protection against cross-site request forgery where relevant
- Input validation
- Audit events
- Rate limiting
- Session protection
- Reauthentication for high-impact actions where appropriate

### Boundary 3: Reader simulator to API

Data crossing this boundary includes:

- Equipment identity
- Tap transactions
- Synchronization batches
- Local sequence numbers
- Software version
- Fare-policy version
- Revocation version
- Device-health information

Required controls:

- Equipment authentication
- Message-integrity validation
- Protocol-version validation
- Payload-size limits
- Batch-size limits
- Sequence validation
- Replay detection
- Idempotency
- Reader-status validation
- Reader disablement enforcement

### Boundary 4: API and worker to PostgreSQL

Data crossing this boundary includes:

- Queries
- Transaction updates
- Account state
- Credentials
- Reader state
- Fare transactions
- Audit events
- Job records
- Reconciliation results

Required controls:

- Database authentication
- Least-privilege database roles
- Parameterized queries
- Transaction boundaries
- Connection encryption where deployed across a network
- Migration controls
- Backup protection
- Error redaction

### Boundary 5: Application process to key provider

Data crossing this boundary includes:

- Signing requests
- Verification requests
- Key identifiers
- Key versions
- Development key material

Required controls:

- Narrow provider interface
- Authorized key use
- Key-version validation
- No unrestricted key export
- Secret-memory minimization
- Error redaction
- Auditability for key-management operations

### Boundary 6: Online backend to offline reader

The reader may continue processing transactions using cached state after losing
backend connectivity.

Required controls:

- Maximum offline duration
- Maximum cached-policy age
- Maximum cached-revocation age
- Maximum queued transaction count
- Maximum transaction age
- Maximum simulated fare exposure
- Durable local queue
- Ordered sequence numbers
- Integrity-protected synchronization
- Explicit reconciliation

### Boundary 7: Continuous integration to repository code

Repository-controlled code executes inside continuous-integration jobs.

Required controls:

- Minimal workflow permissions
- Pinned or reviewed third-party actions
- No production secrets in ordinary pull-request workflows
- Dependency review
- Secret scanning
- Restricted workflow changes
- Protected main branch
- Required status checks

### Boundary 8: Development environment to production-like environment

Development keys, passwords, data, and convenience settings must not be reused
as production-like credentials.

Required controls:

- Explicit environment identifiers
- Separate secret sources
- Development-key labeling
- Startup validation
- No committed secrets
- Environment-specific configuration
- Disabled unsafe development features outside development

## 7. Threat-model methodology

TransitGuard uses STRIDE categories as an initial threat-classification method:

- Spoofing
- Tampering
- Repudiation
- Information Disclosure
- Denial of Service
- Elevation of Privilege

Each identified threat includes:

- Threat identifier
- Category
- Affected assets
- Attack scenario
- Required controls
- Initial priority

Priority levels are:

- Critical
- High
- Medium
- Low

Priority describes required engineering attention. It does not represent a
formal quantified risk assessment.

## 8. Spoofing threats

### TG-S-001: Reader identity spoofing

Category:

- Spoofing

Scenario:

An attacker submits fare transactions while claiming to be a registered
reader.

Affected assets:

- Equipment identity
- Fare transactions
- Account state
- Reconciliation evidence

Required controls:

- Unique reader credentials
- Cryptographic equipment authentication
- Reader-status validation
- Credential rotation
- Reader disablement
- Failed-authentication monitoring

Priority:

- High

### TG-S-002: Administrator identity spoofing

Category:

- Spoofing

Scenario:

An attacker obtains or forges administrator authentication and performs
credential, policy, reader, or reconciliation operations.

Affected assets:

- Fare policies
- Credentials
- Reader registrations
- Account state
- Reconciliation results

Required controls:

- Strong authentication
- Secure session handling
- Multi-factor authentication in production-like deployments
- Rate limiting
- Login monitoring
- Reauthentication for sensitive actions
- Audit events

Priority:

- Critical

### TG-S-003: Passenger identity spoofing

Category:

- Spoofing

Scenario:

An attacker accesses another passenger's account, balance, credentials, or
journey history.

Affected assets:

- Transit-account state
- Fare credentials
- Journey history
- Personal information

Required controls:

- Secure authentication
- Account-level authorization
- Session expiration
- Credential revocation
- Login-rate limiting
- Sensitive-action confirmation

Priority:

- High

### TG-S-004: Internal worker spoofing

Category:

- Spoofing

Scenario:

An attacker submits internal job requests while claiming to be the background
worker.

Affected assets:

- Reconciliation results
- Background jobs
- Account state
- Audit records

Required controls:

- Separate internal-service identity
- Least-privilege authorization
- Authenticated job transport
- Job-type validation
- Network and process isolation where applicable

Priority:

- High

## 9. Tampering threats

### TG-T-001: Fare-transaction modification

Category:

- Tampering

Scenario:

A transaction is modified between creation by a reader and processing by the
backend.

Affected assets:

- Fare amount
- Credential reference
- Reader identifier
- Event time
- Fare-policy version

Required controls:

- Message-integrity protection
- Equipment authentication
- Canonical serialization
- Signature or message-authentication verification
- Reconciliation
- Immutable transaction identifiers

Priority:

- Critical

### TG-T-002: Offline queue modification

Category:

- Tampering

Scenario:

Queued reader transactions are altered, reordered, inserted, or deleted before
synchronization.

Affected assets:

- Offline transactions
- Local sequence ordering
- Account state
- Reconciliation evidence

Required controls:

- Durable queue
- Integrity metadata
- Sequence numbers
- Transaction identifiers
- Restricted local file permissions
- Queue validation during startup
- Reconciliation

Priority:

- High

### TG-T-003: Fare-policy modification

Category:

- Tampering

Scenario:

An attacker changes fare rules or causes readers to use an unauthorized fare
policy.

Affected assets:

- Fare policy
- Fare decisions
- Account balances
- Reader operation

Required controls:

- Immutable policy versions
- Authorized activation
- Policy signing or integrity protection
- Audit records
- Reader policy-version reporting
- Rollback through a new version

Priority:

- Critical

### TG-T-004: Revocation-list modification

Category:

- Tampering

Scenario:

An attacker removes revoked credentials from a distributed revocation list or
adds legitimate credentials without authorization.

Affected assets:

- Credential lifecycle
- Reader decisions
- Account access

Required controls:

- Authorized revocation changes
- Versioned revocation data
- Integrity protection
- Reader version reporting
- Audit events
- Backend validation during synchronization

Priority:

- Critical

### TG-T-005: Audit-record modification

Category:

- Tampering

Scenario:

An attacker changes or deletes audit evidence to hide unauthorized activity.

Affected assets:

- Audit records
- Accountability
- Incident evidence

Required controls:

- Append-oriented storage
- Restricted update and deletion permissions
- Database-role separation
- Tamper-evident chaining or signatures where practical
- Protected backups
- Audit-access monitoring

Priority:

- High

## 10. Repudiation threats

### TG-R-001: Administrator denies sensitive action

Category:

- Repudiation

Scenario:

An administrator denies issuing, revoking, replacing, or modifying a protected
resource.

Required controls:

- Authenticated actor identity
- Audit event
- Timestamp
- Correlation identifier
- Target-resource identifier
- Action result
- Reason or justification

Priority:

- High

### TG-R-002: Reader denies transaction submission

Category:

- Repudiation

Scenario:

A reader submits transactions but later denies originating them.

Required controls:

- Equipment authentication
- Signed or authenticated messages
- Batch identifiers
- Sequence numbers
- Durable acknowledgements
- Reconciliation evidence

Priority:

- Medium

### TG-R-003: Manual reconciliation action lacks attribution

Category:

- Repudiation

Scenario:

A discrepancy is resolved manually without recording who made the decision or
why.

Required controls:

- Authenticated operator identity
- Resolution audit event
- Required reason
- Before-and-after state
- Correlation identifier

Priority:

- High

## 11. Information-disclosure threats

### TG-I-001: Secret material appears in logs

Category:

- Information Disclosure

Scenario:

Private keys, tokens, passwords, or credential secrets are emitted through
logs, traces, errors, or panic output.

Required controls:

- Redaction wrappers
- Restricted debug implementations
- Structured logging policy
- Secret scanning
- Error review
- Tests verifying redaction
- Panic-output review

Priority:

- Critical

### TG-I-002: Passenger accesses another account

Category:

- Information Disclosure

Scenario:

A passenger changes an identifier in a request and retrieves another
passenger's balance, credentials, or journey history.

Required controls:

- Object-level authorization
- Server-derived account ownership
- Opaque identifiers
- Authorization tests
- Access monitoring

Priority:

- High

### TG-I-003: Administrative interface exposes excessive data

Category:

- Information Disclosure

Scenario:

An administrator or operator can retrieve more credential, account, or security
data than required for their role.

Required controls:

- Least-privilege views
- Field-level redaction
- Role-based authorization
- Audit access
- Pagination
- Export controls

Priority:

- High

### TG-I-004: Repository contains committed secrets

Category:

- Information Disclosure

Scenario:

Development or production-like credentials are committed to Git history.

Required controls:

- `.gitignore`
- Environment-variable templates without secret values
- Secret scanning
- Pre-commit scanning
- Credential rotation after exposure
- Development-key documentation

Priority:

- Critical

## 12. Denial-of-service threats

### TG-D-001: Oversized synchronization batch

Category:

- Denial of Service

Scenario:

A reader or attacker submits an extremely large batch that consumes excessive
memory, CPU, database time, or storage.

Required controls:

- Maximum request size
- Maximum batch size
- Streaming or bounded parsing
- Per-reader rate limits
- Timeouts
- Database transaction limits

Priority:

- High

### TG-D-002: Expensive reconciliation request

Category:

- Denial of Service

Scenario:

An authorized or compromised actor repeatedly launches reconciliation over an
unbounded data range.

Required controls:

- Bounded reconciliation windows
- Authorization
- Background-job queues
- Concurrency limits
- Query timeouts
- Resource metrics
- Administrative rate limits

Priority:

- High

### TG-D-003: Authentication endpoint abuse

Category:

- Denial of Service

Scenario:

An attacker sends repeated login attempts or expensive credential-verification
requests.

Required controls:

- Rate limiting
- Backoff
- Request-cost limits
- Monitoring
- Temporary lockout policies where appropriate
- Efficient password-hashing configuration

Priority:

- Medium

### TG-D-004: Reader offline queue exhaustion

Category:

- Denial of Service

Scenario:

A reader remains offline or receives excessive taps until local durable storage
is exhausted.

Required controls:

- Maximum queue depth
- Maximum queue-storage size
- Disk-space monitoring
- Controlled rejection mode
- Operator alerts
- Oldest-transaction-age monitoring

Priority:

- High

### TG-D-005: Database connection exhaustion

Category:

- Denial of Service

Scenario:

API or worker processes consume all available database connections.

Required controls:

- Bounded connection pools
- Query timeouts
- Request concurrency limits
- Worker concurrency limits
- Pool metrics
- Graceful overload behavior

Priority:

- High

## 13. Elevation-of-privilege threats

### TG-E-001: Passenger invokes administrator operation

Category:

- Elevation of Privilege

Scenario:

A passenger uses an administrative endpoint or manipulates client state to
perform credential issuance, policy activation, or reader registration.

Required controls:

- Server-side authorization
- Separate administrative scopes or roles
- Endpoint authorization tests
- No trust in client-hidden controls
- Audit events

Priority:

- Critical

### TG-E-002: Operator gains full administrative access

Category:

- Elevation of Privilege

Scenario:

A read-only operator exploits missing permission checks to modify protected
resources.

Required controls:

- Explicit permission model
- Deny-by-default authorization
- Role-separation tests
- Audit events
- Periodic permission review

Priority:

- High

### TG-E-003: Reader performs account-management operation

Category:

- Elevation of Privilege

Scenario:

A reader credential is accepted by passenger or administrative endpoints.

Required controls:

- Distinct credential types
- Audience validation
- Actor-type validation
- Endpoint-specific authentication
- Separate authorization policies

Priority:

- Critical

### TG-E-004: Worker performs unauthorized application action

Category:

- Elevation of Privilege

Scenario:

A worker identity gains broad API or database permissions beyond required job
processing.

Required controls:

- Least-privilege database role
- Restricted job interfaces
- Separate service credentials
- Job-type authorization
- Audit-sensitive mutations

Priority:

- High

## 14. Replay and duplicate-effect threats

### TG-RP-001: Fare transaction replay

Scenario:

A previously valid fare transaction is submitted again to charge an account
multiple times or create duplicate journey state.

Required controls:

- Stable transaction identifier
- Idempotency record
- Reader sequence number
- Durable duplicate detection
- Equivalent replay response
- Reconciliation

Priority:

- Critical

### TG-RP-002: Synchronization batch replay

Scenario:

A valid reader batch is resent after acknowledgement loss or intentionally
replayed.

Required controls:

- Stable batch identifier
- Per-transaction identifiers
- Reader sequence tracking
- Durable batch result
- Idempotent acknowledgement behavior

Priority:

- High

### TG-RP-003: Administrative request replay

Scenario:

A credential issuance, balance adjustment, or policy activation request is
repeated after a timeout.

Required controls:

- Idempotency key
- Actor and operation scope
- Request-fingerprint validation
- Durable result reference
- Idempotency-conflict handling

Priority:

- High

### TG-RP-004: Acknowledgement replay

Scenario:

An old acknowledgement is presented to a reader to cause premature deletion of
queued transactions.

Required controls:

- Batch-identifier binding
- Reader-identity binding
- Sequence-range binding
- Integrity verification
- Durable reader acknowledgement state

Priority:

- High

## 15. Offline-operation threats

### TG-O-001: Reader operates with stale revocation data

Scenario:

A reader continues accepting a credential that was revoked while the reader
was offline.

Required controls:

- Revocation-version tracking
- Maximum revocation age
- Maximum offline duration
- Reduced offline acceptance policy
- Backend reconciliation
- Operator visibility

Priority:

- High

### TG-O-002: Reader operates with stale fare policy

Scenario:

A reader applies outdated fare rules for an excessive period.

Required controls:

- Policy-version tracking
- Maximum policy age
- Policy effective times
- Reader status reporting
- Reconciliation
- Controlled offline shutdown mode

Priority:

- Medium

### TG-O-003: Reader clock manipulation

Scenario:

A manipulated or incorrect reader clock changes transfer, expiration, or policy
decisions.

Required controls:

- Clock-drift tracking
- Backend receipt time
- Allowed time skew
- Signed synchronization time
- Reconciliation
- Operator alerting

Priority:

- High

### TG-O-004: Sequence rollback after reader restart

Scenario:

A reader loses durable state and reuses old local sequence numbers.

Required controls:

- Durable sequence storage
- Atomic queue and sequence updates
- Startup consistency validation
- Backend last-acknowledged sequence
- Reader recovery procedure

Priority:

- High

## 16. Supply-chain and development threats

### TG-SC-001: Malicious or compromised dependency

Scenario:

A Rust, JavaScript, mobile, GitHub Action, or Nix dependency introduces
malicious behavior or a critical vulnerability.

Required controls:

- Lock files
- Dependency review
- Security advisories
- Minimal dependency selection
- Version pinning where appropriate
- Automated audit tooling
- Removal of unused dependencies

Priority:

- High

### TG-SC-002: Unauthorized workflow modification

Scenario:

A pull request modifies continuous-integration workflows to expose secrets or
run unsafe code.

Required controls:

- Protected main branch
- Pull-request review
- Minimal workflow permissions
- No production secrets in ordinary pull requests
- Workflow-diff inspection
- Restricted deployment environments

Priority:

- High

### TG-SC-003: Development key reused outside development

Scenario:

A known development key is accidentally used in a production-like deployment.

Required controls:

- Environment-specific providers
- Startup rejection of development keys outside development
- Visible development-key labels
- Separate configuration
- Key-identifier validation

Priority:

- Critical

## 17. Privacy threats

TransitGuard will use fictional test data whenever possible.

Potential privacy threats include:

- Excessive passenger data collection
- Overly detailed journey-history retention
- Sensitive data in logs
- Unrestricted administrative exports
- Unnecessary identifier exposure

Required controls include:

- Data minimization
- Purpose-limited fields
- Role-based access
- Retention policy
- Redaction
- Synthetic demonstration data
- Audit of sensitive access

TransitGuard must not store real payment-card information.

Demonstration data should not contain real private passenger information.

## 18. Security controls baseline

The initial security-control baseline includes:

- Protected `main` branch
- Pull-request workflow
- Continuous integration
- Rust formatting and linting
- Automated tests
- Secret-safe `.env.example`
- No committed private keys
- Explicit actor categories
- Explicit trust boundaries
- Idempotency requirements
- Reader sequence numbers
- Versioned fare policies
- Versioned revocation data
- Structured audit events
- Structured telemetry
- Sensitive-data redaction
- Bounded offline operation
- Bounded synchronization batches
- Bounded retries and timeouts

These controls are architectural requirements until implemented.

Documentation alone does not satisfy an implementation requirement.

## 19. Security testing requirements

TransitGuard security testing will progressively include:

- Unauthorized endpoint tests
- Cross-account access tests
- Disabled-reader tests
- Revoked-credential tests
- Expired-credential tests
- Invalid-signature tests
- Duplicate-transaction tests
- Replayed-batch tests
- Sequence-gap tests
- Oversized-request tests
- Batch-limit tests
- Stale-policy tests
- Stale-revocation tests
- Secret-redaction tests
- Idempotency-conflict tests
- Database-rollback tests
- Permission-boundary tests
- Dependency-failure tests

Security tests must verify observable behavior rather than only internal
function calls.

## 20. Security logging requirements

Security-relevant events include:

- Authentication failure
- Authorization failure
- Reader-authentication failure
- Invalid signature
- Replay detection
- Idempotency conflict
- Disabled-reader request
- Revoked-credential presentation
- Administrative credential issuance
- Administrative credential revocation
- Reader registration
- Reader disablement
- Fare-policy activation
- Development-key use outside development
- Reconciliation security discrepancy

Security logs must include:

- Event category
- Timestamp
- Actor category
- Non-sensitive actor identifier
- Correlation identifier
- Operation
- Outcome

Security logs must not include:

- Passwords
- Private keys
- Raw authentication tokens
- Full credential secrets
- Database credentials
- Session secrets

## 21. Residual risks

Some risks will remain after initial controls are implemented.

Expected residual risks include:

- A fully compromised authorized administrator
- A compromised reader with valid equipment credentials
- Previously unknown dependency vulnerabilities
- Delayed revocation during permitted offline operation
- Data inconsistency caused by severe storage corruption
- Denial of service beyond configured capacity
- Development-environment misuse
- Human error during manual reconciliation

Residual risks must be documented rather than hidden.

High-impact residual risks should have:

- Detection controls
- Response procedures
- Recovery procedures
- Audit evidence
- Operational ownership

## 22. Security assumptions

This initial threat model assumes:

- The operating system provides basic process and file isolation.
- PostgreSQL authentication is enabled.
- Transport security will be used for network communication.
- Development machines are not treated as trusted production key stores.
- GitHub repository permissions are controlled.
- Reader credentials are unique per reader.
- Production-like secrets are not placed in client applications.
- Database backups are access-controlled.
- System clocks are imperfect and require skew handling.
- Offline readers may be unavailable for extended periods.
- Authenticated actors may still be malicious or compromised.

Security design must not depend on every authenticated actor behaving
correctly.

## 23. Required follow-up decisions

Future Architecture Decision Records must define:

- Authentication mechanism for passengers
- Authentication mechanism for administrators
- Authentication mechanism for reader equipment
- Authorization model
- Credential-signing algorithm
- Message-integrity mechanism
- Key-provider interface
- Development-key handling
- Idempotency-storage design
- Offline queue storage
- Audit-record integrity strategy
- Fare-policy integrity mechanism
- Revocation-distribution format
- Database role separation
- Retention policy
- Security-event alerting

## 24. Review cadence

This threat model must be reviewed:

- Before implementing authentication
- Before implementing cryptographic signing
- Before implementing reader synchronization
- Before implementing administrative operations
- Before introducing external deployment
- After a significant architecture change
- After a security incident
- Before a version described as production-oriented release quality

Each review should update:

- Assets
- Trust boundaries
- Threats
- Implemented controls
- Missing controls
- Residual risks
- Follow-up decisions

## 25. Current security status

At Phase 0, TransitGuard has:

- Documented trust boundaries
- Documented protected assets
- Identified initial STRIDE threats
- Established security terminology
- Defined initial required controls
- Established a CI baseline
- Established a protected Git workflow

At Phase 0, TransitGuard does not yet have:

- Implemented authentication
- Implemented authorization
- Implemented credential signing
- Implemented equipment authentication
- Implemented PostgreSQL persistence
- Implemented audit storage
- Implemented synchronization security
- Implemented rate limiting
- Implemented secret-management integration
- Completed penetration testing
- Completed security certification

TransitGuard must not claim these controls are implemented until the
corresponding code and tests exist.
