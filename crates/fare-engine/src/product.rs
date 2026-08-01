use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{
    Currency, EventTime, FarePolicyId, FarePolicyVersion, Money, TransitProductId,
    TransitProductInstanceId,
};

use crate::{FarePolicy, ZoneId};

/// Errors produced while validating a transit product.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TransitProductError {
    /// The product validity interval ended before it began.
    #[error(
        "transit product valid-until time {valid_until} cannot precede valid-from time {valid_from}"
    )]
    InvalidValidityInterval {
        /// Inclusive beginning of the validity interval.
        valid_from: i64,

        /// Inclusive end of the validity interval.
        valid_until: i64,
    },

    /// A zone range ended before it began.
    #[error("transit product ending zone {last_zone} cannot precede starting zone {first_zone}")]
    InvalidZoneRange {
        /// First covered zone.
        first_zone: u16,

        /// Last covered zone.
        last_zone: u16,
    },
}

/// Geographic coverage provided by a fictional transit product.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum TransitProductCoverage {
    /// The product covers every project-owned fare zone.
    AllZones,

    /// The product covers one inclusive range of fare zones.
    ZoneRange {
        /// First covered zone.
        first_zone: ZoneId,

        /// Last covered zone.
        last_zone: ZoneId,
    },
}

impl TransitProductCoverage {
    /// Creates a validated inclusive zone range.
    pub fn zone_range(first_zone: ZoneId, last_zone: ZoneId) -> Result<Self, TransitProductError> {
        if last_zone < first_zone {
            return Err(TransitProductError::InvalidZoneRange {
                first_zone: first_zone.value(),
                last_zone: last_zone.value(),
            });
        }

        Ok(Self::ZoneRange {
            first_zone,
            last_zone,
        })
    }

    /// Reports whether a zone is covered.
    #[must_use]
    pub const fn covers(self, zone: ZoneId) -> bool {
        match self {
            Self::AllZones => true,
            Self::ZoneRange {
                first_zone,
                last_zone,
            } => zone.value() >= first_zone.value() && zone.value() <= last_zone.value(),
        }
    }
}

/// Unvalidated fictional transit-product input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TransitProductDefinition {
    /// Stable product identity.
    pub product_id: TransitProductId,

    /// Identity of this issued product instance.
    pub instance_id: TransitProductInstanceId,

    /// Inclusive beginning of the validity interval.
    pub valid_from: EventTime,

    /// Inclusive end of the validity interval.
    pub valid_until: EventTime,

    /// Zones in which the product may be used.
    pub coverage: TransitProductCoverage,
}

/// A validated immutable transit-product instance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TransitProduct {
    definition: TransitProductDefinition,
}

impl TransitProduct {
    /// Validates a transit-product definition.
    pub fn validate(definition: TransitProductDefinition) -> Result<Self, TransitProductError> {
        if definition.valid_until < definition.valid_from {
            return Err(TransitProductError::InvalidValidityInterval {
                valid_from: definition.valid_from.unix_milliseconds(),
                valid_until: definition.valid_until.unix_milliseconds(),
            });
        }

        Ok(Self { definition })
    }

    /// Returns the stable product identity.
    #[must_use]
    pub const fn product_id(self) -> TransitProductId {
        self.definition.product_id
    }

    /// Returns the issued product-instance identity.
    #[must_use]
    pub const fn instance_id(self) -> TransitProductInstanceId {
        self.definition.instance_id
    }

    /// Returns the inclusive beginning of validity.
    #[must_use]
    pub const fn valid_from(self) -> EventTime {
        self.definition.valid_from
    }

    /// Returns the inclusive end of validity.
    #[must_use]
    pub const fn valid_until(self) -> EventTime {
        self.definition.valid_until
    }

    /// Returns the configured zone coverage.
    #[must_use]
    pub const fn coverage(self) -> TransitProductCoverage {
        self.definition.coverage
    }

    /// Returns the complete validated definition.
    #[must_use]
    pub const fn definition(self) -> TransitProductDefinition {
        self.definition
    }
}

impl<'de> Deserialize<'de> for TransitProduct {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let definition = TransitProductDefinition::deserialize(deserializer)?;

        Self::validate(definition).map_err(serde::de::Error::custom)
    }
}

/// Stable reason that a presented product was invalid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ProductInvalidReason {
    /// The product's validity interval has not begun.
    NotYetValid,

    /// The product's validity interval has ended.
    Expired,

    /// At least one journey zone was outside product coverage.
    OutsideZoneCoverage,
}

/// Result of checking an optional transit product.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ProductApplicationOutcome {
    /// Stored value was selected without a transit product.
    NotPresented,

    /// A valid and applicable product covered the fare.
    Covered,

    /// A presented product was invalid for this journey.
    Invalid {
        /// Stable invalidity classification.
        reason: ProductInvalidReason,
    },
}

/// Errors produced while applying a transit product.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProductEvaluationError {
    /// The calculated fare used a currency different from the policy.
    #[error("transit-product fare uses {actual}, expected policy currency {expected}")]
    CurrencyMismatch {
        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the fare.
        actual: Currency,
    },

    /// Product application cannot process a negative fare.
    #[error("transit-product fare cannot be negative: {fare}")]
    NegativeFare {
        /// Invalid fare.
        fare: Money,
    },

    /// A validated product subtraction unexpectedly failed.
    #[error("transit-product fare calculation failed")]
    ArithmeticFailure,
}

/// Complete transit-product application evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ProductApplication {
    policy_id: FarePolicyId,
    policy_version: FarePolicyVersion,
    product_id: Option<TransitProductId>,
    instance_id: Option<TransitProductInstanceId>,
    outcome: ProductApplicationOutcome,
    discount_applied: Money,
    fare_after_product: Money,
}

impl ProductApplication {
    /// Returns the policy identity.
    #[must_use]
    pub const fn policy_id(self) -> FarePolicyId {
        self.policy_id
    }

    /// Returns the policy version.
    #[must_use]
    pub const fn policy_version(self) -> FarePolicyVersion {
        self.policy_version
    }

    /// Returns the presented product identity.
    #[must_use]
    pub const fn product_id(self) -> Option<TransitProductId> {
        self.product_id
    }

    /// Returns the presented product-instance identity.
    #[must_use]
    pub const fn instance_id(self) -> Option<TransitProductInstanceId> {
        self.instance_id
    }

    /// Returns the product-validation outcome.
    #[must_use]
    pub const fn outcome(self) -> ProductApplicationOutcome {
        self.outcome
    }

    /// Returns the amount covered by the product.
    #[must_use]
    pub const fn discount_applied(self) -> Money {
        self.discount_applied
    }

    /// Returns the fare remaining after product application.
    #[must_use]
    pub const fn fare_after_product(self) -> Money {
        self.fare_after_product
    }
}

/// Applies an optional fictional transit product.
///
/// Product validity is inclusive at both `valid_from` and `valid_until`.
/// A valid product must cover both the origin and destination zones.
pub fn apply_transit_product(
    policy: FarePolicy,
    product: Option<TransitProduct>,
    event_time: EventTime,
    origin_zone: ZoneId,
    destination_zone: ZoneId,
    fare_after_caps: Money,
) -> Result<ProductApplication, ProductEvaluationError> {
    validate_fare(policy, fare_after_caps)?;

    let Some(product) = product else {
        return Ok(ProductApplication {
            policy_id: policy.id(),
            policy_version: policy.version(),
            product_id: None,
            instance_id: None,
            outcome: ProductApplicationOutcome::NotPresented,
            discount_applied: Money::zero(policy.currency()),
            fare_after_product: fare_after_caps,
        });
    };

    let outcome = determine_outcome(product, event_time, origin_zone, destination_zone);

    let discount_applied = match outcome {
        ProductApplicationOutcome::Covered => fare_after_caps,
        ProductApplicationOutcome::NotPresented | ProductApplicationOutcome::Invalid { .. } => {
            Money::zero(policy.currency())
        }
    };

    let fare_after_product = fare_after_caps
        .checked_subtract(discount_applied)
        .map_err(|_| ProductEvaluationError::ArithmeticFailure)?;

    Ok(ProductApplication {
        policy_id: policy.id(),
        policy_version: policy.version(),
        product_id: Some(product.product_id()),
        instance_id: Some(product.instance_id()),
        outcome,
        discount_applied,
        fare_after_product,
    })
}

fn validate_fare(policy: FarePolicy, fare: Money) -> Result<(), ProductEvaluationError> {
    if fare.currency() != policy.currency() {
        return Err(ProductEvaluationError::CurrencyMismatch {
            expected: policy.currency(),
            actual: fare.currency(),
        });
    }

    if fare.is_negative() {
        return Err(ProductEvaluationError::NegativeFare { fare });
    }

    Ok(())
}

fn determine_outcome(
    product: TransitProduct,
    event_time: EventTime,
    origin_zone: ZoneId,
    destination_zone: ZoneId,
) -> ProductApplicationOutcome {
    if event_time < product.valid_from() {
        return ProductApplicationOutcome::Invalid {
            reason: ProductInvalidReason::NotYetValid,
        };
    }

    if event_time > product.valid_until() {
        return ProductApplicationOutcome::Invalid {
            reason: ProductInvalidReason::Expired,
        };
    }

    if !product.coverage().covers(origin_zone) || !product.coverage().covers(destination_zone) {
        return ProductApplicationOutcome::Invalid {
            reason: ProductInvalidReason::OutsideZoneCoverage,
        };
    }

    ProductApplicationOutcome::Covered
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EventTime, FarePolicyId, FarePolicyVersion, Money, TransitProductId,
        TransitProductInstanceId,
    };

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition,
        TransferWindow, ZoneId,
    };

    use super::{
        ProductApplicationOutcome, ProductInvalidReason, TransitProduct, TransitProductCoverage,
        TransitProductDefinition, TransitProductError, apply_transit_product,
    };

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

    fn product(
        valid_from: i64,
        valid_until: i64,
        coverage: TransitProductCoverage,
    ) -> TransitProduct {
        let definition = TransitProductDefinition {
            product_id: TransitProductId::generate(),
            instance_id: TransitProductInstanceId::generate(),
            valid_from: event_time(valid_from),
            valid_until: event_time(valid_until),
            coverage,
        };

        let Ok(product) = TransitProduct::validate(definition) else {
            panic!("test product must be valid");
        };

        product
    }

    #[test]
    fn invalid_validity_interval_is_rejected() {
        let definition = TransitProductDefinition {
            product_id: TransitProductId::generate(),
            instance_id: TransitProductInstanceId::generate(),
            valid_from: event_time(2_000),
            valid_until: event_time(1_999),
            coverage: TransitProductCoverage::AllZones,
        };

        assert!(matches!(
            TransitProduct::validate(definition),
            Err(TransitProductError::InvalidValidityInterval {
                valid_from: 2_000,
                valid_until: 1_999
            })
        ));
    }

    #[test]
    fn validity_boundaries_are_inclusive() {
        let product = product(1_000, 2_000, TransitProductCoverage::AllZones);

        for current_time in [1_000, 2_000] {
            let result = apply_transit_product(
                policy(),
                Some(product),
                event_time(current_time),
                zone(1),
                zone(4),
                Money::from_minor_units(400, Currency::Usd),
            );

            let Ok(application) = result else {
                panic!("boundary product must evaluate");
            };

            assert_eq!(application.outcome(), ProductApplicationOutcome::Covered);

            assert_eq!(application.fare_after_product(), Money::zero(Currency::Usd));
        }
    }

    #[test]
    fn expired_product_is_invalid() {
        let result = apply_transit_product(
            policy(),
            Some(product(1_000, 2_000, TransitProductCoverage::AllZones)),
            event_time(2_001),
            zone(1),
            zone(2),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("expired product must produce an outcome");
        };

        assert_eq!(
            application.outcome(),
            ProductApplicationOutcome::Invalid {
                reason: ProductInvalidReason::Expired,
            }
        );

        assert_eq!(
            application.fare_after_product(),
            Money::from_minor_units(250, Currency::Usd)
        );
    }

    #[test]
    fn product_must_cover_origin_and_destination() {
        let Ok(coverage) = TransitProductCoverage::zone_range(zone(1), zone(3)) else {
            panic!("ordered zone range must be valid");
        };

        let result = apply_transit_product(
            policy(),
            Some(product(1_000, 2_000, coverage)),
            event_time(1_500),
            zone(1),
            zone(4),
            Money::from_minor_units(400, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("coverage failure must produce an outcome");
        };

        assert_eq!(
            application.outcome(),
            ProductApplicationOutcome::Invalid {
                reason: ProductInvalidReason::OutsideZoneCoverage,
            }
        );
    }

    #[test]
    fn no_product_preserves_the_calculated_fare() {
        let result = apply_transit_product(
            policy(),
            None,
            event_time(1_500),
            zone(1),
            zone(2),
            Money::from_minor_units(325, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("missing product must evaluate");
        };

        assert_eq!(
            application.outcome(),
            ProductApplicationOutcome::NotPresented
        );

        assert_eq!(
            application.fare_after_product(),
            Money::from_minor_units(325, Currency::Usd)
        );
    }
}
