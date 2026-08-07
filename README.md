# TransitGuard

TransitGuard is a production-oriented Rust simulation of a secure transit
fare-processing and reconciliation platform.

The project models fictional transit cards, reader equipment, protected
credentials, offline fare transactions, synchronization, reconciliation,
administrative services, and operational monitoring.

> Current status: Phase 7 — backend synchronization transport and idempotent ingest

## Implemented capabilities

- Strongly typed fictional transit domain model
- Credential and reader-equipment lifecycle services
- PostgreSQL persistence adapters
- Deterministic online and offline fare evaluation
- Zone pricing, discounts, transfers, fare caps, and transit products
- Fictional reader simulator and project-owned device protocol
- Atomic reader-local sequence allocation and offline queue insertion
- Durable SQLite synchronization batches and stable retry identities
- Versioned HTTP/JSON reader synchronization protocol
- Bounded synchronization batches and payloads
- Transactional PostgreSQL synchronization ingest
- Deterministic request fingerprinting and idempotent backend replay
- Conflict detection for reused synchronization identities
- Validated, idempotent synchronization acknowledgements
- Atomic partial-outcome acknowledgement application
- Durable retry, permanent-failure, and manual-review records
- Lost-response and process-restart synchronization recovery
- API liveness and PostgreSQL readiness health endpoints
- Structured synchronization telemetry with sanitized failure categories
- Reader queue and synchronization health snapshots

## Planned capabilities

- Full financial reconciliation and discrepancy-resolution workflows
- Audit-event pipelines and expanded operational dashboards
- Administrative web application
- Passenger mobile application
- Production credential provisioning and deployment security

## Safety boundary

TransitGuard does not interact with real transit systems, real transit cards,
real fare equipment, real payment networks, or real transit-authority
credentials.

All protocols, cards, readers, keys, transactions, acknowledgements, endpoints,
and infrastructure are fictional and project-owned.

## Development documentation

- [Local PostgreSQL persistence validation](docs/development/postgresql-validation.md)
- [Deterministic fare engine](docs/development/fare-engine.md)
- [Reader simulator](docs/development/reader-simulator.md)
- [Reader-local offline storage and recovery](docs/development/reader-offline-storage.md)
- [Backend synchronization and recovery](docs/development/backend-synchronization.md)

## Architecture decisions

- [Use PostgreSQL as the system of record](docs/adr/0002-use-postgresql-as-system-of-record.md)
- [Use SQLite for reader offline storage](docs/adr/0003-use-sqlite-for-reader-offline-storage.md)
- [Use versioned HTTP/JSON for reader synchronization](docs/adr/0004-use-http-json-for-reader-synchronization.md)

## License

Apache License 2.0
