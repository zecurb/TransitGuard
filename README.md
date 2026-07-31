# TransitGuard

TransitGuard is a production-oriented Rust simulation of a secure transit
fare-processing and reconciliation platform.

The project models fictional transit cards, reader equipment, protected
credentials, offline fare transactions, reconciliation, administrative
services, and operational monitoring.

> Current status: Phase 3 — PostgreSQL persistence

## Planned capabilities

- Secure fictional card and equipment identities
- Fare validation and calculation
- Balances, passes, transfers, and fare caps
- Offline reader processing
- Delayed synchronization
- Duplicate detection and idempotency
- Transaction reconciliation
- Audit logging and observability
- Administrative web application
- Passenger mobile application

## Safety boundary

TransitGuard does not interact with real transit systems, real transit cards,
real fare equipment, or real transit-authority credentials. All protocols,
cards, readers, keys, and infrastructure are fictional and project-owned.

## Development documentation

- [Local PostgreSQL persistence validation](docs/development/postgresql-validation.md)

## License

Apache License 2.0
