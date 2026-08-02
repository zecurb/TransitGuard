use serde::Serialize;
use transitguard_domain::{FareProcessingMode, Money};
use transitguard_fare_engine::{FareEvaluationOutcome, FarePolicy};

use crate::{
    ReaderConnectivity, ReaderFareContext, ReaderFareProcessingError, ReaderOperatingState,
    ReaderPresentationInput, ReaderSimulator, ReaderSimulatorError,
};

/// One operation in a reader-simulator scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioAction {
    /// Change simulated backend connectivity.
    SetConnectivity(ReaderConnectivity),

    /// Process one fictional credential presentation.
    ProcessFare {
        /// Protocol presentation input.
        presentation_input: ReaderPresentationInput,

        /// Fare history, balance, product, and offline context.
        fare_context: Box<ReaderFareContext>,
    },
}

/// Stable classification of a scenario action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ScenarioActionKind {
    /// Connectivity was changed.
    ConnectivityChange,

    /// A fare presentation was attempted.
    FarePresentation,
}

/// Stable failure category recorded by the scenario runner.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ScenarioFailureCategory {
    /// Reader configuration was invalid.
    ReaderConfiguration,

    /// Reader was not ready to accept a presentation.
    ReaderNotReady,

    /// Reader lifecycle transition was invalid.
    ReaderStateTransition,

    /// Reader-local sequence values were exhausted.
    SequenceExhausted,

    /// Normal fare input or calculation was invalid.
    FareEvaluation,

    /// Offline freshness or risk input was invalid.
    OfflineEvaluation,

    /// An offline presentation omitted required offline context.
    MissingOfflineContext,

    /// A protocol zone could not be translated.
    ZoneTranslation,
}

/// Result recorded for one scenario step.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ScenarioStepResult {
    /// Connectivity changed successfully.
    ConnectivityChanged {
        /// New simulated connectivity.
        connectivity: ReaderConnectivity,
    },

    /// A fare presentation was processed successfully.
    FareProcessed {
        /// Reader-local sequence assigned to the presentation.
        local_sequence: u64,

        /// Online or offline processing mode.
        processing_mode: FareProcessingMode,

        /// Stable approved or rejected fare outcome.
        outcome: FareEvaluationOutcome,

        /// Final calculated fare before account mutation.
        final_fare: Money,
    },

    /// The action failed without terminating the scenario.
    Failed {
        /// Stable failure classification.
        category: ScenarioFailureCategory,
    },
}

/// Observable record produced after one scenario action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ScenarioStepRecord {
    /// One-based position in the scenario.
    pub step_number: usize,

    /// Kind of action attempted.
    pub action: ScenarioActionKind,

    /// Reader state after the action.
    pub state: ReaderOperatingState,

    /// Next sequence available after the action.
    pub next_local_sequence: u64,

    /// Action result.
    pub result: ScenarioStepResult,
}

/// Complete normalized scenario result.
///
/// Generated reader, credential, policy, and transaction identifiers are
/// deliberately excluded so repeated runs can be compared deterministically.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScenarioReport {
    /// Ordered records for every attempted action.
    pub steps: Vec<ScenarioStepRecord>,
}

/// Executes every action and records successes and failures.
///
/// A failed step does not terminate later scenario actions.
#[must_use]
pub fn run_scenario(
    reader: &mut ReaderSimulator,
    policy: FarePolicy,
    actions: &[ScenarioAction],
) -> ScenarioReport {
    let mut steps = Vec::with_capacity(actions.len());

    for (index, action) in actions.iter().cloned().enumerate() {
        let (action_kind, result) = match action {
            ScenarioAction::SetConnectivity(connectivity) => {
                let result = match reader.set_connectivity(connectivity) {
                    Ok(()) => ScenarioStepResult::ConnectivityChanged { connectivity },

                    Err(error) => ScenarioStepResult::Failed {
                        category: map_reader_error(error),
                    },
                };

                (ScenarioActionKind::ConnectivityChange, result)
            }

            ScenarioAction::ProcessFare {
                presentation_input,
                fare_context,
            } => {
                let result = match reader.process_fare(policy, presentation_input, *fare_context) {
                    Ok(decision) => {
                        let presentation = decision.presentation();

                        let evaluation = decision.fare_evaluation().evaluation();

                        ScenarioStepResult::FareProcessed {
                            local_sequence: presentation.local_sequence_number().value(),
                            processing_mode: presentation.processing_mode(),
                            outcome: evaluation.outcome(),
                            final_fare: evaluation.evidence().final_fare(),
                        }
                    }

                    Err(error) => ScenarioStepResult::Failed {
                        category: map_fare_error(error),
                    },
                };

                (ScenarioActionKind::FarePresentation, result)
            }
        };

        let health = reader.health_snapshot();

        steps.push(ScenarioStepRecord {
            step_number: index + 1,
            action: action_kind,
            state: health.state,
            next_local_sequence: health.next_local_sequence,
            result,
        });
    }

    ScenarioReport { steps }
}

fn map_fare_error(error: ReaderFareProcessingError) -> ScenarioFailureCategory {
    match error {
        ReaderFareProcessingError::Reader(error) => map_reader_error(error),

        ReaderFareProcessingError::FareEvaluation(_) => ScenarioFailureCategory::FareEvaluation,

        ReaderFareProcessingError::OfflineEvaluation(_) => {
            ScenarioFailureCategory::OfflineEvaluation
        }

        ReaderFareProcessingError::MissingOfflineContext => {
            ScenarioFailureCategory::MissingOfflineContext
        }

        ReaderFareProcessingError::ZoneTranslationFailed { .. } => {
            ScenarioFailureCategory::ZoneTranslation
        }
    }
}

fn map_reader_error(error: ReaderSimulatorError) -> ScenarioFailureCategory {
    match error {
        ReaderSimulatorError::InvalidSoftwareVersion => {
            ScenarioFailureCategory::ReaderConfiguration
        }

        ReaderSimulatorError::InvalidStateTransition { .. } => {
            ScenarioFailureCategory::ReaderStateTransition
        }

        ReaderSimulatorError::NotReady { .. } => ScenarioFailureCategory::ReaderNotReady,

        ReaderSimulatorError::SequenceExhausted => ScenarioFailureCategory::SequenceExhausted,
    }
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{CredentialMedium, DeviceProtocolVersion, ProtocolZoneId};
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareCredentialId, FarePolicyId,
        FarePolicyVersion, FareProcessingMode, Money, ReaderId,
    };
    use transitguard_fare_engine::{
        DiscountBasisPoints, EligibilityDiscounts, FareCapHistory, FarePolicy,
        FarePolicyDefinition, OfflineEvaluationContext, TransferHistory, TransferWindow,
    };

    use crate::{ReaderConnectivity, ReaderFareContext, ReaderPresentationInput, ReaderSimulator};

    use super::{ScenarioAction, ScenarioFailureCategory, ScenarioStepResult, run_scenario};

    const BASE_TIME: i64 = 1_700_000_000_000;

    fn event_time(milliseconds: i64) -> EventTime {
        let Ok(value) = EventTime::from_unix_milliseconds(milliseconds) else {
            panic!("test event time must be valid");
        };

        value
    }

    fn zone(value: u16) -> ProtocolZoneId {
        let Ok(zone) = ProtocolZoneId::new(value) else {
            panic!("positive protocol zone must be valid");
        };

        zone
    }

    fn policy() -> FarePolicy {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("policy version one must be valid");
        };

        let Ok(transfer_window) = TransferWindow::from_milliseconds(5_400_000) else {
            panic!("transfer window must be valid");
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
            panic!("scenario policy must be valid");
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
            panic!("scenario reader must be valid");
        };

        reader
    }

    fn presentation_input(
        milliseconds: i64,
        origin: u16,
        destination: u16,
    ) -> ReaderPresentationInput {
        ReaderPresentationInput::new(
            FareCredentialId::generate(),
            CredentialMedium::Card,
            event_time(milliseconds),
            zone(origin),
            zone(destination),
        )
    }

    fn online_context() -> ReaderFareContext {
        ReaderFareContext::online(
            EligibilityClassification::Standard,
            TransferHistory::none(),
            FareCapHistory::zero(Currency::Usd),
            None,
            Money::from_minor_units(1_000, Currency::Usd),
        )
    }

    fn offline_context(milliseconds: i64) -> ReaderFareContext {
        ReaderFareContext::offline(
            EligibilityClassification::Standard,
            TransferHistory::none(),
            FareCapHistory::zero(Currency::Usd),
            None,
            Money::from_minor_units(1_000, Currency::Usd),
            OfflineEvaluationContext {
                policy_cached_at: event_time(milliseconds - 500),
                maximum_policy_age_milliseconds: 1_000,
                revocation_data_cached_at: event_time(milliseconds - 500),
                maximum_revocation_age_milliseconds: 1_000,
                provisional_charge_limit: Money::from_minor_units(500, Currency::Usd),
            },
        )
    }

    fn actions() -> [ScenarioAction; 6] {
        let offline_time = BASE_TIME + 1_000;

        [
            ScenarioAction::ProcessFare {
                presentation_input: presentation_input(BASE_TIME, 1, 3),
                fare_context: Box::new(online_context()),
            },
            ScenarioAction::SetConnectivity(ReaderConnectivity::Disconnected),
            ScenarioAction::ProcessFare {
                presentation_input: presentation_input(offline_time, 1, 3),
                fare_context: Box::new(online_context()),
            },
            ScenarioAction::ProcessFare {
                presentation_input: presentation_input(offline_time, 1, 3),
                fare_context: Box::new(offline_context(offline_time)),
            },
            ScenarioAction::SetConnectivity(ReaderConnectivity::Connected),
            ScenarioAction::ProcessFare {
                presentation_input: presentation_input(BASE_TIME + 2_000, 2, 2),
                fare_context: Box::new(online_context()),
            },
        ]
    }

    #[test]
    fn repeated_scenarios_are_reproducible() {
        let base_reader = reader();
        let mut first_reader = base_reader.clone();
        let mut second_reader = base_reader;

        assert_eq!(first_reader.start(ReaderConnectivity::Connected,), Ok(()));

        assert_eq!(second_reader.start(ReaderConnectivity::Connected,), Ok(()));

        let actions = actions();
        let policy = policy();

        let first = run_scenario(&mut first_reader, policy, &actions);

        let second = run_scenario(&mut second_reader, policy, &actions);

        assert_eq!(first, second);
    }

    #[test]
    fn failed_offline_step_does_not_consume_sequence() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected,), Ok(()));

        let report = run_scenario(&mut reader, policy(), &actions());

        let Some(failed_step) = report.steps.get(2) else {
            panic!("failed scenario step must exist");
        };

        assert_eq!(
            failed_step.result,
            ScenarioStepResult::Failed {
                category: ScenarioFailureCategory::MissingOfflineContext,
            }
        );

        assert_eq!(failed_step.next_local_sequence, 2);

        let Some(offline_step) = report.steps.get(3) else {
            panic!("offline scenario step must exist");
        };

        assert_eq!(
            offline_step.result,
            ScenarioStepResult::FareProcessed {
                local_sequence: 2,
                processing_mode: FareProcessingMode::Offline,
                outcome: transitguard_fare_engine::FareEvaluationOutcome::Approved {
                    charged_amount: Money::from_minor_units(400, Currency::Usd,),
                    reason: transitguard_domain::FareApprovalReason::OfflineProvisional,
                },
                final_fare: Money::from_minor_units(400, Currency::Usd,),
            }
        );

        assert_eq!(offline_step.next_local_sequence, 3);
    }

    #[test]
    fn connectivity_scenario_returns_online() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected,), Ok(()));

        let report = run_scenario(&mut reader, policy(), &actions());

        let Some(final_step) = report.steps.last() else {
            panic!("final scenario step must exist");
        };

        assert_eq!(final_step.state, crate::ReaderOperatingState::ReadyOnline);

        assert_eq!(final_step.next_local_sequence, 4);
    }
}
