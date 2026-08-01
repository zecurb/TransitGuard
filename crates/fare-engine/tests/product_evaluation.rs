use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
    FarePolicyVersion, FareRejectionReason, Money, TransitProductId, TransitProductInstanceId,
};

use transitguard_fare_engine::{
    DiscountBasisPoints, EligibilityDiscounts, FareCapHistory, FareEvaluationInput,
    FareEvaluationOutcome, FarePolicy, FarePolicyDefinition, ProductApplicationOutcome,
    ProductInvalidReason, TransferHistory, TransferWindow, TransitProduct, TransitProductCoverage,
    TransitProductDefinition, ZoneId, evaluate_fare,
};

fn event_time(milliseconds: i64) -> EventTime {
    let Ok(time) = EventTime::from_unix_milliseconds(milliseconds) else {
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
    let Ok(version) = FarePolicyVersion::new(1) else {
        panic!("version one must be valid");
    };

    let Ok(transfer_window) = TransferWindow::from_milliseconds(5_400_000) else {
        panic!("positive transfer window must be valid");
    };

    let definition = FarePolicyDefinition {
        id: FarePolicyId::generate(),
        version,
        currency: Currency::Usd,
        base_fare: Money::from_minor_units(250, Currency::Usd),
        zone_surcharge: Money::from_minor_units(75, Currency::Usd),
        transfer_window,
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

fn product(valid_from: i64, valid_until: i64) -> TransitProduct {
    let definition = TransitProductDefinition {
        product_id: TransitProductId::generate(),
        instance_id: TransitProductInstanceId::generate(),
        valid_from: event_time(valid_from),
        valid_until: event_time(valid_until),
        coverage: TransitProductCoverage::AllZones,
    };

    let Ok(product) = TransitProduct::validate(definition) else {
        panic!("test product must be valid");
    };

    product
}

fn input(
    event_time: EventTime,
    transit_product: Option<TransitProduct>,
    balance: Money,
) -> FareEvaluationInput {
    FareEvaluationInput {
        event_time,
        origin_zone: zone(1),
        destination_zone: zone(3),
        eligibility: EligibilityClassification::Standard,
        transfer_history: TransferHistory::none(),
        fare_cap_history: FareCapHistory::zero(Currency::Usd),
        transit_product,
        available_balance: balance,
    }
}

#[test]
fn valid_product_covers_fare_without_stored_value() {
    let result = evaluate_fare(
        policy(),
        input(
            event_time(1_500),
            Some(product(1_000, 2_000)),
            Money::zero(Currency::Usd),
        ),
    );

    let Ok(evaluation) = result else {
        panic!("valid product must evaluate");
    };

    assert_eq!(
        evaluation.outcome(),
        FareEvaluationOutcome::Approved {
            charged_amount: Money::zero(Currency::Usd),
            reason: FareApprovalReason::TransitProduct,
        }
    );

    assert_eq!(
        evaluation.evidence().product_outcome(),
        ProductApplicationOutcome::Covered
    );

    assert_eq!(
        evaluation.evidence().product_discount(),
        evaluation.evidence().fare_after_caps()
    );

    assert_eq!(
        evaluation.evidence().fare_after_product(),
        Money::zero(Currency::Usd)
    );
}

#[test]
fn expired_product_returns_product_invalid_rejection() {
    let result = evaluate_fare(
        policy(),
        input(
            event_time(2_001),
            Some(product(1_000, 2_000)),
            Money::from_minor_units(1_000, Currency::Usd),
        ),
    );

    let Ok(evaluation) = result else {
        panic!("expired product must produce a decision");
    };

    assert_eq!(
        evaluation.outcome(),
        FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::ProductInvalid,
        }
    );

    assert_eq!(
        evaluation.evidence().product_outcome(),
        ProductApplicationOutcome::Invalid {
            reason: ProductInvalidReason::Expired,
        }
    );
}

#[test]
fn missing_product_preserves_stored_value_processing() {
    let result = evaluate_fare(
        policy(),
        input(
            event_time(1_500),
            None,
            Money::from_minor_units(1_000, Currency::Usd),
        ),
    );

    let Ok(evaluation) = result else {
        panic!("stored-value fare must evaluate");
    };

    assert_eq!(
        evaluation.outcome(),
        FareEvaluationOutcome::Approved {
            charged_amount: Money::from_minor_units(400, Currency::Usd,),
            reason: FareApprovalReason::StandardFare,
        }
    );

    assert_eq!(
        evaluation.evidence().product_outcome(),
        ProductApplicationOutcome::NotPresented
    );
}
