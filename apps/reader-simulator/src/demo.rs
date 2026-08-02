use thiserror::Error;
use transitguard_device_protocol::{
    CredentialMedium, DeviceProtocolVersion, ProtocolZoneId,
};
use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareCredentialId, FarePolicyId,
    FarePolicyVersion, Money, ReaderId,
};
use transitguard_fare_engine::{
    EligibilityDiscounts, FareCapHistory, FarePolicy, FarePolicyDefinition,
    OfflineEvaluationContext, TransferHistory, TransferWindow,
};

use crate::{
    ReaderConnectivity, ReaderFareContext, ReaderPresentationInput, ReaderSimulator,
    ReaderSimulatorError, ScenarioAction, ScenarioReport, run_scenario,
};

const BASE_TIME_MILLISECONDS: i64 = 1_700_000_000_000;

/// Errors produced while constructing the built-in demonstration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DemoScenarioError {
    /// A compile-time demonstration value violated a domain invariant.
    #[error("invalid static demonstration configuration: {field}")]
    InvalidStaticConfiguration {
        /// Name of the invalid demonstration field.
        field: &'static str,
    },

    /// Reader construction or startup failed.
    #[error(transparent)]
    Reader(#[from] ReaderSimulatorError),
}

/// Runs the normalized built-in reader demonstration.
///
/// The report excludes generated identities, making repeated output
/// deterministic even though the underlying simulated entities use unique
/// project-owned identifiers.
pub fn run_demo_scenario() -> Result<ScenarioReport, DemoScenarioError> {
    let mut reader = ReaderSimulator::new(
        ReaderId::generate(),
        DeviceProtocolVersion::CURRENT,
        env!("CARGO_PKG_VERSION"),
    )?;

    reader.start(ReaderConnectivity::Connected)?;

    let policy = demo_policy()?;
    let actions = demo_actions()?;

    Ok(run_scenario(&mut reader, policy, &actions))
}

fn demo_actions() -> Result<[ScenarioAction; 6], DemoScenarioError> {
    let offline_time = BASE_TIME_MILLISECONDS + 1_000;

    Ok([
        ScenarioAction::ProcessFare {
            presentation_input: presentation_input(
                BASE_TIME_MILLISECONDS,
                1,
                3,
            )?,
            fare_context: Box::new(online_context()),
        },
        ScenarioAction::SetConnectivity(
            ReaderConnectivity::Disconnected,
        ),
        ScenarioAction::ProcessFare {
            presentation_input: presentation_input(
                offline_time,
                1,
                3,
            )?,
            fare_context: Box::new(online_context()),
        },
        ScenarioAction::ProcessFare {
            presentation_input: presentation_input(
                offline_time,
                1,
                3,
            )?,
            fare_context: Box::new(
                offline_context(offline_time)?,
            ),
        },
        ScenarioAction::SetConnectivity(
            ReaderConnectivity::Connected,
        ),
        ScenarioAction::ProcessFare {
            presentation_input: presentation_input(
                BASE_TIME_MILLISECONDS + 2_000,
                2,
                2,
            )?,
            fare_context: Box::new(online_context()),
        },
    ])
}

fn demo_policy() -> Result<FarePolicy, DemoScenarioError> {
    let version = checked(
        FarePolicyVersion::new(1),
        "fare policy version",
    )?;

    let transfer_window = checked(
        TransferWindow::from_milliseconds(5_400_000),
        "transfer window",
    )?;

    checked(
        FarePolicy::validate(FarePolicyDefinition {
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
                EligibilityDiscounts::none(),
        }),
        "fare policy",
    )
}

fn presentation_input(
    event_time_milliseconds: i64,
    origin_zone: u16,
    destination_zone: u16,
) -> Result<ReaderPresentationInput, DemoScenarioError> {
    Ok(ReaderPresentationInput::new(
        FareCredentialId::generate(),
        CredentialMedium::Card,
        event_time(event_time_milliseconds)?,
        protocol_zone(origin_zone)?,
        protocol_zone(destination_zone)?,
    ))
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

fn offline_context(
    event_time_milliseconds: i64,
) -> Result<ReaderFareContext, DemoScenarioError> {
    let cached_at =
        event_time(event_time_milliseconds - 500)?;

    Ok(ReaderFareContext::offline(
        EligibilityClassification::Standard,
        TransferHistory::none(),
        FareCapHistory::zero(Currency::Usd),
        None,
        Money::from_minor_units(1_000, Currency::Usd),
        OfflineEvaluationContext {
            policy_cached_at: cached_at,
            maximum_policy_age_milliseconds: 1_000,
            revocation_data_cached_at: cached_at,
            maximum_revocation_age_milliseconds: 1_000,
            provisional_charge_limit:
                Money::from_minor_units(
                    500,
                    Currency::Usd,
                ),
        },
    ))
}

fn event_time(
    milliseconds: i64,
) -> Result<EventTime, DemoScenarioError> {
    checked(
        EventTime::from_unix_milliseconds(milliseconds),
        "event time",
    )
}

fn protocol_zone(
    value: u16,
) -> Result<ProtocolZoneId, DemoScenarioError> {
    checked(
        ProtocolZoneId::new(value),
        "protocol zone",
    )
}

fn checked<T, E>(
    result: Result<T, E>,
    field: &'static str,
) -> Result<T, DemoScenarioError> {
    result.map_err(|_| {
        DemoScenarioError::InvalidStaticConfiguration {
            field,
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        ScenarioFailureCategory, ScenarioStepResult,
    };

    use super::run_demo_scenario;

    #[test]
    fn demo_scenario_is_reproducible() {
        let first = run_demo_scenario();
        let second = run_demo_scenario();

        let (Ok(first), Ok(second)) = (first, second)
        else {
            panic!(
                "built-in demonstration must be valid"
            );
        };

        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 6);

        let Some(failed_offline_step) =
            first.steps.get(2)
        else {
            panic!(
                "offline failure step must exist"
            );
        };

        assert_eq!(
            failed_offline_step.result,
            ScenarioStepResult::Failed {
                category:
                    ScenarioFailureCategory::
                        MissingOfflineContext,
            }
        );

        assert_eq!(
            failed_offline_step.next_local_sequence,
            2
        );

        let Some(final_step) = first.steps.last()
        else {
            panic!("final demonstration step must exist");
        };

        assert_eq!(final_step.next_local_sequence, 4);
    }
}
