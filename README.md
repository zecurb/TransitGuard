# TransitGuard

TransitGuard is a production-oriented Rust simulation of a secure transit
fare-processing and reconciliation platform.

The project models fictional transit cards, reader equipment, protected
credentials, offline fare transactions, reconciliation, administrative
services, and operational monitoring.

> Current status: Phase 6 — durable reader-local SQLite offline processing

## Implemented capabilities

- Strongly typed fictional transit domain model
- Credential and reader-equipment lifecycle services
- PostgreSQL persistence adapters
- Deterministic online and offline fare evaluation
- Zone pricing, discounts, transfers, fare caps, and transit products
- Fictional reader simulator and project-owned device protocol
- Atomic reader-local sequence allocation and offline queue insertion
- Durable SQLite synchronization batches and stable retry identities
- Validated, idempotent synchronization acknowledgements
- Atomic partial-outcome application
- Durable retry, permanent-failure, and manual-review records
- Reader queue and synchronization health snapshots

## Planned capabilities

- Backend synchronization transport
- Reconciliation workflows
- Audit-event pipelines and operational dashboards
- Administrative web application
- Passenger mobile application

## Safety boundary

TransitGuard does not interact with real transit systems, real transit cards,
real fare equipment, or real transit-authority credentials. All protocols,
cards, readers, keys, and infrastructure are fictional and project-owned.

## Development documentation

- [Local PostgreSQL persistence validation](docs/development/postgresql-validation.md)
- [Deterministic fare engine](docs/development/fare-engine.md)
- [Reader simulator](docs/development/reader-simulator.md)
- [Reader-local offline storage and recovery](docs/development/reader-offline-storage.md)

## License

Apache License 2.0
