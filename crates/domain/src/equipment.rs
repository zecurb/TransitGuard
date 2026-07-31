use core::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EquipmentKeyId, ReaderId};

/// The backend's current understanding of reader equipment state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderEquipmentStatus {
    /// The reader has been created but has not completed registration.
    PendingRegistration,

    /// The reader is registered and communicating normally.
    Active,

    /// The reader is registered but currently operating offline.
    Offline,

    /// The reader is temporarily prohibited from normal operation.
    Disabled,

    /// The reader's equipment identity was permanently invalidated.
    Revoked,

    /// The reader was permanently removed from service.
    Decommissioned,
}

impl ReaderEquipmentStatus {
    /// Returns whether the status permanently prevents normal operation.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Revoked | Self::Decommissioned)
    }
}

impl fmt::Display for ReaderEquipmentStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            Self::PendingRegistration => "pending registration",
            Self::Active => "active",
            Self::Offline => "offline",
            Self::Disabled => "disabled",
            Self::Revoked => "revoked",
            Self::Decommissioned => "decommissioned",
        };

        formatter.write_str(status)
    }
}

/// The documented reason reader equipment was disabled.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderDisablementReason {
    /// The equipment may have been compromised.
    SuspectedCompromise,

    /// The simulated equipment was reported lost.
    LostEquipment,

    /// The reader is using an invalid configuration.
    InvalidConfiguration,

    /// An authorized administrator disabled the reader.
    AdministrativeAction,

    /// The reader was disabled during test cleanup.
    TestCleanup,
}

/// The documented reason an equipment identity was revoked.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderRevocationReason {
    /// The equipment may have been compromised.
    SuspectedCompromise,

    /// Equipment authentication material may have been exposed.
    CredentialExposure,

    /// An authorized administrator revoked the equipment identity.
    AdministrativeAction,

    /// The reader was involved in a security incident.
    SecurityIncident,

    /// The identity was revoked during test cleanup.
    TestCleanup,
}

/// Public identity metadata for project-owned reader equipment.
///
/// This type identifies the reader and the project-owned verification key.
/// It never contains a private key or another authentication secret.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct EquipmentIdentity {
    reader_id: ReaderId,
    key_id: EquipmentKeyId,
}

impl EquipmentIdentity {
    /// Creates equipment identity metadata.
    #[must_use]
    pub const fn new(reader_id: ReaderId, key_id: EquipmentKeyId) -> Self {
        Self { reader_id, key_id }
    }

    /// Returns the associated reader identifier.
    #[must_use]
    pub const fn reader_id(self) -> ReaderId {
        self.reader_id
    }

    /// Returns the public equipment-key identifier.
    #[must_use]
    pub const fn key_id(self) -> EquipmentKeyId {
        self.key_id
    }

    fn replace_key(&mut self, key_id: EquipmentKeyId) {
        self.key_id = key_id;
    }
}

/// Errors produced by reader-equipment operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReaderEquipmentError {
    /// The requested lifecycle transition is prohibited.
    #[error("cannot transition reader equipment from {from} to {to}")]
    InvalidStatusTransition {
        /// Status before the attempted transition.
        from: ReaderEquipmentStatus,

        /// Requested status.
        to: ReaderEquipmentStatus,
    },

    /// Terminal equipment cannot receive identity changes.
    #[error("reader equipment in terminal status {status} cannot be modified")]
    TerminalEquipment {
        /// Current terminal status.
        status: ReaderEquipmentStatus,
    },
}

/// Project-owned simulated reader equipment.
///
/// This aggregate controls reader registration, operational status,
/// disablement, equipment-identity revocation, and decommissioning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReaderEquipment {
    id: ReaderId,
    identity: EquipmentIdentity,
    status: ReaderEquipmentStatus,
    disablement_reason: Option<ReaderDisablementReason>,
    revocation_reason: Option<ReaderRevocationReason>,
}

impl ReaderEquipment {
    /// Creates reader equipment awaiting registration.
    #[must_use]
    pub const fn new_pending(id: ReaderId, key_id: EquipmentKeyId) -> Self {
        Self {
            id,
            identity: EquipmentIdentity::new(id, key_id),
            status: ReaderEquipmentStatus::PendingRegistration,
            disablement_reason: None,
            revocation_reason: None,
        }
    }

    /// Returns the reader identifier.
    #[must_use]
    pub const fn id(&self) -> ReaderId {
        self.id
    }

    /// Returns public equipment identity metadata.
    #[must_use]
    pub const fn identity(&self) -> EquipmentIdentity {
        self.identity
    }

    /// Returns the current equipment status.
    #[must_use]
    pub const fn status(&self) -> ReaderEquipmentStatus {
        self.status
    }

    /// Returns the recorded disablement reason.
    #[must_use]
    pub const fn disablement_reason(&self) -> Option<ReaderDisablementReason> {
        self.disablement_reason
    }

    /// Returns the recorded identity-revocation reason.
    #[must_use]
    pub const fn revocation_reason(&self) -> Option<ReaderRevocationReason> {
        self.revocation_reason
    }

    /// Returns whether the reader may accept fictional fare presentations.
    ///
    /// Offline readers may continue bounded provisional processing.
    #[must_use]
    pub const fn can_accept_fare_presentations(&self) -> bool {
        matches!(
            self.status,
            ReaderEquipmentStatus::Active | ReaderEquipmentStatus::Offline
        )
    }

    /// Returns whether the reader may authenticate to the backend.
    #[must_use]
    pub const fn may_authenticate_to_backend(&self) -> bool {
        matches!(
            self.status,
            ReaderEquipmentStatus::Active | ReaderEquipmentStatus::Offline
        )
    }

    /// Completes registration and activates pending equipment.
    ///
    /// Activating already active equipment is idempotent.
    pub fn activate(&mut self) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::PendingRegistration => {
                self.status = ReaderEquipmentStatus::Active;
                Ok(())
            }
            ReaderEquipmentStatus::Active => Ok(()),
            current => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: current,
                to: ReaderEquipmentStatus::Active,
            }),
        }
    }

    /// Records that an active reader is operating offline.
    ///
    /// Repeating the offline transition is idempotent.
    pub fn mark_offline(&mut self) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::Active => {
                self.status = ReaderEquipmentStatus::Offline;
                Ok(())
            }
            ReaderEquipmentStatus::Offline => Ok(()),
            current => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: current,
                to: ReaderEquipmentStatus::Offline,
            }),
        }
    }

    /// Records restored connectivity for an offline reader.
    ///
    /// Marking already active equipment online is idempotent.
    pub fn mark_online(&mut self) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::Offline => {
                self.status = ReaderEquipmentStatus::Active;
                Ok(())
            }
            ReaderEquipmentStatus::Active => Ok(()),
            current => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: current,
                to: ReaderEquipmentStatus::Active,
            }),
        }
    }

    /// Temporarily disables reader equipment.
    ///
    /// Repeated disablement is idempotent and preserves the first reason.
    pub fn disable(&mut self, reason: ReaderDisablementReason) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::PendingRegistration
            | ReaderEquipmentStatus::Active
            | ReaderEquipmentStatus::Offline => {
                self.status = ReaderEquipmentStatus::Disabled;
                self.disablement_reason = Some(reason);
                Ok(())
            }
            ReaderEquipmentStatus::Disabled => Ok(()),
            current => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: current,
                to: ReaderEquipmentStatus::Disabled,
            }),
        }
    }

    /// Re-enables disabled reader equipment.
    ///
    /// Re-enabling active equipment is idempotent.
    pub fn enable(&mut self) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::Disabled => {
                self.status = ReaderEquipmentStatus::Active;
                self.disablement_reason = None;
                Ok(())
            }
            ReaderEquipmentStatus::Active => Ok(()),
            current => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: current,
                to: ReaderEquipmentStatus::Active,
            }),
        }
    }

    /// Permanently revokes the equipment identity.
    ///
    /// Repeated revocation is idempotent and preserves the first reason.
    pub fn revoke(&mut self, reason: ReaderRevocationReason) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::PendingRegistration
            | ReaderEquipmentStatus::Active
            | ReaderEquipmentStatus::Offline
            | ReaderEquipmentStatus::Disabled => {
                self.status = ReaderEquipmentStatus::Revoked;
                self.disablement_reason = None;
                self.revocation_reason = Some(reason);
                Ok(())
            }
            ReaderEquipmentStatus::Revoked => Ok(()),
            ReaderEquipmentStatus::Decommissioned => {
                Err(ReaderEquipmentError::InvalidStatusTransition {
                    from: ReaderEquipmentStatus::Decommissioned,
                    to: ReaderEquipmentStatus::Revoked,
                })
            }
        }
    }

    /// Permanently removes the reader from service.
    ///
    /// Repeated decommissioning is idempotent.
    pub fn decommission(&mut self) -> Result<(), ReaderEquipmentError> {
        match self.status {
            ReaderEquipmentStatus::PendingRegistration
            | ReaderEquipmentStatus::Active
            | ReaderEquipmentStatus::Offline
            | ReaderEquipmentStatus::Disabled => {
                self.status = ReaderEquipmentStatus::Decommissioned;
                self.disablement_reason = None;
                self.revocation_reason = None;
                Ok(())
            }
            ReaderEquipmentStatus::Decommissioned => Ok(()),
            ReaderEquipmentStatus::Revoked => Err(ReaderEquipmentError::InvalidStatusTransition {
                from: ReaderEquipmentStatus::Revoked,
                to: ReaderEquipmentStatus::Decommissioned,
            }),
        }
    }

    /// Replaces the public identifier of the equipment key.
    ///
    /// Key rotation is permitted before terminal revocation or
    /// decommissioning. No private key material is stored here.
    pub fn rotate_equipment_key(
        &mut self,
        key_id: EquipmentKeyId,
    ) -> Result<(), ReaderEquipmentError> {
        if self.status.is_terminal() {
            return Err(ReaderEquipmentError::TerminalEquipment {
                status: self.status,
            });
        }

        self.identity.replace_key(key_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{EquipmentKeyId, ReaderId};

    use super::{
        ReaderDisablementReason, ReaderEquipment, ReaderEquipmentError, ReaderEquipmentStatus,
        ReaderRevocationReason,
    };

    fn pending_reader() -> ReaderEquipment {
        ReaderEquipment::new_pending(ReaderId::generate(), EquipmentKeyId::generate())
    }

    fn active_reader() -> ReaderEquipment {
        let mut reader = pending_reader();
        let result = reader.activate();

        assert!(result.is_ok());
        reader
    }

    #[test]
    fn new_reader_is_pending_registration() {
        let reader = pending_reader();

        assert_eq!(reader.status(), ReaderEquipmentStatus::PendingRegistration);
        assert_eq!(reader.identity().reader_id(), reader.id());
        assert!(!reader.can_accept_fare_presentations());
        assert!(!reader.may_authenticate_to_backend());
    }

    #[test]
    fn pending_reader_can_be_activated() {
        let mut reader = pending_reader();

        let result = reader.activate();

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Active);
        assert!(reader.can_accept_fare_presentations());
        assert!(reader.may_authenticate_to_backend());
    }

    #[test]
    fn activation_is_idempotent() {
        let mut reader = active_reader();

        let result = reader.activate();

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Active);
    }

    #[test]
    fn active_reader_can_move_offline_and_online() {
        let mut reader = active_reader();

        assert!(reader.mark_offline().is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Offline);
        assert!(reader.can_accept_fare_presentations());

        assert!(reader.mark_online().is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Active);
    }

    #[test]
    fn pending_reader_cannot_be_marked_offline() {
        let mut reader = pending_reader();

        let result = reader.mark_offline();

        assert!(matches!(
            result,
            Err(ReaderEquipmentError::InvalidStatusTransition {
                from: ReaderEquipmentStatus::PendingRegistration,
                to: ReaderEquipmentStatus::Offline
            })
        ));
    }

    #[test]
    fn active_reader_can_be_disabled() {
        let mut reader = active_reader();

        let result = reader.disable(ReaderDisablementReason::InvalidConfiguration);

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Disabled);
        assert_eq!(
            reader.disablement_reason(),
            Some(ReaderDisablementReason::InvalidConfiguration)
        );
        assert!(!reader.can_accept_fare_presentations());
    }

    #[test]
    fn repeated_disablement_preserves_original_reason() {
        let mut reader = active_reader();

        assert!(
            reader
                .disable(ReaderDisablementReason::InvalidConfiguration)
                .is_ok()
        );

        let result = reader.disable(ReaderDisablementReason::AdministrativeAction);

        assert!(result.is_ok());
        assert_eq!(
            reader.disablement_reason(),
            Some(ReaderDisablementReason::InvalidConfiguration)
        );
    }

    #[test]
    fn disabled_reader_can_be_enabled() {
        let mut reader = active_reader();

        assert!(
            reader
                .disable(ReaderDisablementReason::AdministrativeAction)
                .is_ok()
        );

        let result = reader.enable();

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Active);
        assert_eq!(reader.disablement_reason(), None);
    }

    #[test]
    fn reader_identity_can_be_revoked() {
        let mut reader = active_reader();

        let result = reader.revoke(ReaderRevocationReason::CredentialExposure);

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Revoked);
        assert_eq!(
            reader.revocation_reason(),
            Some(ReaderRevocationReason::CredentialExposure)
        );
        assert!(!reader.may_authenticate_to_backend());
    }

    #[test]
    fn repeated_revocation_preserves_original_reason() {
        let mut reader = active_reader();

        assert!(
            reader
                .revoke(ReaderRevocationReason::CredentialExposure)
                .is_ok()
        );

        let result = reader.revoke(ReaderRevocationReason::AdministrativeAction);

        assert!(result.is_ok());
        assert_eq!(
            reader.revocation_reason(),
            Some(ReaderRevocationReason::CredentialExposure)
        );
    }

    #[test]
    fn revoked_reader_cannot_be_enabled() {
        let mut reader = active_reader();

        assert!(
            reader
                .revoke(ReaderRevocationReason::SecurityIncident)
                .is_ok()
        );

        let result = reader.enable();

        assert!(matches!(
            result,
            Err(ReaderEquipmentError::InvalidStatusTransition {
                from: ReaderEquipmentStatus::Revoked,
                to: ReaderEquipmentStatus::Active
            })
        ));
    }

    #[test]
    fn active_reader_can_be_decommissioned() {
        let mut reader = active_reader();

        let result = reader.decommission();

        assert!(result.is_ok());
        assert_eq!(reader.status(), ReaderEquipmentStatus::Decommissioned);
        assert!(reader.status().is_terminal());
        assert!(!reader.can_accept_fare_presentations());
    }

    #[test]
    fn decommissioned_reader_cannot_be_activated() {
        let mut reader = active_reader();

        assert!(reader.decommission().is_ok());

        let result = reader.activate();

        assert!(matches!(
            result,
            Err(ReaderEquipmentError::InvalidStatusTransition {
                from: ReaderEquipmentStatus::Decommissioned,
                to: ReaderEquipmentStatus::Active
            })
        ));
    }

    #[test]
    fn equipment_key_can_be_rotated() {
        let mut reader = active_reader();
        let original = reader.identity().key_id();
        let replacement = EquipmentKeyId::generate();

        let result = reader.rotate_equipment_key(replacement);

        assert!(result.is_ok());
        assert_ne!(original, replacement);
        assert_eq!(reader.identity().key_id(), replacement);
    }

    #[test]
    fn terminal_reader_key_cannot_be_rotated() {
        let mut reader = active_reader();

        assert!(
            reader
                .revoke(ReaderRevocationReason::SuspectedCompromise)
                .is_ok()
        );

        let result = reader.rotate_equipment_key(EquipmentKeyId::generate());

        assert!(matches!(
            result,
            Err(ReaderEquipmentError::TerminalEquipment {
                status: ReaderEquipmentStatus::Revoked
            })
        ));
    }

    #[test]
    fn terminal_statuses_are_identified() {
        assert!(ReaderEquipmentStatus::Revoked.is_terminal());
        assert!(ReaderEquipmentStatus::Decommissioned.is_terminal());
        assert!(!ReaderEquipmentStatus::PendingRegistration.is_terminal());
        assert!(!ReaderEquipmentStatus::Active.is_terminal());
        assert!(!ReaderEquipmentStatus::Offline.is_terminal());
        assert!(!ReaderEquipmentStatus::Disabled.is_terminal());
    }
}
