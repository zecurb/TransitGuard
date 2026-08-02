use transitguard_domain::{
    Currency, EligibilityClassification, EventTime,
    FareApprovalReason, FarePolicyId, FarePolicyVersion,
    FareRejectionReason, Money,
};

use transitguard_fare_engine::{
    DiscountBasisPoints, EligibilityDiscounts, FareCapHistory,
    FareEvaluationInput, FareEvaluationOutcome, FarePolicy,
    FarePolicyDefinition, OfflineEvaluationContext,
    TransferHistory, TransferWindow, ZoneId, evaluate_fare,
    evaluate_fare_offline,
};

const EVENT_TIME_MILLISECONDS: i64 = 10_000_000;

fn event_time(milliseconds: i64) -> EventTime {
    let Ok(time) =
        EventTime::from_unix_milliseconds(milliseconds)
    else {
        panic!("test event time must be valid");
    };

    time
}

fn zone(value: u16) -> ZoneId {
    let Ok(zone) = ZoneId::new(value) else {
        panic!("positive zone identifier must be valid");
    };

    zone
}

fn policy() -> FarePolicy {
    let Ok(version) = FarePolicyVersion::new(7) else {
        panic!("positive policy version must be valid");
    };

    let Ok(transfer_window) =
        TransferWindow::from_milliseconds(5_400_000)
    else {
        panic!("positive transfer window must be valid");
    };

    let Ok(youth_discount) =
        DiscountBasisPoints::new(5_000)
    else {
        panic!("fifty-percent discount must be valid");
    };

    let definition = FarePolicyDefinition {
        id: FarePolicyId::generate(),
        version,
        currency: Currency::Usd,
        base_fare: Money::from_minor_units(
            250,
            Currency::Usd,
        ),
        zone_surcharge: Money::from_minor_units(
            75,
            Currency::Usd,
        ),
        transfer_window,
        transfer_discount: Money::from_minor_units(
            250,
            Currency::Usd,
        ),
        daily_cap: Money::from_minor_units(
            750,
            Currency::Usd,
        ),
        weekly_cap: Money::from_minor_units(
            3_000,
            Currency::Usd,
        ),
        eligibility_discounts:
            EligibilityDiscounts::new(
                youth_discount,
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::FULL_FARE,
            ),
    };

    let Ok(policy) = FarePolicy::validate(definition) else {
        panic!("test fare policy must be valid");
    };

    policy
}

fn input(balance_minor_units: i64) -> FareEvaluationInput {
    FareEvaluationInput {
        event_time: event_time(EVENT_TIME_MILLISECONDS),
        origin_zone: zone(1),
        destination_zone: zone(3),
        eligibility: EligibilityClassification::Youth,
        transfer_history: TransferHistory::none(),
        fare_cap_history: FareCapHistory::zero(
            Currency::Usd,
        ),
        transit_product: None,
        available_balance: Money::from_minor_units(
            balance_minor_units,
            Currency::Usd,
        ),
    }
}

fn offline_context(
    provisional_limit_minor_units: i64,
) -> OfflineEvaluationContext {
    OfflineEvaluationContext {
        policy_cached_at: event_time(
            EVENT_TIME_MILLISECONDS - 500,
        ),
        maximum_policy_age_milliseconds: 1_000,
        revocation_data_cached_at: event_time(
            EVENT_TIME_MILLISECONDS - 500,
        ),
        maximum_revocation_age_milliseconds: 1_000,
        provisional_charge_limit:
            Money::from_minor_units(
                provisional_limit_minor_units,
                Currency::Usd,
            ),
    }
}

#[test]
fn online_and_offline_modes_share_calculation_evidence() {
    let policy = policy();
    let input = input(1_000);

    let online_result = evaluate_fare(policy, input);

    let offline_result = evaluate_fare_offline(
        policy,
        input,
        offline_context(1_000),
    );

    let Ok(online) = online_result else {
        panic!("online evaluation must succeed");
    };

    let Ok(offline) = offline_result else {
        panic!("offline evaluation must succeed");
    };

    let offline_evaluation = offline.evaluation();

    assert_eq!(
        online.evidence(),
        offline_evaluation.evidence()
    );

    assert_eq!(
        online.policy_id(),
        offline_evaluation.policy_id()
    );

    assert_eq!(
        online.policy_version(),
        offline_evaluation.policy_version()
    );

    assert_eq!(
        online.event_time(),
        offline_evaluation.event_time()
    );

    assert_eq!(
        online.outcome(),
        FareEvaluationOutcome::Approved {
            charged_amount: Money::from_minor_units(
                200,
                Currency::Usd,
            ),
            reason: FareApprovalReason::StandardFare,
        }
    );

    assert_eq!(
        offline_evaluation.outcome(),
        FareEvaluationOutcome::Approved {
            charged_amount: Money::from_minor_units(
                200,
                Currency::Usd,
            ),
            reason:
                FareApprovalReason::OfflineProvisional,
        }
    );
}

#[test]
fn offline_mode_preserves_online_insufficient_balance_rejection() {
    let policy = policy();
    let input = input(199);

    let online_result = evaluate_fare(policy, input);

    let offline_result = evaluate_fare_offline(
        policy,
        input,
        offline_context(1_000),
    );

    let Ok(online) = online_result else {
        panic!("online rejection must be produced");
    };

    let Ok(offline) = offline_result else {
        panic!("offline rejection must be produced");
    };

    assert_eq!(
        online.outcome(),
        FareEvaluationOutcome::Rejected {
            reason:
                FareRejectionReason::InsufficientStoredValue,
        }
    );

    assert_eq!(
        offline.evaluation().outcome(),
        online.outcome()
    );

    assert_eq!(
        offline.evaluation().evidence(),
        online.evidence()
    );
}

#[test]
fn offline_risk_rules_do_not_change_calculated_fare() {
    let policy = policy();
    let input = input(1_000);

    let online_result = evaluate_fare(policy, input);

    let offline_result = evaluate_fare_offline(
        policy,
        input,
        offline_context(199),
    );

    let Ok(online) = online_result else {
        panic!("online evaluation must succeed");
    };

    let Ok(offline) = offline_result else {
        panic!("offline evaluation must produce a decision");
    };

    assert_eq!(
        offline.evaluation().outcome(),
        FareEvaluationOutcome::Rejected {
            reason:
                FareRejectionReason::OfflineLimitExceeded,
        }
    );

    assert_eq!(
        offline.evaluation().evidence().final_fare(),
        Money::from_minor_units(200, Currency::Usd)
    );

    assert_eq!(
        offline.evaluation().evidence(),
        online.evidence()
    );
}

#[test]
fn repeated_online_and_offline_evaluations_are_identical() {
    let policy = policy();
    let input = input(1_000);
    let context = offline_context(1_000);

    assert_eq!(
        evaluate_fare(policy, input),
        evaluate_fare(policy, input)
    );

    assert_eq!(
        evaluate_fare_offline(policy, input, context),
        evaluate_fare_offline(policy, input, context)
    );
}
