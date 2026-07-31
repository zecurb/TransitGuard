# ADR 0001: Use a Modular Rust Workspace

- Status: Accepted
- Date: 2026-07-30
- Decision owners: TransitGuard maintainers
- Related documents:
  - `docs/architecture/SYSTEM_ARCHITECTURE.md`
  - `docs/architecture/DOMAIN_GLOSSARY.md`
  - `docs/security/THREAT_MODEL.md`

## Context

TransitGuard must model a production-oriented transit fare-processing platform
with several distinct concerns:

- Core transit domain rules
- Fare calculation
- Application use cases
- Reader-device communication
- Credential and key-management abstractions
- PostgreSQL persistence
- Transaction reconciliation
- Configuration
- Telemetry
- HTTP APIs
- Background jobs
- Offline reader behavior

These concerns require explicit boundaries, independent testing, and controlled
dependency direction.

TransitGuard could begin as either:

1. One unstructured Rust application
2. A modular Rust workspace with multiple crates and deployable applications
3. A collection of independently deployed network services

An unstructured application would make it difficult to prevent database,
network, and framework concerns from leaking into core business logic.

A large microservice architecture would introduce deployment, networking,
authentication, observability, failure-handling, and data-consistency
complexity before the domain behavior is stable.

The initial engineering objective is to establish strong internal boundaries
without introducing unnecessary distributed-system complexity.

## Decision

TransitGuard will begin as a modular Rust Cargo workspace.

The workspace contains deployable applications under:

```text
apps/
apps/api
apps/worker
apps/reader-simulator

crates/
crates/domain
crates/application
crates/fare-engine
crates/device-protocol
crates/security
crates/reconciliation
crates/persistence
crates/telemetry
crates/config

```

The architecture will enforce boundaries through:

- Separate Cargo packages
- Explicit dependency declarations
- Restricted dependency direction
- Public APIs between crates
- Unit and integration tests
- Architecture documentation
- Architecture Decision Records
- Continuous-integration checks

The domain and fare-engine crates must remain independent from:

- HTTP frameworks
- PostgreSQL clients
- Database row types
- Filesystem configuration
- Cloud SDKs
- Concrete logging providers
- Web and mobile client types

Deployable applications may coordinate infrastructure and application
components, but they must not own duplicate versions of domain rules.

TransitGuard will not initially split every crate into an independently
deployed service.

## Dependency direction

The intended dependency direction is:

```text
Deployable applications
        |
        v
Application layer
        |
        +----------------------+
        |                      |
        v                      v
Domain and fare engine    Security abstractions
        |
        v
Infrastructure interfaces
        |
        v
Persistence, telemetry, and concrete providers
```

The domain crate must not depend on infrastructure crates.

Infrastructure crates may implement interfaces defined by the application
layer.

Circular crate dependencies are not permitted.

## Service extraction criteria

A component may later become an independently deployed service when at least
one concrete requirement justifies the separation.

Acceptable justifications include:

- Independent scaling requirements
- Stronger security isolation
- Independent availability objectives
- Independent deployment cadence
- Distinct data ownership
- Distinct team ownership
- Resource-intensive workload isolation
- Fault-containment requirements
- Regulatory or operational isolation requirements

The following are not sufficient by themselves:

- The crate contains many source files
- Microservices appear more advanced
- The project needs more deployable components for presentation
- A framework makes service creation easy
- Another company uses microservices

Any service extraction must receive a separate Architecture Decision Record.

## Consequences

### Positive consequences

The modular workspace provides:

- Strong compile-time boundaries
- Shared Rust types where appropriate
- One reproducible development environment
- One dependency lock file
- Workspace-wide formatting and linting
- Workspace-wide testing
- Fast local development
- Clear ownership of domain behavior
- Easier refactoring while the domain evolves
- A controlled path toward service extraction

### Negative consequences

The initial system may have:

- Shared release cycles for several components
- Shared workspace dependency resolution
- Less deployment independence than a microservice architecture
- The possibility of accidental coupling through shared crates
- A need for deliberate enforcement of internal boundaries

### Operational consequences

The first local deployment will contain multiple processes, but not a large
network of independently managed services.

Initial deployable processes are:

- TransitGuard API
- TransitGuard Worker
- One or more TransitGuard Reader Simulators

PostgreSQL will initially serve as the central durable system of record.

### Testing consequences

Core business rules can be tested without:

- Starting PostgreSQL
- Starting an HTTP server
- Starting the background worker
- Starting a reader simulator
- Loading external configuration
- Connecting to external infrastructure

Integration tests will verify interactions between application,
infrastructure, and persistence components.

### Security consequences

The modular design creates explicit locations for:

- Authentication and authorization boundaries
- Credential operations
- Equipment identity verification
- Secret redaction
- Audit handling
- Protocol validation
- Persistence controls

The architecture does not automatically make these components secure.

Each security control must still be implemented and tested.

## Alternatives considered

### Alternative 1: One Rust package

A single package would reduce initial setup work.

It was rejected because it would provide weak enforcement of architectural
boundaries and would make infrastructure leakage more likely.

### Alternative 2: Immediate microservices

Each major component could begin as an independent network service.

This was rejected because it would introduce premature complexity involving:

- Service discovery
- Inter-service authentication
- Network failure handling
- Distributed tracing
- Independent deployments
- Data ownership
- Cross-service consistency
- Versioned service contracts

Those concerns should be introduced only when justified by an actual system
requirement.

### Alternative 3: One deployable application with internal modules

TransitGuard could use one Cargo package containing multiple Rust modules.

This would be simpler than a workspace while still offering some organization.

It was rejected because separate crates provide stronger dependency and public
API boundaries for the core domain, fare engine, security, and persistence
layers.

## Enforcement

This decision will be enforced through:

- Cargo workspace membership
- Explicit crate dependencies
- Code review
- Continuous integration
- Clippy and formatting checks
- Workspace tests
- Architecture documentation
- Additional ADRs for boundary changes

A change that introduces an infrastructure dependency into the domain crate
must be rejected or documented through a superseding Architecture Decision
Record.

## Review conditions

This decision should be reviewed when:

- A component requires independent scaling
- A component requires stronger security isolation
- Separate teams own different bounded contexts
- Independent deployment becomes operationally necessary
- The shared workspace creates unacceptable build or release coupling
- Data ownership requires separate durable systems
- A new deployment model is proposed

## Current outcome

TransitGuard will continue as a modular Rust Cargo workspace.

Strong internal boundaries will be established before additional distributed
service boundaries are introduced.
