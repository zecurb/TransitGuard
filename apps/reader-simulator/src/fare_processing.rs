use serde::Serialize;
use thiserror::Error;
use transitguard_device_protocol::{CredentialPresentation, ProtocolZoneId};
use transitguard_domain::{EligibilityClassification, FareProcessingMode, Money};
use transitguard_fare_engine::{
    FareCapHistory, FareEvaluation, FareEvaluationError, FareEvaluationInput, FarePolicy,
    OfflineEvaluationContext, OfflineEvaluationError, OfflineFareEvaluation, TransferHistory,
    TransitProduct, ZoneId, evaluate_fare, evaluate_fare_offline,
};

use crate::{ReaderOperatingState, ReaderPresentationInput, ReaderSimulator, ReaderSimulatorError};

/// Fare data supplied to the reader for one simulated tap.
///
/// Presentation time and journey zones come from
/// [`ReaderPresentationInput`] so the protocol message and fare evaluation
/// cannot use different journey values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ReaderFareContext {
    eligibility: EligibilityClassification,
    transfer_history: TransferHistory,
    fare_cap_history: FareCapHistory,
    transit_product: Option<TransitProduct>,
    available_balance: Money,
    offline_context: Option<OfflineEvaluationContext>,
}

impl ReaderFareContext {
    /// Creates context for online fare processing.
    #[must_use]
    pub const fn online(
        eligibility: EligibilityClassification,
        transfer_history: TransferHistory,
        fare_cap_history: FareCapHistory,
        transit_product: Option<TransitProduct>,
        available_balance: Money,
    ) -> Self {
        Self {
            eligibility,
            transfer_history,
            fare_cap_history,
            transit_product,
            available_balance,
            offline_context: None,
        }
    }

    /// Creates context for bounded offline fare processing.
    #[must_use]
    pub const fn offline(
        eligibility: EligibilityClassification,
        transfer_history: TransferHistory,
        fare_cap_history: FareCapHistory,
        transit_product: Option<TransitProduct>,
        available_balance: Money,
        offline_context: OfflineEvaluationContext,
    ) -> Self {
        Self {
            eligibility,
            transfer_history,
            fare_cap_history,
            transit_product,
            available_balance,
            offline_context: Some(offline_context),
        }
    }

    /// Returns the rider eligibility classification.
    #[must_use]
    pub const fn eligibility(self) -> EligibilityClassification {
        self.eligibility
    }

    /// Returns the previous paid-fare history.
    #[must_use]
    pub const fn transfer_history(self) -> TransferHistory {
        self.transfer_history
    }

    /// Returns accumulated daily and weekly charges.
    #[must_use]
    pub const fn fare_cap_history(self) -> FareCapHistory {
        self.fare_cap_history
    }

    /// Returns the optional transit product.
    #[must_use]
    pub const fn transit_product(self) -> Option<TransitProduct> {
        self.transit_product
    }

    /// Returns available stored value.
    #[must_use]
    pub const fn available_balance(self) -> Money {
        self.available_balance
    }

    /// Returns offline risk and freshness context.
    #[must_use]
    pub const fn offline_context(self) -> Option<OfflineEvaluationContext> {
        self.offline_context
    }
}

/// Fare result produced according to reader connectivity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderFareEvaluation {
    /// Fare evaluated while the backend was available.
    Online(FareEvaluation),

    /// Fare evaluated using bounded reader-local rules.
    Offline(OfflineFareEvaluation),
}

impl ReaderFareEvaluation {
    /// Returns the shared deterministic fare evaluation.
    #[must_use]
    pub const fn evaluation(self) -> FareEvaluation {
        match self {
            Self::Online(evaluation) => evaluation,
            Self::Offline(evaluation) => evaluation.evaluation(),
        }
    }

    /// Returns the processing mode represented by this result.
    #[must_use]
    pub const fn processing_mode(self) -> FareProcessingMode {
        match self {
            Self::Online(_) => FareProcessingMode::Online,
            Self::Offline(_) => FareProcessingMode::Offline,
        }
    }
}

/// Complete reader response for one simulated credential tap.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ReaderTapDecision {
    presentation: CredentialPresentation,
    fare_evaluation: ReaderFareEvaluation,
}

impl ReaderTapDecision {
    /// Returns the project-owned device-protocol presentation.
    #[must_use]
    pub const fn presentation(self) -> CredentialPresentation {
        self.presentation
    }

    /// Returns the fare calculation result.
    #[must_use]
    pub const fn fare_evaluation(self) -> ReaderFareEvaluation {
        self.fare_evaluation
    }
}

/// Errors produced while connecting a reader tap to fare evaluation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReaderFareProcessingError {
    /// Reader lifecycle or sequence assignment failed.
    #[error(transparent)]
    Reader(#[from] ReaderSimulatorError),

    /// Online fare evaluation failed.
    #[error(transparent)]
    FareEvaluation(#[from] FareEvaluationError),

    /// Offline fare evaluation failed.
    #[error(transparent)]
    OfflineEvaluation(#[from] OfflineEvaluationError),

    /// Offline operation requires freshness and risk-limit data.
    #[error("offline reader processing requires an offline evaluation context")]
    MissingOfflineContext,

    /// A protocol zone could not be translated into a fare zone.
    #[error("protocol zone {value} could not be translated into a fare zone")]
    ZoneTranslationFailed {
        /// Invalid protocol-zone value.
        value: u16,
    },
}

impl ReaderSimulator {
    /// Processes one credential tap using the shared deterministic fare engine.
    ///
    /// Fare calculation is completed before a reader-local sequence is
    /// consumed. Invalid fare input therefore does not advance sequence state.
    pub fn process_fare(
        &mut self,
        policy: FarePolicy,
        presentation_input: ReaderPresentationInput,
        fare_context: ReaderFareContext,
    ) -> Result<ReaderTapDecision, ReaderFareProcessingError> {
        let processing_mode = match self.state() {
            ReaderOperatingState::ReadyOnline => FareProcessingMode::Online,

            ReaderOperatingState::ReadyOffline => FareProcessingMode::Offline,

            ReaderOperatingState::Booting | ReaderOperatingState::Stopped => {
                return Err(ReaderSimulatorError::NotReady {
                    state: self.state(),
                }
                .into());
            }
        };

        let origin_zone = translate_zone(presentation_input.origin_zone())?;

        let destination_zone = translate_zone(presentation_input.destination_zone())?;

        let evaluation_input = FareEvaluationInput {
            event_time: presentation_input.event_time(),
            origin_zone,
            destination_zone,
            eligibility: fare_context.eligibility(),
            transfer_history: fare_context.transfer_history(),
            fare_cap_history: fare_context.fare_cap_history(),
            transit_product: fare_context.transit_product(),
            available_balance: fare_context.available_balance(),
        };

        let fare_evaluation = match processing_mode {
            FareProcessingMode::Online => {
                ReaderFareEvaluation::Online(evaluate_fare(policy, evaluation_input)?)
            }

            FareProcessingMode::Offline => {
                let Some(offline_context) = fare_context.offline_context() else {
                    return Err(ReaderFareProcessingError::MissingOfflineContext);
                };

                ReaderFareEvaluation::Offline(evaluate_fare_offline(
                    policy,
                    evaluation_input,
                    offline_context,
                )?)
            }
        };

        let presentation = self.present_credential(presentation_input)?;

        Ok(ReaderTapDecision {
            presentation,
            fare_evaluation,
        })
    }
}

fn translate_zone(zone: ProtocolZoneId) -> Result<ZoneId, ReaderFareProcessingError> {
    ZoneId::new(zone.value()).map_err(|_| ReaderFareProcessingError::ZoneTranslationFailed {
        value: zone.value(),
    })
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{CredentialMedium, DeviceProtocolVersion, ProtocolZoneId};
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FareCredentialId,
        FarePolicyId, FarePolicyVersion, FareProcessingMode, Money, ReaderId,
    };
    use transitguard_fare_engine::{
        DiscountBasisPoints, EligibilityDiscounts, FareCapHistory, FareEvaluationError,
        FareEvaluationOutcome, FarePolicy, FarePolicyDefinition, OfflineEvaluationContext,
        TransferHistory, TransferWindow,
    };

    use crate::{ReaderConnectivity, ReaderPresentationInput, ReaderSimulator};

    use super::{ReaderFareContext, ReaderFareEvaluation, ReaderFareProcessingError};

    const EVENT_TIME_MILLISECONDS: i64 = 1_700_000_000_000;

    fn event_time(milliseconds: i64) -> EventTime {
        let Ok(value) = EventTime::from_unix_milliseconds(milliseconds) else {
            panic!("test event time must be valid");
        };

        value
    }

    fn protocol_zone(value: u16) -> ProtocolZoneId {
        let Ok(zone) = ProtocolZoneId::new(value) else {
            panic!("positive protocol zone must be valid");
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
            panic!("test fare policy must be valid");
        };

        policy
    }

    fn reader() -> ReaderSimulator {
        let result = ReaderSimulator::new(
            ReaderId::generate(),
            DeviceProtocolVersion::CURRENT,
            "0.1.0",
        );

        let Ok(reader) = result else {
            panic!("test reader must be valid");
        };

        reader
    }

    fn presentation_input() -> ReaderPresentationInput {
        ReaderPresentationInput::new(
            FareCredentialId::generate(),
            CredentialMedium::Card,
            event_time(EVENT_TIME_MILLISECONDS),
            protocol_zone(1),
            protocol_zone(3),
        )
    }

    fn online_context(currency: Currency) -> ReaderFareContext {
        ReaderFareContext::online(
            EligibilityClassification::Standard,
            TransferHistory::none(),
            FareCapHistory::zero(currency),
            None,
            Money::from_minor_units(1_000, currency),
        )
    }

    fn offline_context() -> ReaderFareContext {
        ReaderFareContext::offline(
            EligibilityClassification::Standard,
            TransferHistory::none(),
            FareCapHistory::zero(Currency::Usd),
            None,
            Money::from_minor_units(1_000, Currency::Usd),
            OfflineEvaluationContext {
                policy_cached_at: event_time(EVENT_TIME_MILLISECONDS - 500),
                maximum_policy_age_milliseconds: 1_000,
                revocation_data_cached_at: event_time(EVENT_TIME_MILLISECONDS - 500),
                maximum_revocation_age_milliseconds: 1_000,
                provisional_charge_limit: Money::from_minor_units(500, Currency::Usd),
            },
        )
    }

    #[test]
    fn online_reader_returns_standard_fare() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected,), Ok(()));

        let result = reader.process_fare(
            policy(),
            presentation_input(),
            online_context(Currency::Usd),
        );

        let Ok(decision) = result else {
            panic!("online fare must evaluate");
        };

        assert_eq!(
            decision.presentation().processing_mode(),
            FareProcessingMode::Online
        );

        assert_eq!(
            decision.fare_evaluation().processing_mode(),
            FareProcessingMode::Online
        );

        assert_eq!(
            decision.fare_evaluation().evaluation().outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(400, Currency::Usd,),
                reason: FareApprovalReason::StandardFare,
            }
        );
    }

    #[test]
    fn offline_reader_returns_provisional_fare() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Disconnected,), Ok(()));

        let result = reader.process_fare(policy(), presentation_input(), offline_context());

        let Ok(decision) = result else {
            panic!("offline fare must evaluate");
        };

        assert!(matches!(
            decision.fare_evaluation(),
            ReaderFareEvaluation::Offline(_)
        ));

        assert_eq!(
            decision.presentation().processing_mode(),
            FareProcessingMode::Offline
        );

        assert_eq!(
            decision.fare_evaluation().evaluation().outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(400, Currency::Usd,),
                reason: FareApprovalReason::OfflineProvisional,
            }
        );
    }

    #[test]
    fn offline_reader_requires_offline_context() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Disconnected,), Ok(()));

        let result = reader.process_fare(
            policy(),
            presentation_input(),
            online_context(Currency::Usd),
        );

        assert_eq!(
            result,
            Err(ReaderFareProcessingError::MissingOfflineContext)
        );

        assert_eq!(reader.health_snapshot().next_local_sequence, 1);
    }

    #[test]
    fn fare_error_does_not_consume_sequence() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected,), Ok(()));

        let result = reader.process_fare(
            policy(),
            presentation_input(),
            online_context(Currency::Eur),
        );

        assert_eq!(
            result,
            Err(ReaderFareProcessingError::FareEvaluation(
                FareEvaluationError::BalanceCurrencyMismatch {
                    expected: Currency::Usd,
                    actual: Currency::Eur,
                },
            ),)
        );

        assert_eq!(reader.health_snapshot().next_local_sequence, 1);
    }

    #[test]
    fn protocol_and_fare_evidence_use_same_journey() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected,), Ok(()));

        let result = reader.process_fare(
            policy(),
            presentation_input(),
            online_context(Currency::Usd),
        );

        let Ok(decision) = result else {
            panic!("fare must evaluate");
        };

        assert_eq!(
            decision.presentation().event_time(),
            decision.fare_evaluation().evaluation().event_time()
        );

        assert_eq!(
            decision
                .fare_evaluation()
                .evaluation()
                .evidence()
                .additional_zone_count(),
            2
        );
    }
}
