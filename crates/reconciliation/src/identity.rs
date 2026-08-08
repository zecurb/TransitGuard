use core::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::{Uuid, Variant, Version};

/// Errors produced while validating a reconciliation identifier.
#[derive(Debug, Error)]
pub enum ReconciliationIdError {
    /// The all-zero UUID is not a valid reconciliation identity.
    #[error("reconciliation identifier cannot use the nil UUID")]
    Nil,

    /// The UUID did not use the required RFC variant.
    #[error("reconciliation identifier must use the RFC 9562 UUID variant")]
    InvalidVariant,

    /// Reconciliation identities use time-sortable UUID version 7 values.
    #[error("reconciliation identifier must use UUID version 7")]
    UnsupportedVersion,

    /// Text could not be parsed as a UUID.
    #[error("invalid reconciliation identifier: {source}")]
    InvalidFormat {
        /// UUID parsing failure.
        #[source]
        source: uuid::Error,
    },
}

/// Strongly typed identity of one authoritative reconciliation record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReconciliationId(Uuid);

impl ReconciliationId {
    /// Generates a new time-sortable reconciliation identity.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn validate(value: Uuid) -> Result<Self, ReconciliationIdError> {
        if value.is_nil() {
            return Err(ReconciliationIdError::Nil);
        }

        if value.get_variant() != Variant::RFC4122 {
            return Err(ReconciliationIdError::InvalidVariant);
        }

        if value.get_version() != Some(Version::SortRand) {
            return Err(ReconciliationIdError::UnsupportedVersion);
        }

        Ok(Self(value))
    }
}

impl TryFrom<Uuid> for ReconciliationId {
    type Error = ReconciliationIdError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        Self::validate(value)
    }
}

impl FromStr for ReconciliationId {
    type Err = ReconciliationIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = Uuid::parse_str(value)
            .map_err(|source| ReconciliationIdError::InvalidFormat { source })?;

        Self::try_from(parsed)
    }
}

impl fmt::Display for ReconciliationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for ReconciliationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Uuid::deserialize(deserializer)?;

        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use uuid::{Variant, Version};

    use super::{ReconciliationId, ReconciliationIdError};

    #[test]
    fn generated_identity_is_uuid_version_seven() {
        let id = ReconciliationId::generate();

        assert!(!id.as_uuid().is_nil());
        assert_eq!(id.as_uuid().get_variant(), Variant::RFC4122);
        assert_eq!(id.as_uuid().get_version(), Some(Version::SortRand));
    }

    #[test]
    fn identity_round_trips_through_text() {
        let original = ReconciliationId::generate();
        let parsed = original.to_string().parse::<ReconciliationId>();

        assert!(matches!(parsed, Ok(value) if value == original));
    }

    #[test]
    fn non_version_seven_identity_is_rejected() {
        let parsed = "550e8400-e29b-41d4-a716-446655440000".parse::<ReconciliationId>();

        assert!(matches!(
            parsed,
            Err(ReconciliationIdError::UnsupportedVersion)
        ));
    }
}
