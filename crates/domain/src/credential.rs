use core::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FareCredentialId, TransitAccountId};

/// The project-owned representation of a fare credential.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareCredentialKind {
    /// A fictional physical card credential.
    Card,

    /// A fictional passenger mobile credential.
    Mobile,

    /// A development-only test credential.
    DevelopmentTestToken,
}

/// The lifecycle status of a fare credential.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareCredentialStatus {
    /// The credential has been created but is not yet usable.
    Pending,

    /// The credential may be used for fare processing.
    Active,

    /// The credential is temporarily unusable.
    Suspended,

    /// The credential was permanently invalidated.
    Revoked,

    /// The credential reached the end of its validity.
    Expired,

    /// The credential was superseded by another credential.
    Replaced,
}

impl FareCredentialStatus {
    /// Returns whether no later usable status is permitted.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Revoked | Self::Expired | Self::Replaced)
    }
}

impl fmt::Display for FareCredentialStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::Replaced => "replaced",
        };

        formatter.write_str(status)
    }
}

/// The documented reason a credential was revoked.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum RevocationReason {
    /// The simulated credential was reported lost.
    ReportedLost,

    /// The simulated credential was reported stolen.
    ReportedStolen,

    /// The credential was invalidated during replacement.
    Replaced,

    /// Its associated account was suspended.
    AccountSuspended,

    /// An authorized administrator revoked the credential.
    AdministrativeAction,

    /// The credential was involved in a security incident.
    SecurityIncident,

    /// The credential was removed during test cleanup.
    TestCleanup,
}

/// Errors produced by fare-credential lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareCredentialError {
    /// The requested lifecycle transition is prohibited.
    #[error("cannot transition fare credential from {from} to {to}")]
    InvalidStatusTransition {
        /// Status before the attempted transition.
        from: FareCredentialStatus,

        /// Requested status.
        to: FareCredentialStatus,
    },

    /// A credential cannot identify itself as its replacement.
    #[error("fare credential cannot replace itself")]
    SelfReplacement,

    /// A replaced credential already identifies a different successor.
    #[error("replacement already recorded as {existing}; cannot change it to {requested}")]
    ReplacementAlreadyRecorded {
        /// Existing replacement credential.
        existing: FareCredentialId,

        /// Newly requested replacement credential.
        requested: FareCredentialId,
    },
}

/// A project-owned credential associated with a transit account.
///
/// The aggregate preserves credential lifecycle invariants. Terminal
/// credentials cannot become usable again.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FareCredential {
    id: FareCredentialId,
    transit_account_id: TransitAccountId,
    kind: FareCredentialKind,
    status: FareCredentialStatus,
    revocation_reason: Option<RevocationReason>,
    replacement_id: Option<FareCredentialId>,
}

impl FareCredential {
    /// Creates a pending fare credential.
    #[must_use]
    pub const fn new_pending(
        id: FareCredentialId,
        transit_account_id: TransitAccountId,
        kind: FareCredentialKind,
    ) -> Self {
        Self {
            id,
            transit_account_id,
            kind,
            status: FareCredentialStatus::Pending,
            revocation_reason: None,
            replacement_id: None,
        }
    }

    /// Returns the credential identifier.
    #[must_use]
    pub const fn id(&self) -> FareCredentialId {
        self.id
    }

    /// Returns the associated transit-account identifier.
    #[must_use]
    pub const fn transit_account_id(&self) -> TransitAccountId {
        self.transit_account_id
    }

    /// Returns the credential representation.
    #[must_use]
    pub const fn kind(&self) -> FareCredentialKind {
        self.kind
    }

    /// Returns the current credential status.
    #[must_use]
    pub const fn status(&self) -> FareCredentialStatus {
        self.status
    }

    /// Returns the recorded revocation reason.
    #[must_use]
    pub const fn revocation_reason(&self) -> Option<RevocationReason> {
        self.revocation_reason
    }

    /// Returns the replacement credential identifier.
    #[must_use]
    pub const fn replacement_id(&self) -> Option<FareCredentialId> {
        self.replacement_id
    }

    /// Returns whether the credential is currently usable.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        matches!(self.status, FareCredentialStatus::Active)
    }

    /// Activates a pending or suspended credential.
    ///
    /// Activating an active credential is idempotent.
    pub fn activate(&mut self) -> Result<(), FareCredentialError> {
        match self.status {
            FareCredentialStatus::Pending | FareCredentialStatus::Suspended => {
                self.status = FareCredentialStatus::Active;
                Ok(())
            }
            FareCredentialStatus::Active => Ok(()),
            current => Err(FareCredentialError::InvalidStatusTransition {
                from: current,
                to: FareCredentialStatus::Active,
            }),
        }
    }

    /// Suspends an active credential.
    ///
    /// Suspending an already suspended credential is idempotent.
    pub fn suspend(&mut self) -> Result<(), FareCredentialError> {
        match self.status {
            FareCredentialStatus::Active => {
                self.status = FareCredentialStatus::Suspended;
                Ok(())
            }
            FareCredentialStatus::Suspended => Ok(()),
            current => Err(FareCredentialError::InvalidStatusTransition {
                from: current,
                to: FareCredentialStatus::Suspended,
            }),
        }
    }

    /// Reactivates a suspended credential.
    ///
    /// Reactivating an active credential is idempotent.
    pub fn reactivate(&mut self) -> Result<(), FareCredentialError> {
        self.activate()
    }

    /// Permanently revokes the credential.
    ///
    /// Repeating revocation is idempotent and preserves the original reason.
    pub fn revoke(&mut self, reason: RevocationReason) -> Result<(), FareCredentialError> {
        match self.status {
            FareCredentialStatus::Revoked => Ok(()),
            FareCredentialStatus::Expired | FareCredentialStatus::Replaced => {
                Err(FareCredentialError::InvalidStatusTransition {
                    from: self.status,
                    to: FareCredentialStatus::Revoked,
                })
            }
            FareCredentialStatus::Pending
            | FareCredentialStatus::Active
            | FareCredentialStatus::Suspended => {
                self.status = FareCredentialStatus::Revoked;
                self.revocation_reason = Some(reason);
                self.replacement_id = None;
                Ok(())
            }
        }
    }

    /// Permanently marks the credential as expired.
    ///
    /// Expiring an already expired credential is idempotent.
    pub fn expire(&mut self) -> Result<(), FareCredentialError> {
        match self.status {
            FareCredentialStatus::Expired => Ok(()),
            FareCredentialStatus::Revoked | FareCredentialStatus::Replaced => {
                Err(FareCredentialError::InvalidStatusTransition {
                    from: self.status,
                    to: FareCredentialStatus::Expired,
                })
            }
            FareCredentialStatus::Pending
            | FareCredentialStatus::Active
            | FareCredentialStatus::Suspended => {
                self.status = FareCredentialStatus::Expired;
                self.revocation_reason = None;
                self.replacement_id = None;
                Ok(())
            }
        }
    }

    /// Permanently records a successor credential.
    ///
    /// Repeating the operation with the same replacement is idempotent.
    pub fn replace_with(
        &mut self,
        replacement_id: FareCredentialId,
    ) -> Result<(), FareCredentialError> {
        if replacement_id == self.id {
            return Err(FareCredentialError::SelfReplacement);
        }

        match self.status {
            FareCredentialStatus::Active | FareCredentialStatus::Suspended => {
                self.status = FareCredentialStatus::Replaced;
                self.revocation_reason = None;
                self.replacement_id = Some(replacement_id);
                Ok(())
            }
            FareCredentialStatus::Replaced => match self.replacement_id {
                Some(existing) if existing == replacement_id => Ok(()),
                Some(existing) => Err(FareCredentialError::ReplacementAlreadyRecorded {
                    existing,
                    requested: replacement_id,
                }),
                None => Err(FareCredentialError::InvalidStatusTransition {
                    from: FareCredentialStatus::Replaced,
                    to: FareCredentialStatus::Replaced,
                }),
            },
            current => Err(FareCredentialError::InvalidStatusTransition {
                from: current,
                to: FareCredentialStatus::Replaced,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{FareCredentialId, TransitAccountId};

    use super::{
        FareCredential, FareCredentialError, FareCredentialKind, FareCredentialStatus,
        RevocationReason,
    };

    fn pending_credential() -> FareCredential {
        FareCredential::new_pending(
            FareCredentialId::generate(),
            TransitAccountId::generate(),
            FareCredentialKind::Card,
        )
    }

    fn active_credential() -> FareCredential {
        let mut credential = pending_credential();
        let result = credential.activate();

        assert!(result.is_ok());
        credential
    }

    #[test]
    fn new_credential_is_pending_and_unusable() {
        let credential = pending_credential();

        assert_eq!(credential.status(), FareCredentialStatus::Pending);
        assert!(!credential.is_usable());
        assert_eq!(credential.revocation_reason(), None);
        assert_eq!(credential.replacement_id(), None);
    }

    #[test]
    fn pending_credential_can_be_activated() {
        let mut credential = pending_credential();

        let result = credential.activate();

        assert!(result.is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Active);
        assert!(credential.is_usable());
    }

    #[test]
    fn activation_is_idempotent() {
        let mut credential = active_credential();

        let result = credential.activate();

        assert!(result.is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Active);
    }

    #[test]
    fn pending_credential_cannot_be_suspended() {
        let mut credential = pending_credential();

        let result = credential.suspend();

        assert!(matches!(
            result,
            Err(FareCredentialError::InvalidStatusTransition {
                from: FareCredentialStatus::Pending,
                to: FareCredentialStatus::Suspended
            })
        ));
    }

    #[test]
    fn active_credential_can_be_suspended_and_reactivated() {
        let mut credential = active_credential();

        assert!(credential.suspend().is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Suspended);
        assert!(!credential.is_usable());

        assert!(credential.reactivate().is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Active);
        assert!(credential.is_usable());
    }

    #[test]
    fn active_credential_can_be_revoked() {
        let mut credential = active_credential();

        let result = credential.revoke(RevocationReason::ReportedLost);

        assert!(result.is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Revoked);
        assert_eq!(
            credential.revocation_reason(),
            Some(RevocationReason::ReportedLost)
        );
        assert!(!credential.is_usable());
    }

    #[test]
    fn repeated_revocation_preserves_original_reason() {
        let mut credential = active_credential();

        assert!(credential.revoke(RevocationReason::ReportedLost).is_ok());

        let result = credential.revoke(RevocationReason::AdministrativeAction);

        assert!(result.is_ok());
        assert_eq!(
            credential.revocation_reason(),
            Some(RevocationReason::ReportedLost)
        );
    }

    #[test]
    fn revoked_credential_cannot_be_reactivated() {
        let mut credential = active_credential();

        assert!(
            credential
                .revoke(RevocationReason::SecurityIncident)
                .is_ok()
        );

        let result = credential.reactivate();

        assert!(matches!(
            result,
            Err(FareCredentialError::InvalidStatusTransition {
                from: FareCredentialStatus::Revoked,
                to: FareCredentialStatus::Active
            })
        ));
    }

    #[test]
    fn active_credential_can_expire() {
        let mut credential = active_credential();

        let result = credential.expire();

        assert!(result.is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Expired);
        assert!(credential.status().is_terminal());
        assert!(!credential.is_usable());
    }

    #[test]
    fn expired_credential_cannot_be_activated() {
        let mut credential = active_credential();

        assert!(credential.expire().is_ok());

        let result = credential.activate();

        assert!(matches!(
            result,
            Err(FareCredentialError::InvalidStatusTransition {
                from: FareCredentialStatus::Expired,
                to: FareCredentialStatus::Active
            })
        ));
    }

    #[test]
    fn active_credential_can_be_replaced() {
        let mut credential = active_credential();
        let replacement_id = FareCredentialId::generate();

        let result = credential.replace_with(replacement_id);

        assert!(result.is_ok());
        assert_eq!(credential.status(), FareCredentialStatus::Replaced);
        assert_eq!(credential.replacement_id(), Some(replacement_id));
        assert!(!credential.is_usable());
    }

    #[test]
    fn credential_cannot_replace_itself() {
        let mut credential = active_credential();

        let result = credential.replace_with(credential.id());

        assert!(matches!(result, Err(FareCredentialError::SelfReplacement)));
    }

    #[test]
    fn repeated_replacement_with_same_identifier_is_idempotent() {
        let mut credential = active_credential();
        let replacement_id = FareCredentialId::generate();

        assert!(credential.replace_with(replacement_id).is_ok());

        let result = credential.replace_with(replacement_id);

        assert!(result.is_ok());
        assert_eq!(credential.replacement_id(), Some(replacement_id));
    }

    #[test]
    fn recorded_replacement_cannot_be_changed() {
        let mut credential = active_credential();
        let original = FareCredentialId::generate();
        let requested = FareCredentialId::generate();

        assert!(credential.replace_with(original).is_ok());

        let result = credential.replace_with(requested);

        assert!(matches!(
            result,
            Err(
                FareCredentialError::
                    ReplacementAlreadyRecorded {
                        existing,
                        requested: attempted
                    }
            ) if existing == original && attempted == requested
        ));
    }

    #[test]
    fn pending_credential_cannot_be_replaced() {
        let mut credential = pending_credential();

        let result = credential.replace_with(FareCredentialId::generate());

        assert!(matches!(
            result,
            Err(FareCredentialError::InvalidStatusTransition {
                from: FareCredentialStatus::Pending,
                to: FareCredentialStatus::Replaced
            })
        ));
    }

    #[test]
    fn all_terminal_statuses_are_identified() {
        assert!(FareCredentialStatus::Revoked.is_terminal());
        assert!(FareCredentialStatus::Expired.is_terminal());
        assert!(FareCredentialStatus::Replaced.is_terminal());
        assert!(!FareCredentialStatus::Pending.is_terminal());
        assert!(!FareCredentialStatus::Active.is_terminal());
        assert!(!FareCredentialStatus::Suspended.is_terminal());
    }
}
