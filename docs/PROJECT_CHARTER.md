# TransitGuard Project Charter

## Purpose

TransitGuard is a production-oriented Rust portfolio project that simulates a
secure transit fare-processing and reconciliation platform.

The project demonstrates domain-driven design, distributed processing,
credential management, offline operation, observability, resilience, and
production software engineering practices.

## Core capabilities

TransitGuard will support:

- Fictional transit-card issuance
- Project-owned credentials and cryptographic keys
- Registered reader and equipment identities
- Stored balances and transit passes
- Transfers and fare caps
- Lost-card handling and credential revocation
- Offline reader operation
- Delayed transaction synchronization
- Idempotent transaction processing
- Duplicate detection
- Reconciliation and discrepancy reporting
- Audit logging
- Administrative APIs
- A fictional administration website
- A passenger-facing mobile application
- Metrics, structured logs, traces, and health checks

## Initial architecture

TransitGuard is organized as a modular Cargo workspace.

### Applications

- `transitguard-api`
- `transitguard-worker`
- `transitguard-reader-simulator`

### Libraries

- `transitguard-domain`
- `transitguard-application`
- `transitguard-fare-engine`
- `transitguard-device-protocol`
- `transitguard-security`
- `transitguard-reconciliation`
- `transitguard-persistence`
- `transitguard-telemetry`
- `transitguard-config`

## Safety boundary

TransitGuard is fictional, isolated, and project-owned.

It will not:

- Connect to an actual transit authority
- Read or alter real transit cards
- reproduce a real fare-card protocol
- Use real transportation credentials or cryptographic keys
- Interact with production fareboxes or transportation equipment
- Bypass transportation, payment, or device security controls

All cards, readers, protocols, identities, credentials, transactions, and
infrastructure are simulated specifically for this project.

## Engineering standard

The repository will contain:

- Automated tests
- Continuous integration
- Architecture documentation
- Architecture decision records
- Threat modeling
- Operational documentation
- Structured observability
- Versioned releases
- Honest descriptions of implemented and simulated capabilities
