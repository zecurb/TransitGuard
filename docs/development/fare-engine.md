# TransitGuard Fare Engine

## Purpose

The TransitGuard fare engine is a deterministic Rust library shared by the
backend applications and the fictional reader simulator.

The engine calculates fare decisions without:

- querying a database;
- making network requests;
- reading process environment variables;
- accessing the system clock;
- using randomness;
- accessing mutable global state.

All fare policies, products, credentials, readers, zones, and transactions
used by TransitGuard are fictional and project-owned.

## Deterministic contract

A fare decision is determined entirely by:

- one validated `FarePolicy`;
- one complete `FareEvaluationInput`;
- an optional `OfflineEvaluationContext`.

Supplying identical values produces an identical result.

The event time is supplied explicitly by the caller. The fare engine never
determines the current time itself.

## Policy identity and versioning

Every fare policy contains:

- a stable `FarePolicyId`;
- an immutable `FarePolicyVersion`.

The policy identity identifies the policy family. The version identifies the
exact rule set used for a decision.

Decision evidence preserves both values so that reconciliation and operational
investigations can reproduce the original calculation.

## Fare-policy configuration

A validated `FarePolicy` contains:

- policy identity and version;
- currency;
- base fare;
- per-zone surcharge;
- transfer window;
- transfer discount;
- daily fare cap;
- weekly fare cap;
- rider eligibility discounts.

Policy validation enforces the following invariants:

- monetary fields use the policy currency;
- monetary fields are not negative;
- the daily cap is not lower than the base fare;
- the weekly cap is not lower than the daily cap;
- the transfer discount does not exceed the base fare;
- discount percentages do not exceed 100 percent;
- transfer windows are greater than zero.

## Complete evaluation input

`FareEvaluationInput` provides every changing value needed for evaluation:

- event time;
- origin zone;
- destination zone;
- rider eligibility;
- previous paid-fare history;
- accumulated daily charges;
- accumulated weekly charges;
- optional transit product;
- available stored-value balance.

The engine does not retrieve missing information from external infrastructure.

## Evaluation order

Online fare evaluation executes rules in this order:

1. Validate available stored value.
2. Calculate the base fare.
3. Calculate additional-zone surcharges.
4. Apply the rider eligibility discount.
5. Apply an eligible transfer benefit.
6. Apply the daily fare cap.
7. Apply the weekly fare cap.
8. Validate and apply a presented transit product.
9. Compare the remaining fare with available stored value.

This order is part of the fare-policy contract. Changing it may alter charged
amounts and requires explicit policy versioning and design review.

## Monetary calculations

TransitGuard uses `Money` and integer minor units for all monetary operations.

Floating-point arithmetic is not used for fare calculation.

Cross-currency operations return typed errors rather than silently converting
amounts.

Arithmetic overflow returns a typed evaluation error.

## Zone pricing

A same-zone journey receives no additional-zone surcharge.

For journeys across zones, the additional-zone count is the absolute
difference between the origin and destination zone identifiers.

The configured per-zone surcharge is multiplied by this count and added to the
base fare.

## Eligibility discounts

Eligibility discounts are represented in basis points:

- `0` basis points means no discount;
- `5,000` basis points means a 50 percent discount;
- `10,000` basis points means a full discount.

The percentage calculation uses integer minor units and rounds down to the
nearest minor unit.

Eligibility discounts are applied after zone pricing and before transfer
benefits.

## Transfer benefits

The caller supplies the previous paid-fare event time.

A transfer is eligible when:

- the previous paid fare did not occur after the current event;
- the elapsed duration is less than or equal to the configured transfer
  window.

The transfer-window boundary is inclusive.

The transfer discount cannot reduce the fare below zero.

## Daily and weekly caps

The caller supplies the amount already charged during the applicable service
day and service week.

The charge after cap processing is the smallest of:

- the fare after transfer processing;
- the remaining daily-cap allowance;
- the remaining weekly-cap allowance.

A fare cap can reduce a charge to zero but cannot create a negative charge.

Daily accumulated charges cannot exceed weekly accumulated charges.

## Transit products

A transit product contains:

- a stable product identity;
- an issued product-instance identity;
- an inclusive validity start;
- an inclusive validity end;
- all-zone coverage or an inclusive zone range.

A product must be valid at the supplied event time and cover both the origin
and destination zones.

A valid applicable product covers the remaining fare.

An expired, not-yet-valid, or geographically invalid product produces the
stable `ProductInvalid` rejection.

Product validity boundaries are inclusive.

## Stored-value outcomes

When no transit product covers the fare, the final fare is compared with the
available stored-value balance.

A balance equal to the fare is sufficient.

An insufficient balance produces
`FareRejectionReason::InsufficientStoredValue`.

The engine never produces a partial debit. It calculates a decision only.
Account mutation belongs to the application layer.

## Decision evidence

Every evaluation preserves evidence for the complete calculation:

- policy identity and version;
- base fare;
- additional-zone count;
- total zone surcharge;
- fare before eligibility discount;
- eligibility classification;
- discount basis points;
- eligibility discount;
- fare after eligibility;
- transfer eligibility;
- transfer discount;
- fare after transfer;
- fare-cap discount;
- fare after caps;
- daily-cap status;
- weekly-cap status;
- transit-product outcome;
- transit-product discount;
- final fare.

This evidence supports:

- transaction reconciliation;
- customer-support investigation;
- incident analysis;
- auditing;
- deterministic regression testing.

## Offline evaluation

Offline evaluation calls the same `evaluate_fare` implementation used for
online processing.

It does not maintain a second fare-calculation algorithm.

After normal fare calculation, offline evaluation checks:

1. cached fare-policy freshness;
2. cached credential-revocation freshness;
3. the provisional offline charge limit.

Freshness boundaries are inclusive. Cached data exactly as old as its
configured maximum remains valid.

A cache timestamp after the fare event returns a typed error.

A successful offline approval uses
`FareApprovalReason::OfflineProvisional`.

The result remains provisional until it is uploaded to the backend and
processed by reconciliation.

## Stable offline rejections

Offline evaluation can return:

- `StalePolicy`;
- `StaleRevocationData`;
- `OfflineLimitExceeded`.

Existing online rejections, including insufficient stored value and invalid
products, are preserved during offline evaluation.

## Online and offline parity

Online and offline modes share:

- the same validated fare policy;
- the same complete fare input;
- the same base and zone calculation;
- the same eligibility logic;
- the same transfer logic;
- the same fare-cap logic;
- the same transit-product logic;
- the same decision evidence.

Offline processing adds risk controls only after the shared calculation has
completed.

Integration tests verify that online and offline evaluations preserve identical
calculation evidence for identical policy and input values.

## Validation commands

Run the fare-engine test suite:

    nix develop -c cargo test -p transitguard-fare-engine

Run workspace compilation:

    nix develop -c cargo check --workspace

Run strict Clippy validation:

    nix develop -c cargo clippy \
      --workspace \
      --all-targets \
      --all-features \
      -- \
      -D warnings

Run all workspace tests:

    nix develop -c cargo test --workspace --all-features

Run the complete Nix validation:

    nix flake check
