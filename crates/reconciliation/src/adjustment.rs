use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{FareTransactionId, Money};

use crate::{ReconciliationId, ReconciliationOutcome, ReconciliationRecord, ReconciliationTime};

/// Stable one-to-one identity for a proposed adjustment.
///
/// The reconciliation identity is intentionally reused as the underlying
/// business key. Retrying proposal creation for the same reconciliation
/// therefore cannot invent a second adjustment identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProposedAdjustmentId(ReconciliationId);

impl ProposedAdjustmentId {
    /// Derives the adjustment identity from a reconciliation.
    #[must_use]
    pub const fn for_reconciliation(reconciliation_id: ReconciliationId) -> Self {
        Self(reconciliation_id)
    }

    /// Returns the source reconciliation identity.
    #[must_use]
    pub const fn reconciliation_id(self) -> ReconciliationId {
        self.0
    }
}

/// Direction of the proposed correction.
///
/// These values describe project-owned ledger intent only. They do not trigger
/// real payment capture, refunding, banking, or external settlement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ProposedAdjustmentDirection {
    /// Backend evidence indicates the recorded fare should increase.
    IncreaseRecordedFare,

    /// Backend evidence indicates the recorded fare should decrease.
    DecreaseRecordedFare,
}

/// Errors produced while creating proposed adjustments.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProposedAdjustmentError {
    /// Only fare-amount mismatches may automatically propose a correction.
    #[error("reconciliation outcome {outcome:?} cannot create a proposed adjustment")]
    UnsupportedOutcome {
        /// Non-adjustable reconciliation outcome.
        outcome: ReconciliationOutcome,
    },

    /// Fare mismatch did not contain an expected monetary difference.
    #[error("fare mismatch is missing its monetary difference")]
    MissingDifference,

    /// A zero-value correction is not a meaningful adjustment.
    #[error("proposed adjustment cannot be zero")]
    ZeroAdjustment,
}

/// Immutable proposed financial correction.
///
/// This object records intent only. It cannot perform an external charge,
/// refund, bank transfer, card-network settlement, or accounting-system post.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProposedAdjustment {
    id: ProposedAdjustmentId,
    reconciliation_id: ReconciliationId,
    transaction_id: FareTransactionId,
    correction_amount: Money,
    direction: ProposedAdjustmentDirection,
    created_at: ReconciliationTime,
}

impl ProposedAdjustment {
    /// Creates the deterministic proposal associated with a fare mismatch.
    pub fn from_reconciliation(
        reconciliation: ReconciliationRecord,
    ) -> Result<Self, ProposedAdjustmentError> {
        if reconciliation.outcome() != ReconciliationOutcome::FareAmountMismatch {
            return Err(ProposedAdjustmentError::UnsupportedOutcome {
                outcome: reconciliation.outcome(),
            });
        }

        let correction_amount = reconciliation
            .monetary_difference()
            .ok_or(ProposedAdjustmentError::MissingDifference)?;

        if correction_amount.is_zero() {
            return Err(ProposedAdjustmentError::ZeroAdjustment);
        }

        let direction = if correction_amount.is_positive() {
            ProposedAdjustmentDirection::IncreaseRecordedFare
        } else {
            ProposedAdjustmentDirection::DecreaseRecordedFare
        };

        Ok(Self {
            id: ProposedAdjustmentId::for_reconciliation(reconciliation.id()),
            reconciliation_id: reconciliation.id(),
            transaction_id: reconciliation.transaction_id(),
            correction_amount,
            direction,
            created_at: reconciliation.reconciled_at(),
        })
    }

    /// Returns the stable adjustment identity.
    #[must_use]
    pub const fn id(self) -> ProposedAdjustmentId {
        self.id
    }

    /// Returns the source reconciliation identity.
    #[must_use]
    pub const fn reconciliation_id(self) -> ReconciliationId {
        self.reconciliation_id
    }

    /// Returns the affected transaction identity.
    #[must_use]
    pub const fn transaction_id(self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns `expected - observed`.
    ///
    /// Positive means the reader undercharged. Negative means the reader
    /// overcharged.
    #[must_use]
    pub const fn correction_amount(self) -> Money {
        self.correction_amount
    }

    /// Returns the proposed ledger direction.
    #[must_use]
    pub const fn direction(self) -> ProposedAdjustmentDirection {
        self.direction
    }

    /// Returns when the proposal was created.
    #[must_use]
    pub const fn created_at(self) -> ReconciliationTime {
        self.created_at
    }
}
