use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_device_protocol::{
    SynchronizationAcknowledgementEntry as ProtocolAcknowledgementEntry,
    SynchronizationBatchAcknowledgement, SynchronizationEntryOutcome,
};
use transitguard_domain::FareTransactionId;

use crate::{
    ReaderAcknowledgementError, StoredSynchronizationAcknowledgement,
    SynchronizationAcknowledgement, SynchronizationAcknowledgementEntry,
    SynchronizationEntryResolution, store_synchronization_acknowledgement,
};

/// Failures produced while translating a protocol acknowledgement into
/// reader-local durable acknowledgement state.
#[derive(Debug, Error)]
pub enum ReaderProtocolAcknowledgementError {
    /// A failed entry did not contain its required stable category.
    #[error(
        "synchronization acknowledgement entry {transaction_id} \
         is missing its failure category"
    )]
    MissingFailureCategory {
        /// Transaction with incomplete failure metadata.
        transaction_id: FareTransactionId,
    },

    /// A retryable entry did not contain its required retry time.
    #[error(
        "synchronization acknowledgement entry {transaction_id} \
         is missing its retry time"
    )]
    MissingRetryTime {
        /// Transaction with incomplete retry metadata.
        transaction_id: FareTransactionId,
    },

    /// Reader-local acknowledgement validation or storage failed.
    #[error(transparent)]
    Acknowledgement(#[from] ReaderAcknowledgementError),
}

/// Translates a validated backend protocol acknowledgement into the
/// reader-local persistence acknowledgement model.
pub fn translate_protocol_synchronization_acknowledgement(
    acknowledgement: &SynchronizationBatchAcknowledgement,
) -> Result<SynchronizationAcknowledgement, ReaderProtocolAcknowledgementError> {
    let entries = acknowledgement
        .entries()
        .iter()
        .map(translate_entry)
        .collect::<Result<Vec<_>, _>>()?;

    let translated = SynchronizationAcknowledgement::new(
        acknowledgement.reader_id(),
        acknowledgement.batch_id(),
        acknowledgement.protocol_version(),
        acknowledgement.first_local_sequence_number(),
        acknowledgement.last_local_sequence_number(),
        acknowledgement.received_at_unix_milliseconds(),
        entries,
    )?;

    Ok(translated)
}

/// Translates and durably stores one backend protocol acknowledgement.
///
/// Identical responses remain idempotent because the underlying SQLite
/// acknowledgement store compares the canonical durable payload.
pub async fn store_protocol_synchronization_acknowledgement(
    pool: &SqlitePool,
    acknowledgement: &SynchronizationBatchAcknowledgement,
) -> Result<StoredSynchronizationAcknowledgement, ReaderProtocolAcknowledgementError> {
    let translated = translate_protocol_synchronization_acknowledgement(acknowledgement)?;

    let stored = store_synchronization_acknowledgement(pool, &translated).await?;

    Ok(stored)
}

fn translate_entry(
    entry: &ProtocolAcknowledgementEntry,
) -> Result<SynchronizationAcknowledgementEntry, ReaderProtocolAcknowledgementError> {
    let resolution = match entry.outcome() {
        SynchronizationEntryOutcome::Acknowledged => SynchronizationEntryResolution::Acknowledged,

        SynchronizationEntryOutcome::RetryableFailure => {
            let failure_category = required_failure_category(entry)?;

            let retry_at_unix_milliseconds = entry.next_retry_at_unix_milliseconds().ok_or(
                ReaderProtocolAcknowledgementError::MissingRetryTime {
                    transaction_id: entry.transaction_id(),
                },
            )?;

            SynchronizationEntryResolution::RetryableFailure {
                failure_category,
                retry_at_unix_milliseconds,
            }
        }

        SynchronizationEntryOutcome::PermanentFailure => {
            SynchronizationEntryResolution::PermanentFailure {
                failure_category: required_failure_category(entry)?,
            }
        }

        SynchronizationEntryOutcome::ManualReview => SynchronizationEntryResolution::ManualReview {
            failure_category: required_failure_category(entry)?,
        },
    };

    Ok(SynchronizationAcknowledgementEntry::new(
        entry.transaction_id(),
        entry.local_sequence_number(),
        resolution,
    ))
}

fn required_failure_category(
    entry: &ProtocolAcknowledgementEntry,
) -> Result<String, ReaderProtocolAcknowledgementError> {
    entry
        .failure_category()
        .map(|category| category.as_str().to_owned())
        .ok_or(ReaderProtocolAcknowledgementError::MissingFailureCategory {
            transaction_id: entry.transaction_id(),
        })
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{
        DeviceProtocolVersion, ProtocolEnvironmentId,
        SynchronizationAcknowledgementEntry as ProtocolEntry, SynchronizationBatchAcknowledgement,
        SynchronizationBatchAcknowledgementDefinition, SynchronizationEntryOutcome,
        SynchronizationFailureCategory,
    };
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use crate::SynchronizationEntryResolution;

    use super::translate_protocol_synchronization_acknowledgement;

    const RECEIVED_AT: i64 = 1_700_000_000_000;

    fn sequence(value: u64) -> LocalSequenceNumber {
        match LocalSequenceNumber::new(value) {
            Ok(sequence) => sequence,
            Err(error) => {
                panic!("valid sequence failed: {error}")
            }
        }
    }

    fn environment() -> ProtocolEnvironmentId {
        match ProtocolEnvironmentId::new("development") {
            Ok(environment) => environment,
            Err(error) => {
                panic!("valid environment failed: {error}")
            }
        }
    }

    fn entry(
        transaction_id: FareTransactionId,
        local_sequence_number: LocalSequenceNumber,
        outcome: SynchronizationEntryOutcome,
        failure_category: Option<SynchronizationFailureCategory>,
        next_retry_at_unix_milliseconds: Option<i64>,
    ) -> ProtocolEntry {
        match ProtocolEntry::new(
            transaction_id,
            local_sequence_number,
            outcome,
            failure_category,
            next_retry_at_unix_milliseconds,
        ) {
            Ok(entry) => entry,
            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        }
    }

    #[test]
    fn protocol_outcomes_translate_to_durable_resolutions() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();

        let transactions = [
            FareTransactionId::generate(),
            FareTransactionId::generate(),
            FareTransactionId::generate(),
            FareTransactionId::generate(),
        ];

        let protocol_acknowledgement = match SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: DeviceProtocolVersion::CURRENT,
                environment_id: environment(),
                reader_id,
                batch_id,
                first_local_sequence_number: sequence(1),
                last_local_sequence_number: sequence(4),
                received_at_unix_milliseconds: RECEIVED_AT,
                replayed: false,
                entries: vec![
                    entry(
                        transactions[0],
                        sequence(1),
                        SynchronizationEntryOutcome::Acknowledged,
                        None,
                        None,
                    ),
                    entry(
                        transactions[1],
                        sequence(2),
                        SynchronizationEntryOutcome::RetryableFailure,
                        Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable),
                        Some(RECEIVED_AT + 1_000),
                    ),
                    entry(
                        transactions[2],
                        sequence(3),
                        SynchronizationEntryOutcome::PermanentFailure,
                        Some(SynchronizationFailureCategory::BackendValidationFailure),
                        None,
                    ),
                    entry(
                        transactions[3],
                        sequence(4),
                        SynchronizationEntryOutcome::ManualReview,
                        Some(SynchronizationFailureCategory::ManualReviewRequired),
                        None,
                    ),
                ],
            },
        ) {
            Ok(acknowledgement) => acknowledgement,
            Err(error) => {
                panic!(
                    "valid protocol acknowledgement failed: \
                         {error}"
                )
            }
        };

        let translated =
            match translate_protocol_synchronization_acknowledgement(&protocol_acknowledgement) {
                Ok(acknowledgement) => acknowledgement,
                Err(error) => {
                    panic!("acknowledgement translation failed: {error}")
                }
            };

        assert_eq!(translated.reader_id(), reader_id);
        assert_eq!(translated.batch_id(), batch_id);
        assert_eq!(translated.received_at_unix_milliseconds(), RECEIVED_AT);

        assert_eq!(
            translated.entries()[0].resolution(),
            &SynchronizationEntryResolution::Acknowledged
        );

        assert_eq!(
            translated.entries()[1].resolution(),
            &SynchronizationEntryResolution::RetryableFailure {
                failure_category: "backend_temporarily_unavailable".to_owned(),
                retry_at_unix_milliseconds: RECEIVED_AT + 1_000,
            }
        );

        assert_eq!(
            translated.entries()[2].resolution(),
            &SynchronizationEntryResolution::PermanentFailure {
                failure_category: "backend_validation_failure".to_owned(),
            }
        );

        assert_eq!(
            translated.entries()[3].resolution(),
            &SynchronizationEntryResolution::ManualReview {
                failure_category: "manual_review_required".to_owned(),
            }
        );
    }
}
