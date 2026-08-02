use core::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::{Uuid, Variant, Version};

/// Errors produced while constructing or parsing TransitGuard identifiers.
#[derive(Debug, Error)]
pub enum IdentifierError {
    /// The identifier used the all-zero UUID.
    #[error("{kind} cannot use the nil UUID")]
    Nil {
        /// Human-readable identifier category.
        kind: &'static str,
    },

    /// The identifier did not use the RFC 9562 UUID variant.
    #[error("{kind} must use the RFC 9562 UUID variant")]
    InvalidVariant {
        /// Human-readable identifier category.
        kind: &'static str,
    },

    /// The identifier did not use UUID version 7.
    #[error("{kind} must use UUID version 7")]
    UnsupportedVersion {
        /// Human-readable identifier category.
        kind: &'static str,
    },

    /// The identifier text was not a valid UUID.
    #[error("invalid {kind}: {source}")]
    InvalidFormat {
        /// Human-readable identifier category.
        kind: &'static str,

        /// UUID parsing error.
        #[source]
        source: uuid::Error,
    },
}

macro_rules! define_identifier {
    ($name:ident, $kind:literal) => {
        #[doc = concat!("Strongly typed ", $kind, ".")]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new time-sortable UUID version 7 identifier.
            #[must_use]
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// Returns the underlying UUID by reference.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Consumes the identifier and returns its underlying UUID.
            #[must_use]
            pub const fn into_uuid(self) -> Uuid {
                self.0
            }

            fn validate(value: Uuid) -> Result<Self, IdentifierError> {
                if value.is_nil() {
                    return Err(IdentifierError::Nil { kind: $kind });
                }

                if value.get_variant() != Variant::RFC4122 {
                    return Err(IdentifierError::InvalidVariant { kind: $kind });
                }

                if value.get_version() != Some(Version::SortRand) {
                    return Err(IdentifierError::UnsupportedVersion { kind: $kind });
                }

                Ok(Self(value))
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = IdentifierError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::validate(value)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let parsed =
                    Uuid::parse_str(value).map_err(|source| IdentifierError::InvalidFormat {
                        kind: $kind,
                        source,
                    })?;

                Self::try_from(parsed)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = Uuid::deserialize(deserializer)?;

                Self::try_from(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_identifier!(TransitAccountId, "transit account identifier");
define_identifier!(RiderId, "rider identifier");
define_identifier!(FareCredentialId, "fare credential identifier");
define_identifier!(ReaderId, "reader identifier");
define_identifier!(EquipmentKeyId, "equipment key identifier");
define_identifier!(FareTransactionId, "fare transaction identifier");
define_identifier!(FarePolicyId, "fare policy identifier");
define_identifier!(JourneyId, "journey identifier");
define_identifier!(TransitProductId, "transit product identifier");
define_identifier!(
    TransitProductInstanceId,
    "transit product instance identifier"
);
define_identifier!(SynchronizationBatchId, "synchronization batch identifier");
define_identifier!(DomainEventId, "domain event identifier");

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde::de::value::{Error as ValueError, StringDeserializer};
    use uuid::{Variant, Version};

    use super::{FareCredentialId, IdentifierError, ReaderId, TransitAccountId};

    #[test]
    fn generated_identifier_is_uuid_version_seven() {
        let identifier = TransitAccountId::generate();

        assert!(!identifier.as_uuid().is_nil());
        assert_eq!(identifier.as_uuid().get_variant(), Variant::RFC4122);
        assert_eq!(identifier.as_uuid().get_version(), Some(Version::SortRand));
    }

    #[test]
    fn identifier_round_trips_through_text() {
        let original = TransitAccountId::generate();
        let parsed = original.to_string().parse::<TransitAccountId>();

        assert!(matches!(parsed, Ok(value) if value == original));
    }

    #[test]
    fn different_identifier_types_generate_different_values() {
        let account_id = TransitAccountId::generate();
        let credential_id = FareCredentialId::generate();

        assert_ne!(account_id.to_string(), credential_id.to_string());
    }

    #[test]
    fn nil_uuid_is_rejected() {
        let parsed = "00000000-0000-0000-0000-000000000000".parse::<TransitAccountId>();

        assert!(matches!(
            parsed,
            Err(IdentifierError::Nil {
                kind: "transit account identifier"
            })
        ));
    }

    #[test]
    fn non_version_seven_uuid_is_rejected() {
        let parsed = "550e8400-e29b-41d4-a716-446655440000".parse::<FareCredentialId>();

        assert!(matches!(
            parsed,
            Err(IdentifierError::UnsupportedVersion {
                kind: "fare credential identifier"
            })
        ));
    }

    #[test]
    fn malformed_uuid_is_rejected() {
        let parsed = "not-a-uuid".parse::<ReaderId>();

        assert!(matches!(
            parsed,
            Err(IdentifierError::InvalidFormat {
                kind: "reader identifier",
                ..
            })
        ));
    }

    #[test]
    fn serde_deserialization_preserves_validation() {
        let original = FareCredentialId::generate();
        let deserializer = StringDeserializer::<ValueError>::new(original.to_string());

        let decoded = FareCredentialId::deserialize(deserializer);

        assert!(matches!(decoded, Ok(value) if value == original));
    }

    #[test]
    fn serde_rejects_non_version_seven_uuid() {
        let deserializer = StringDeserializer::<ValueError>::new(String::from(
            "550e8400-e29b-41d4-a716-446655440000",
        ));

        let decoded = FareCredentialId::deserialize(deserializer);

        assert!(decoded.is_err());
    }
}
