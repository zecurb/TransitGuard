use core::{cmp::Ordering, fmt};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{Currency, EventTime, FareApprovalReason, FareRejectionReason, Money};

use crate::{
    FareEvaluation, FareEvaluationError, FareEvaluationInput, FareEvaluationOutcome, FarePolicy,
    evaluate_fare,
};

/// Cached dataset whose timestamp was invalid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OfflineSnapshotKind {
    /// Cached fare-policy data.
    FarePolicy,

    /// Cached credential-revocation data.
    RevocationData,
}

impl fmt::Display for OfflineSnapshotKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FarePolicy => formatter.write_str("fare policy"),
            Self::RevocationData => formatter.write_str("revocation data"),
        }
    }
}

/// Explicit context required for reader-local offline evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct OfflineEvaluationContext {
    /// Time when the reader cached the fare policy.
    pub policy_cached_at: EventTime,

    /// Maximum acceptable policy-cache age.
    pub maximum_policy_age_milliseconds: u64,

    /// Time when the reader cached revocation data.
    pub revocation_data_cached_at: EventTime,

    /// Maximum acceptable revocation-cache age.
    pub maximum_revocation_age_milliseconds: u64,

    /// Maximum charge the reader may approve while offline.
    pub provisional_charge_limit: Money,
}

/// Errors produced while validating offline evaluation input.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum OfflineEvaluationError {
    /// Normal fare evaluation failed.
    #[error(transparent)]
    FareEvaluation(#[from] FareEvaluationError),

    /// The provisional limit used the wrong currency.
    #[error("offline provisional limit uses {actual}, expected policy currency {expected}")]
    ProvisionalLimitCurrencyMismatch {
        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the limit.
        actual: Currency,
    },

    /// The provisional limit cannot be negative.
    #[error("offline provisional limit cannot be negative: {limit}")]
    NegativeProvisionalLimit {
        /// Invalid provisional limit.
        limit: Money,
    },

    /// Cached data cannot have a timestamp after the fare event.
    #[error(
        "{snapshot} snapshot time {snapshot_unix_milliseconds} occurs after fare event time {event_unix_milliseconds}"
    )]
    SnapshotOccursAfterEvent {
        /// Cached dataset containing the invalid timestamp.
        snapshot: OfflineSnapshotKind,

        /// Invalid snapshot timestamp.
        snapshot_unix_milliseconds: i64,

        /// Fare-event timestamp.
        event_unix_milliseconds: i64,
    },
}

/// Complete result of deterministic offline fare evaluation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct OfflineFareEvaluation {
    evaluation: FareEvaluation,
    policy_age_milliseconds: u64,
    revocation_data_age_milliseconds: u64,
    provisional_charge_limit: Money,
}

impl OfflineFareEvaluation {
    /// Returns the underlying shared fare-engine evaluation.
    #[must_use]
    pub const fn evaluation(self) -> FareEvaluation {
        self.evaluation
    }

    /// Returns the policy-cache age at evaluation time.
    #[must_use]
    pub const fn policy_age_milliseconds(self) -> u64 {
        self.policy_age_milliseconds
    }

    /// Returns the revocation-cache age at evaluation time.
    #[must_use]
    pub const fn revocation_data_age_milliseconds(self) -> u64 {
        self.revocation_data_age_milliseconds
    }

    /// Returns the configured provisional charge limit.
    #[must_use]
    pub const fn provisional_charge_limit(self) -> Money {
        self.provisional_charge_limit
    }
}

/// Evaluates a fare using cached reader-local state.
///
/// The normal fare is calculated by [`evaluate_fare`]. Offline evaluation
/// then applies freshness and provisional-risk rules without accessing the
/// network, database, environment, system clock, or mutable global state.
///
/// Freshness boundaries are inclusive. A snapshot whose age exactly equals
/// its configured maximum remains valid.
pub fn evaluate_fare_offline(
    policy: FarePolicy,
    input: FareEvaluationInput,
    context: OfflineEvaluationContext,
) -> Result<OfflineFareEvaluation, OfflineEvaluationError> {
    validate_provisional_limit(policy, context.provisional_charge_limit)?;

    let policy_age_milliseconds = snapshot_age(
        OfflineSnapshotKind::FarePolicy,
        context.policy_cached_at,
        input.event_time,
    )?;

    let revocation_data_age_milliseconds = snapshot_age(
        OfflineSnapshotKind::RevocationData,
        context.revocation_data_cached_at,
        input.event_time,
    )?;

    let mut evaluation = evaluate_fare(policy, input)?;

    if policy_age_milliseconds > context.maximum_policy_age_milliseconds {
        evaluation = evaluation.with_outcome(FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::StalePolicy,
        });
    } else if revocation_data_age_milliseconds > context.maximum_revocation_age_milliseconds {
        evaluation = evaluation.with_outcome(FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::StaleRevocationData,
        });
    } else {
        evaluation = apply_offline_limit(evaluation, context.provisional_charge_limit)?;
    }

    Ok(OfflineFareEvaluation {
        evaluation,
        policy_age_milliseconds,
        revocation_data_age_milliseconds,
        provisional_charge_limit: context.provisional_charge_limit,
    })
}

fn validate_provisional_limit(
    policy: FarePolicy,
    limit: Money,
) -> Result<(), OfflineEvaluationError> {
    if limit.currency() != policy.currency() {
        return Err(OfflineEvaluationError::ProvisionalLimitCurrencyMismatch {
            expected: policy.currency(),
            actual: limit.currency(),
        });
    }

    if limit.is_negative() {
        return Err(OfflineEvaluationError::NegativeProvisionalLimit { limit });
    }

    Ok(())
}

fn snapshot_age(
    snapshot: OfflineSnapshotKind,
    cached_at: EventTime,
    event_time: EventTime,
) -> Result<u64, OfflineEvaluationError> {
    let snapshot_unix_milliseconds = cached_at.unix_milliseconds();

    let event_unix_milliseconds = event_time.unix_milliseconds();

    if snapshot_unix_milliseconds > event_unix_milliseconds {
        return Err(OfflineEvaluationError::SnapshotOccursAfterEvent {
            snapshot,
            snapshot_unix_milliseconds,
            event_unix_milliseconds,
        });
    }

    Ok((event_unix_milliseconds - snapshot_unix_milliseconds).unsigned_abs())
}

fn apply_offline_limit(
    evaluation: FareEvaluation,
    limit: Money,
) -> Result<FareEvaluation, OfflineEvaluationError> {
    let outcome = match evaluation.outcome() {
        FareEvaluationOutcome::Rejected { reason } => FareEvaluationOutcome::Rejected { reason },

        FareEvaluationOutcome::Approved { charged_amount, .. } => {
            let comparison = charged_amount.checked_cmp(limit).map_err(|_| {
                OfflineEvaluationError::ProvisionalLimitCurrencyMismatch {
                    expected: limit.currency(),
                    actual: charged_amount.currency(),
                }
            })?;

            if comparison == Ordering::Greater {
                FareEvaluationOutcome::Rejected {
                    reason: FareRejectionReason::OfflineLimitExceeded,
                }
            } else {
                FareEvaluationOutcome::Approved {
                    charged_amount,
                    reason: FareApprovalReason::OfflineProvisional,
                }
            }
        }
    };

    Ok(evaluation.with_outcome(outcome))
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, FareRejectionReason, Money,
    };

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FareCapHistory, FareEvaluationInput,
        FareEvaluationOutcome, FarePolicy, FarePolicyDefinition, TransferHistory, TransferWindow,
        ZoneId,
    };

    use super::{
        OfflineEvaluationContext, OfflineEvaluationError, OfflineSnapshotKind,
        evaluate_fare_offline,
    };

    const EVENT_TIME: i64 = 10_000_000;

    fn event_time(milliseconds: i64) -> EventTime {
        let Ok(time) = EventTime::from_unix_milliseconds(milliseconds) else {
            panic!("test event time must be valid");
        };

        time
    }

    fn zone(value: u16) -> ZoneId {
        let Ok(zone) = ZoneId::new(value) else {
            panic!("positive zone must be valid");
        };

        zone
    }

    fn policy() -> FarePolicy {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("version one must be valid");
        };

        let Ok(window) = TransferWindow::from_milliseconds(5_400_000) else {
            panic!("positive transfer window must be valid");
        };

        let definition = FarePolicyDefinition {
            id: FarePolicyId::generate(),
            version,
            currency: Currency::Usd,
            base_fare: Money::from_minor_units(250, Currency::Usd),
            zone_surcharge: Money::from_minor_units(75, Currency::Usd),
            transfer_window: window,
            transfer_discount: Money::from_minor_units(250, Currency::Usd),
            daily_cap: Money::from_minor_units(750, Currency::Usd),
            weekly_cap: Money::from_minor_units(3_000, Currency::Usd),
            eligibility_discounts: EligibilityDiscounts::new(
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::FULL_FARE,
            ),
        };

        let Ok(policy) = FarePolicy::validate(definition) else {
            panic!("test policy must be valid");
        };

        policy
    }

    fn input() -> FareEvaluationInput {
        FareEvaluationInput {
            event_time: event_time(EVENT_TIME),
            origin_zone: zone(1),
            destination_zone: zone(3),
            eligibility: EligibilityClassification::Standard,
            transfer_history: TransferHistory::none(),
            fare_cap_history: FareCapHistory::zero(Currency::Usd),
            transit_product: None,
            available_balance: Money::from_minor_units(1_000, Currency::Usd),
        }
    }

    fn context(
        policy_cached_at: i64,
        revocation_cached_at: i64,
        limit: i64,
    ) -> OfflineEvaluationContext {
        OfflineEvaluationContext {
            policy_cached_at: event_time(policy_cached_at),
            maximum_policy_age_milliseconds: 1_000,
            revocation_data_cached_at: event_time(revocation_cached_at),
            maximum_revocation_age_milliseconds: 1_000,
            provisional_charge_limit: Money::from_minor_units(limit, Currency::Usd),
        }
    }

    #[test]
    fn exact_freshness_boundaries_are_accepted() {
        let result = evaluate_fare_offline(
            policy(),
            input(),
            context(EVENT_TIME - 1_000, EVENT_TIME - 1_000, 400),
        );

        let Ok(result) = result else {
            panic!("boundary snapshots must evaluate");
        };

        assert_eq!(
            result.evaluation().outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(400, Currency::Usd,),
                reason: FareApprovalReason::OfflineProvisional,
            }
        );
    }

    #[test]
    fn stale_policy_produces_stable_rejection() {
        let result = evaluate_fare_offline(
            policy(),
            input(),
            context(EVENT_TIME - 1_001, EVENT_TIME - 1_000, 500),
        );

        let Ok(result) = result else {
            panic!("stale policy must produce a decision");
        };

        assert_eq!(
            result.evaluation().outcome(),
            FareEvaluationOutcome::Rejected {
                reason: FareRejectionReason::StalePolicy,
            }
        );
    }

    #[test]
    fn stale_revocation_data_produces_stable_rejection() {
        let result = evaluate_fare_offline(
            policy(),
            input(),
            context(EVENT_TIME - 1_000, EVENT_TIME - 1_001, 500),
        );

        let Ok(result) = result else {
            panic!("stale revocation data must produce a decision");
        };

        assert_eq!(
            result.evaluation().outcome(),
            FareEvaluationOutcome::Rejected {
                reason: FareRejectionReason::StaleRevocationData,
            }
        );
    }

    #[test]
    fn charge_above_offline_limit_is_rejected() {
        let result = evaluate_fare_offline(
            policy(),
            input(),
            context(EVENT_TIME - 500, EVENT_TIME - 500, 399),
        );

        let Ok(result) = result else {
            panic!("offline limit must produce a decision");
        };

        assert_eq!(
            result.evaluation().outcome(),
            FareEvaluationOutcome::Rejected {
                reason: FareRejectionReason::OfflineLimitExceeded,
            }
        );
    }

    #[test]
    fn charge_equal_to_offline_limit_is_approved() {
        let result = evaluate_fare_offline(
            policy(),
            input(),
            context(EVENT_TIME - 500, EVENT_TIME - 500, 400),
        );

        let Ok(result) = result else {
            panic!("equal offline limit must evaluate");
        };

        assert_eq!(
            result.evaluation().outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(400, Currency::Usd,),
                reason: FareApprovalReason::OfflineProvisional,
            }
        );
    }

    #[test]
    fn future_snapshot_timestamp_is_rejected() {
        let result =
            evaluate_fare_offline(policy(), input(), context(EVENT_TIME + 1, EVENT_TIME, 500));

        assert!(matches!(
            result,
            Err(OfflineEvaluationError::SnapshotOccursAfterEvent {
                snapshot: OfflineSnapshotKind::FarePolicy,
                snapshot_unix_milliseconds: 10_000_001,
                event_unix_milliseconds: 10_000_000
            })
        ));
    }

    #[test]
    fn identical_offline_inputs_produce_identical_results() {
        let policy = policy();
        let input = input();
        let context = context(EVENT_TIME - 500, EVENT_TIME - 500, 500);

        let first = evaluate_fare_offline(policy, input, context);

        let second = evaluate_fare_offline(policy, input, context);

        assert_eq!(first, second);
    }
}
