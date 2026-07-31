# Contributing to TransitGuard

Thank you for contributing to TransitGuard.

TransitGuard is a production-oriented Rust portfolio project that simulates a
fictional transit fare-processing, credential, reader, synchronization, and
reconciliation platform.

All contributions must preserve the project's architecture, engineering
standards, security boundaries, and fictional safety scope.

## Project scope

TransitGuard may include:

- Fictional transit accounts
- Project-owned fare credentials
- Simulated transit cards
- Simulated mobile credentials
- Reader simulators
- Fare-policy processing
- Offline reader operation
- Transaction synchronization
- Credential revocation
- Transaction reconciliation
- Administrative web interfaces
- Passenger mobile interfaces
- Observability and operational tooling

TransitGuard must not:

- Connect to a real transit authority
- Read or modify real transit cards
- Reproduce proprietary transit protocols
- Use real transit-authority credentials
- Interact with production fareboxes
- Process real payment-card transactions
- Store real payment-card information
- Bypass transportation or payment security controls
- Claim equipment certification or regulatory compliance

All credentials, readers, keys, protocols, transactions, accounts, and
infrastructure must remain fictional and project-owned.

## Development environment

TransitGuard uses a Nix development shell.

Enter the environment from the repository root:

```bash
nix develop
```

Verify the required tools:

```bash
rustc --version
cargo --version
cargo clippy --version
rustfmt --version
psql --version
```

## Repository structure

```text
apps/
  api/
  worker/
  reader-simulator/

crates/
  application/
  config/
  device-protocol/
  domain/
  fare-engine/
  persistence/
  reconciliation/
  security/
  telemetry/

clients/
  mobile/
  web/

docs/
  adr/
  architecture/
  operations/
  security/
```

## Architectural boundaries

### Deployable applications

The `apps/` directory contains deployable Rust applications.

Application crates may coordinate use cases and infrastructure, but they must
not duplicate domain rules or fare-calculation logic.

### Domain crate

`crates/domain` contains:

- Domain entities
- Value objects
- Business invariants
- State transitions
- Domain events
- Domain errors

The domain crate must not depend on:

- HTTP frameworks
- PostgreSQL clients
- SQLite clients
- Database row types
- Filesystem configuration
- Cloud SDKs
- Concrete telemetry implementations

### Application crate

`crates/application` coordinates application use cases and defines required
ports, repositories, and transaction interfaces.

### Fare-engine crate

`crates/fare-engine` contains deterministic fare-calculation rules.

A fare decision must be reproducible from validated inputs and an identified
fare-policy version.

### Device-protocol crate

`crates/device-protocol` defines the fictional TransitGuard reader-to-backend
protocol.

It must not reproduce or claim compatibility with a real transit protocol.

### Security crate

`crates/security` contains project-owned abstractions for:

- Credential signing
- Signature verification
- Equipment authentication
- Key identifiers and versions
- Authorization
- Revocation
- Secret redaction

Private keys and secrets must never be committed to the repository.

### Persistence crate

`crates/persistence` contains concrete PostgreSQL persistence
implementations.

Database records must remain separate from domain types.

### Reconciliation crate

`crates/reconciliation` contains transaction matching, discrepancy
classification, evidence preservation, and resolution logic.

Discrepancies must never be silently discarded.

### Telemetry crate

`crates/telemetry` contains structured logging, tracing, metrics, health
reporting, and sensitive-data redaction.

### Configuration crate

`crates/config` loads and validates application and environment
configuration.

Required invalid or missing configuration must cause startup to fail safely.

## Branch workflow

Do not develop directly on `main`.

Create a branch from the latest `main`:

```bash
git switch main
git pull --ff-only origin main
git switch -c <branch-name>
```

Recommended branch names include:

```text
phase/1-domain-model
feat/fare-credential
feat/offline-reader-queue
fix/duplicate-transaction-processing
docs/threat-model
chore/dependency-policy
```

Each branch should contain one coherent workstream.

## Commit messages

Use this format:

```text
<type>: <description>
```

Common commit types are:

```text
feat
fix
docs
test
refactor
chore
build
ci
```

Good examples:

```text
feat: add fare credential lifecycle model
fix: prevent duplicate stored-value deductions
docs: document offline synchronization behavior
test: cover revoked credential rejection
refactor: separate fare policy validation
chore: update workspace dependency policy
```

Avoid vague messages such as:

```text
update files
changes
work
fix stuff
more code
```

## Required validation

Before committing Rust changes, run:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
nix flake check
```

A change is not ready for review while any required check is failing.

## Rust standards

TransitGuard uses Rust Edition 2024.

The workspace lint policy forbids or denies:

- Unsafe code
- `dbg!`
- `todo!`
- `unimplemented!`
- Unrestricted `unwrap`
- Unrestricted `expect`

Production code must use explicit error handling.

Preferred tools include:

- `Result`
- Domain-specific error enums
- `thiserror`
- Error translation at architectural boundaries

Panics must not be used for ordinary validation, dependency failure, or
business-rule failure.

## Testing standards

Meaningful features must include tests appropriate to their architectural
layer.

### Unit tests

Use unit tests for:

- Value-object validation
- Domain invariants
- State transitions
- Fare calculations
- Error classification
- Serialization behavior

### Integration tests

Use integration tests for:

- PostgreSQL repositories
- SQLite reader durability
- Database transactions
- API request flows
- Reader synchronization
- Credential revocation
- Reconciliation
- Idempotency

### Failure tests

Test failure behavior including:

- Duplicate requests
- Replayed batches
- Invalid signatures
- Disabled readers
- Revoked credentials
- Database failures
- Timeouts
- Partial synchronization failures
- Stale policies
- Stale revocation data

Tests must verify observable behavior and durable outcomes.

## Security requirements

Never commit:

- Private keys
- Authentication tokens
- Passwords
- Database credentials
- Session secrets
- Real passenger information
- Real payment information
- Real transit credentials

Use `.env.example` only for safe configuration examples.

Sensitive values must not appear in:

- Logs
- Metrics
- Traces
- Panic messages
- Test snapshots
- Screenshots
- Error responses
- Pull-request descriptions

Security-relevant changes should update:

```text
docs/security/THREAT_MODEL.md
```

Architectural security decisions should receive an Architecture Decision
Record.

## Documentation requirements

Update documentation when a change affects:

- Architecture
- Domain terminology
- Trust boundaries
- Security controls
- Public APIs
- Protocol formats
- Configuration
- Operational procedures
- Persistence schemas
- Failure behavior

Architecture decisions belong in:

```text
docs/adr/
```

Use the next available four-digit ADR number.

Example:

```text
0004-select-identifier-strategy.md
```

## Pull-request requirements

Every pull request should explain:

- What changed
- Why the change is needed
- Important implementation decisions
- Tests performed
- Security considerations
- Documentation changes
- Known limitations

Pull requests should remain small enough to review coherently.

Do not combine unrelated features, refactoring, dependency changes, and
documentation changes without a clear reason.

## Review checklist

Before requesting review, verify:

- The branch is based on the latest `main`.
- The working tree is clean.
- Formatting passes.
- Compilation passes.
- Clippy passes with warnings denied.
- All tests pass.
- Nix flake validation passes.
- No secrets were added.
- Domain boundaries remain intact.
- Documentation is current.
- Failure behavior is tested.
- Security implications were considered.

## Merge policy

TransitGuard uses pull requests for changes to `main`.

The preferred merge method is:

```text
Squash and merge
```

A pull request may be merged only after required continuous-integration checks
pass.

After a pull request is merged, update the local repository:

```bash
git switch main
git pull --ff-only origin main
git fetch --prune origin
```

Delete completed local branches after confirming the pull request was merged.

## Architecture changes

Create an Architecture Decision Record when a change introduces or
significantly changes:

- Service boundaries
- Persistence technology
- Protocol structure
- Authentication
- Authorization
- Cryptographic algorithms
- Key storage
- Identifier strategy
- Event delivery
- Offline durability
- Audit integrity
- Deployment model
- Major dependency direction

An accepted ADR may later be superseded, but it must not be silently rewritten
to conceal the original decision.

## Reporting problems

Use GitHub issues for:

- Bugs
- Feature proposals
- Documentation problems
- Architecture proposals
- Security-hardening work
- Operational improvements

Do not publish sensitive vulnerability details in a public issue.

Security reports must follow the process in:

```text
SECURITY.md
```

## Definition of done

A change is complete when:

- The implementation is finished.
- Tests cover the expected behavior.
- Failure behavior is defined.
- Security implications are addressed.
- Documentation is current.
- Required checks pass.
- The pull request is reviewed.
- The change is merged into `main`.
- The completed branch is removed.
