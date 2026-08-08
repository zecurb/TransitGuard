use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{FareTransactionId, ReaderId};

use crate::{
    ReconciliationId, ReconciliationOutcome, ReconciliationRecord, ReconciliationStatus,
    ReconciliationTime,
};

/// Stable one-to-one identity for a discrepancy associated with a reconciliation.
///
/// The underlying reconciliation identity is reused intentionally so retrying
/// discrepancy creation cannot create a second business identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DiscrepancyCaseId(ReconciliationId);

impl DiscrepancyCaseId {
    /// Derives the stable discrepancy identity from its reconciliation.
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

/// Stable discrepancy classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiscrepancyCategory {
    /// Reader and backend approved different amounts.
    FareAmountMismatch,

    /// Reader and backend used different policy identities or versions.
    PolicyVersionMismatch,

    /// Eligibility evidence differs.
    EligibilityMismatch,

    /// Transit-product evidence differs.
    ProductMismatch,

    /// Transfer evidence differs.
    TransferMismatch,

    /// Fare-cap evidence differs.
    FareCapMismatch,

    /// Duplicate source transaction activity was identified.
    DuplicateTransaction,

    /// Required historical backend context is unavailable.
    MissingBackendContext,

    /// Evidence violated reconciliation invariants.
    InvalidEvidence,

    /// The comparison requires human investigation.
    ManualReviewRequired,
}

impl TryFrom<ReconciliationOutcome> for DiscrepancyCategory {
    type Error = DiscrepancyCaseError;

    fn try_from(outcome: ReconciliationOutcome) -> Result<Self, Self::Error> {
        match outcome {
            ReconciliationOutcome::Matched => Err(DiscrepancyCaseError::MatchedReconciliation),

            ReconciliationOutcome::FareAmountMismatch => Ok(Self::FareAmountMismatch),

            ReconciliationOutcome::PolicyVersionMismatch => Ok(Self::PolicyVersionMismatch),

            ReconciliationOutcome::EligibilityMismatch => Ok(Self::EligibilityMismatch),

            ReconciliationOutcome::ProductMismatch => Ok(Self::ProductMismatch),

            ReconciliationOutcome::TransferMismatch => Ok(Self::TransferMismatch),

            ReconciliationOutcome::FareCapMismatch => Ok(Self::FareCapMismatch),

            ReconciliationOutcome::DuplicateTransaction => Ok(Self::DuplicateTransaction),

            ReconciliationOutcome::MissingBackendContext => Ok(Self::MissingBackendContext),

            ReconciliationOutcome::InvalidEvidence => Ok(Self::InvalidEvidence),

            ReconciliationOutcome::ManualReviewRequired => Ok(Self::ManualReviewRequired),
        }
    }
}

/// Durable discrepancy-case lifecycle state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiscrepancyState {
    /// Discrepancy is open and awaiting ordinary resolution.
    Open,

    /// Discrepancy requires explicit human review.
    ManualReview,

    /// Discrepancy was resolved.
    Resolved,

    /// Discrepancy was intentionally dismissed.
    Dismissed,
}

impl DiscrepancyState {
    /// Reports whether no further state transition is permitted.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Resolved | Self::Dismissed)
    }
}

/// Identity of the project-owned actor resolving a discrepancy.
///
/// Phase 8 does not yet implement an operator-account subsystem, so this
/// identifier is intentionally opaque and does not contain names, email
/// addresses, credentials, or other personal information.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ResolutionActorId(ReconciliationId);

impl ResolutionActorId {
    /// Generates a project-owned actor reference.
    #[must_use]
    pub fn generate() -> Self {
        Self(ReconciliationId::generate())
    }
}

/// Stable final action performed on a discrepancy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiscrepancyResolutionAction {
    /// Investigation resolved the discrepancy.
    Resolve,

    /// Investigation determined no further action is required.
    Dismiss,
}

/// Stable resolution reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DiscrepancyResolutionReason {
    /// Reader evidence was accepted after investigation.
    ReaderEvidenceConfirmed,

    /// Backend evidence was accepted after investigation.
    BackendEvidenceConfirmed,

    /// A documented policy exception explains the discrepancy.
    PolicyExceptionApproved,

    /// Duplicate activity was confirmed.
    DuplicateConfirmed,

    /// The discrepancy originated from fictional development/test data.
    TestDataCorrection,

    /// Investigation determined there is no monetary impact.
    NoFinancialImpact,
}

/// Immutable final resolution metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DiscrepancyResolution {
    actor_id: ResolutionActorId,
    action: DiscrepancyResolutionAction,
    reason: DiscrepancyResolutionReason,
    resolved_at: ReconciliationTime,
}

impl DiscrepancyResolution {
    /// Returns the resolver reference.
    #[must_use]
    pub const fn actor_id(self) -> ResolutionActorId {
        self.actor_id
    }

    /// Returns the final action.
    #[must_use]
    pub const fn action(self) -> DiscrepancyResolutionAction {
        self.action
    }

    /// Returns the stable reason.
    #[must_use]
    pub const fn reason(self) -> DiscrepancyResolutionReason {
        self.reason
    }

    /// Returns when the action occurred.
    #[must_use]
    pub const fn resolved_at(self) -> ReconciliationTime {
        self.resolved_at
    }
}

/// Immutable lifecycle transition retained for audit history.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DiscrepancyStatusTransition {
    from: DiscrepancyState,
    to: DiscrepancyState,
    actor_id: ResolutionActorId,
    action: DiscrepancyResolutionAction,
    reason: DiscrepancyResolutionReason,
    occurred_at: ReconciliationTime,
}

impl DiscrepancyStatusTransition {
    /// Returns the prior state.
    #[must_use]
    pub const fn from(self) -> DiscrepancyState {
        self.from
    }

    /// Returns the new state.
    #[must_use]
    pub const fn to(self) -> DiscrepancyState {
        self.to
    }

    /// Returns when the transition occurred.
    #[must_use]
    pub const fn occurred_at(self) -> ReconciliationTime {
        self.occurred_at
    }
}

/// Errors produced by discrepancy lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DiscrepancyCaseError {
    /// Matched reconciliations cannot create discrepancy cases.
    #[error("matched reconciliation cannot create a discrepancy case")]
    MatchedReconciliation,

    /// Resolution time cannot precede discrepancy creation.
    #[error("discrepancy resolution cannot precede case creation")]
    ResolutionBeforeCreation,

    /// A finalized discrepancy cannot be changed to a different resolution.
    #[error("discrepancy is already finalized with state {state:?}")]
    AlreadyFinalized {
        /// Existing terminal state.
        state: DiscrepancyState,
    },
}

/// Auditable discrepancy generated from an unmatched reconciliation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiscrepancyCase {
    id: DiscrepancyCaseId,
    reconciliation_id: ReconciliationId,
    transaction_id: FareTransactionId,
    reader_id: ReaderId,
    category: DiscrepancyCategory,
    state: DiscrepancyState,
    created_at: ReconciliationTime,
    resolution: Option<DiscrepancyResolution>,
    history: Vec<DiscrepancyStatusTransition>,
}

impl DiscrepancyCase {
    /// Creates the stable discrepancy associated with a reconciliation.
    pub fn from_reconciliation(
        reconciliation: ReconciliationRecord,
    ) -> Result<Self, DiscrepancyCaseError> {
        let category = DiscrepancyCategory::try_from(reconciliation.outcome())?;

        let state = match reconciliation.status() {
            ReconciliationStatus::Matched => {
                return Err(DiscrepancyCaseError::MatchedReconciliation);
            }

            ReconciliationStatus::Discrepancy => DiscrepancyState::Open,

            ReconciliationStatus::ManualReview => DiscrepancyState::ManualReview,
        };

        Ok(Self {
            id: DiscrepancyCaseId::for_reconciliation(reconciliation.id()),
            reconciliation_id: reconciliation.id(),
            transaction_id: reconciliation.transaction_id(),
            reader_id: reconciliation.reader_id(),
            category,
            state,
            created_at: reconciliation.reconciled_at(),
            resolution: None,
            history: Vec::new(),
        })
    }

    /// Returns the stable discrepancy identity.
    #[must_use]
    pub const fn id(&self) -> DiscrepancyCaseId {
        self.id
    }

    /// Returns the source reconciliation identity.
    #[must_use]
    pub const fn reconciliation_id(&self) -> ReconciliationId {
        self.reconciliation_id
    }

    /// Returns the transaction under investigation.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the originating reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the stable discrepancy category.
    #[must_use]
    pub const fn category(&self) -> DiscrepancyCategory {
        self.category
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> DiscrepancyState {
        self.state
    }

    /// Returns final resolution metadata, when finalized.
    #[must_use]
    pub const fn resolution(&self) -> Option<DiscrepancyResolution> {
        self.resolution
    }

    /// Returns immutable status-transition history.
    #[must_use]
    pub fn history(&self) -> &[DiscrepancyStatusTransition] {
        &self.history
    }

    /// Resolves the discrepancy.
    pub fn resolve(
        &mut self,
        actor_id: ResolutionActorId,
        reason: DiscrepancyResolutionReason,
        resolved_at: ReconciliationTime,
    ) -> Result<(), DiscrepancyCaseError> {
        self.finalize(
            actor_id,
            DiscrepancyResolutionAction::Resolve,
            reason,
            resolved_at,
            DiscrepancyState::Resolved,
        )
    }

    /// Dismisses the discrepancy.
    pub fn dismiss(
        &mut self,
        actor_id: ResolutionActorId,
        reason: DiscrepancyResolutionReason,
        resolved_at: ReconciliationTime,
    ) -> Result<(), DiscrepancyCaseError> {
        self.finalize(
            actor_id,
            DiscrepancyResolutionAction::Dismiss,
            reason,
            resolved_at,
            DiscrepancyState::Dismissed,
        )
    }

    fn finalize(
        &mut self,
        actor_id: ResolutionActorId,
        action: DiscrepancyResolutionAction,
        reason: DiscrepancyResolutionReason,
        resolved_at: ReconciliationTime,
        target: DiscrepancyState,
    ) -> Result<(), DiscrepancyCaseError> {
        if resolved_at < self.created_at {
            return Err(DiscrepancyCaseError::ResolutionBeforeCreation);
        }

        let requested_resolution = DiscrepancyResolution {
            actor_id,
            action,
            reason,
            resolved_at,
        };

        if self.state.is_final() {
            if self.state == target && self.resolution == Some(requested_resolution) {
                return Ok(());
            }

            return Err(DiscrepancyCaseError::AlreadyFinalized { state: self.state });
        }

        let previous = self.state;

        self.state = target;
        self.resolution = Some(requested_resolution);

        self.history.push(DiscrepancyStatusTransition {
            from: previous,
            to: target,
            actor_id,
            action,
            reason,
            occurred_at: resolved_at,
        });

        Ok(())
    }
}
