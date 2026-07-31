# TransitGuard Security Policy

TransitGuard is a production-oriented Rust portfolio project that simulates a
fictional transit fare-processing, credential, reader, synchronization, and
reconciliation platform.

Security reports are taken seriously. This document explains which versions
are supported, how to report a vulnerability privately, what information to
include, and what reporters can expect during remediation.

## Project safety boundary

TransitGuard operates entirely inside a fictional, project-owned environment.

TransitGuard does not:

- Connect to a real transit authority
- Read or modify real transit cards
- Reproduce proprietary transit-card protocols
- Use real transit-authority credentials
- Interact with production fareboxes
- Process actual payment-card transactions
- Store real payment-card information
- Claim transportation-equipment certification
- Claim payment, privacy, or regulatory compliance
- Bypass transportation or payment security controls

All cards, readers, equipment identities, credentials, cryptographic keys,
accounts, transactions, protocols, and infrastructure used by this project
must remain fictional and project-owned.

Do not test TransitGuard research against:

- Real transit systems
- Real transportation equipment
- Real passenger accounts
- Real payment systems
- Third-party infrastructure
- Systems you do not own or lack permission to test

## Supported versions

TransitGuard is currently under active development.

Security fixes are applied to:

- The current `main` branch
- The most recent published release, once releases begin

Older development branches, superseded prototypes, abandoned experiments, and
historical releases may not receive security updates.

| Version | Supported |
| --- | --- |
| Current `main` branch | Yes |
| Latest published release | Yes, once available |
| Older releases | No, unless explicitly documented |
| Unmerged feature branches | No |
| Forks maintained by other users | No |

## Reporting a vulnerability

Do not disclose suspected vulnerabilities through:

- Public GitHub issues
- Public pull requests
- Public discussions
- Commit messages
- Social media
- Screenshots containing sensitive details
- Public demonstrations before remediation

Use GitHub's private vulnerability-reporting capability for the TransitGuard
repository.

A private report should include:

- A concise title
- The affected component
- The affected branch, commit, or release
- Vulnerability category
- Technical description
- Reproduction steps
- Required configuration
- Expected behavior
- Actual behavior
- Potential impact
- Proof-of-concept material that remains inside the fictional environment
- Suggested mitigation, when available
- Whether the issue is already public
- Whether credentials or secrets may have been exposed

When private vulnerability reporting is unavailable, contact the repository
owner through the GitHub profile without including exploit details. Request a
private communication channel before sharing sensitive information.

## Appropriate report scope

Appropriate vulnerability reports may involve:

- Authentication bypass
- Authorization bypass
- Cross-account data access
- Reader-equipment identity spoofing
- Credential-signature verification failures
- Fare-policy integrity failures
- Revocation bypass
- Replay attacks
- Duplicate transaction effects
- Idempotency failures
- Offline queue data loss
- Synchronization acknowledgement forgery
- Sequence-number rollback
- Reconciliation evidence tampering
- Audit-record tampering
- Secret exposure
- Sensitive information in logs
- Unsafe cryptographic key handling
- SQL injection
- Command injection
- Path traversal
- Server-side request forgery
- Cross-site scripting
- Cross-site request forgery
- Dependency vulnerabilities with a demonstrated impact
- Denial-of-service conditions
- Privilege escalation
- Unsafe configuration defaults
- Insecure development-key use
- Database migration behavior that causes protected data loss

## Reports outside project scope

The following are outside the intended TransitGuard security-testing scope:

- Attacks against real transit authorities
- Testing real transit cards
- Testing production buses or fareboxes
- Extracting credentials from real transportation equipment
- Research involving unauthorized third-party systems
- Real payment-card testing
- Social engineering
- Physical intrusion
- Account takeover against unrelated services
- GitHub platform vulnerabilities
- NixOS platform vulnerabilities unrelated to TransitGuard
- Vulnerabilities in dependencies without a demonstrated TransitGuard impact
- Automated scanner output without validation
- Missing security headers on services that do not yet exist
- Claims based only on planned functionality
- Reports requiring access to real passenger information

General bugs that do not create a security impact should be reported through a
normal GitHub issue.

## Responsible testing requirements

Security testing must remain inside an environment owned or explicitly
authorized by the tester.

Use:

- Synthetic accounts
- Project-owned credentials
- Development-only cryptographic keys
- Local reader simulators
- Local PostgreSQL instances
- Local SQLite reader databases
- Fictional fare policies
- Fictional transaction records

Do not:

- Access data belonging to another person
- Degrade shared infrastructure
- Destroy evidence
- Exfiltrate secrets
- Retain exposed information longer than necessary
- Publish exploit details before remediation
- Use a vulnerability to access unrelated systems
- Introduce persistent access mechanisms
- Test against real transportation infrastructure

Stop testing after obtaining enough evidence to demonstrate the issue safely.

## Proof-of-concept requirements

A proof of concept should be:

- Minimal
- Reproducible
- Limited to the fictional TransitGuard environment
- Free from real credentials or personal information
- Designed to avoid destructive effects
- Accompanied by cleanup instructions when state is modified

Do not include:

- Real private keys
- Real authentication tokens
- Real passwords
- Real passenger data
- Real payment information
- Real transit credentials
- Third-party confidential information

Sensitive proof-of-concept files should be shared only through the private
reporting channel.

## Report severity information

TransitGuard uses the following initial severity categories.

### Critical

A critical vulnerability may allow:

- Unauthorized administrative control
- Exposure of private signing keys
- Widespread authentication bypass
- Widespread authorization bypass
- Arbitrary code execution in a backend process
- Undetected modification of fare-policy or revocation state
- Destructive compromise of authoritative transaction records

### High

A high-severity vulnerability may allow:

- Cross-account access
- Reader identity spoofing
- Credential revocation bypass
- Repeatable duplicate stored-value deductions
- Forged synchronization acknowledgements
- Significant audit or reconciliation tampering
- Exposure of authentication credentials
- Reliable denial of service affecting core processing

### Medium

A medium-severity vulnerability may allow:

- Limited unauthorized information disclosure
- Bounded denial of service
- Security-control bypass requiring substantial preconditions
- Incorrect offline processing with limited exposure
- Sensitive operational metadata disclosure
- Weaknesses that materially reduce defense in depth

### Low

A low-severity vulnerability may involve:

- Minor information exposure
- Security-hardening opportunities
- Misleading error behavior
- Limited configuration weaknesses
- Issues with minimal practical impact

Final severity depends on:

- Exploitability
- Required access
- Affected assets
- Scope
- Detectability
- Persistence
- Recovery complexity
- Existing mitigations
- Impact within the fictional environment

## Response targets

The following are response targets rather than contractual guarantees:

- Initial acknowledgement: within 3 business days
- Initial technical triage: within 10 business days
- Status update after validation: within 15 business days
- Remediation timeline: based on severity and implementation complexity

A report may require additional time when it involves:

- Multiple architectural boundaries
- Cryptographic design
- Database migrations
- Backward compatibility
- Offline reader recovery
- Protocol-version changes
- Coordinated dependency updates

The reporter will receive status updates through the private reporting channel
when practical.

## Triage process

A security report will be reviewed for:

1. Reproducibility
2. Affected component
3. Affected versions
4. Required permissions
5. Attack prerequisites
6. Confidentiality impact
7. Integrity impact
8. Availability impact
9. Auditability impact
10. Existing mitigations
11. Recommended remediation
12. Need for coordinated disclosure

The report may be classified as:

- Confirmed vulnerability
- Security-hardening improvement
- General software defect
- Duplicate report
- Previously known issue
- Not reproducible
- Outside project scope
- Expected documented behavior

## Remediation process

A confirmed vulnerability may require:

- A private remediation branch
- New unit tests
- New integration tests
- New security regression tests
- Updates to the threat model
- A new Architecture Decision Record
- Protocol-version changes
- Database migrations
- Credential or key rotation
- Revocation updates
- Configuration changes
- Dependency updates
- Operational recovery instructions
- A security advisory
- A patched release

Security fixes should include a regression test whenever practical.

A vulnerability is not considered remediated only because it has been
documented.

## Coordinated disclosure

Public disclosure should occur after:

- The vulnerability is validated
- A fix or mitigation is available
- Required key or credential rotation is complete
- Affected releases are identified
- Recovery guidance is prepared
- Users have had a reasonable opportunity to update

The public advisory may include:

- Vulnerability summary
- Affected versions
- Impact
- Severity
- Patched version
- Mitigation
- Reporter credit
- Timeline

Exploit details may be limited when publication would create unnecessary risk.

## Reporter recognition

Reporters may receive public credit when:

- The report is valid
- The reporter followed responsible-disclosure expectations
- The reporter requests recognition
- Public credit does not expose sensitive information

A reporter may remain anonymous.

TransitGuard does not currently operate a monetary bug-bounty program.

## Secret exposure procedure

A suspected exposed secret must be treated as compromised.

Required actions include:

1. Stop using the exposed secret.
2. Revoke or rotate it.
3. Remove it from active configuration.
4. Determine where it was exposed.
5. Search logs, artifacts, commits, and build output.
6. Assess unauthorized use.
7. Replace affected dependent credentials.
8. Document the incident.
9. Add a regression control.
10. Remove the secret from repository history when necessary.

Deleting a secret from the latest commit does not make the historical secret
safe.

Development keys must also be rotated when their exposure violates the
documented development environment assumptions.

## Security-sensitive data

The following values must not be committed or exposed:

- Private signing keys
- Database passwords
- Authentication tokens
- Session secrets
- Administrator credentials
- Reader private credentials
- Passenger passwords
- Raw credential secrets
- Real personal information
- Real payment-card information
- Real transit-authority credentials

Safe examples belong in:

```text
.env.example
```

Safe examples must use obviously fictional values.

## Security documentation

Security-relevant implementation work should review and update:

```text
docs/security/THREAT_MODEL.md
```

Architecture decisions involving security should be recorded under:

```text
docs/adr/
```

Examples include decisions about:

- Authentication
- Authorization
- Cryptographic algorithms
- Key providers
- Key rotation
- Equipment identity
- Protocol integrity
- Idempotency
- Replay protection
- Audit integrity
- Secret management
- Offline transaction durability

## Security validation

Security-sensitive changes should run the normal project checks:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
nix flake check
```

Additional validation may include:

- Authorization tests
- Cross-account access tests
- Invalid-signature tests
- Revoked-credential tests
- Disabled-reader tests
- Duplicate transaction tests
- Replay tests
- Sequence-gap tests
- Secret-redaction tests
- Database rollback tests
- Partial synchronization tests
- Dependency audits
- Secret scanning
- Migration tests
- Recovery tests

## Dependency vulnerabilities

A dependency advisory should be evaluated for:

- Whether TransitGuard uses the affected dependency
- Whether the vulnerable feature is enabled
- Whether the vulnerable code path is reachable
- Whether an existing mitigation applies
- Whether an update introduces compatibility risk
- Whether the lock file resolves to an affected version

Dependency updates must still pass the complete validation suite.

A scanner result without reachability or impact analysis may represent a
hardening task rather than a confirmed TransitGuard vulnerability.

## Current security maturity

TransitGuard currently has:

- A documented threat model
- Documented trust boundaries
- Documented protected assets
- A protected pull-request workflow
- Continuous integration
- A workspace lint baseline
- A secret-safe environment template
- Explicit architectural boundaries
- A fictional safety scope

TransitGuard does not yet claim:

- Production-ready authentication
- Production-ready authorization
- Certified cryptographic key storage
- Certified transportation-equipment security
- Payment-system compliance
- Privacy compliance
- Completed penetration testing
- Completed independent security assessment
- Regulatory certification

Security claims must reflect controls that are implemented and tested, not
controls that exist only in planning documents.

## Good-faith research

Good-faith research follows this policy, remains within authorized systems,
minimizes harm, protects sensitive information, and permits reasonable time for
remediation.

TransitGuard maintainers will evaluate reports based on technical merit,
scope, safety, and responsible handling.

Thank you for helping improve TransitGuard safely.
