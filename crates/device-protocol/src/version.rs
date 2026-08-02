use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Errors produced while constructing device-protocol values.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DeviceProtocolVersionError {
    /// Protocol versions begin at one.
    #[error("device-protocol version must be greater than zero")]
    ZeroVersion,
}

/// Version of the project-owned TransitGuard reader protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DeviceProtocolVersion(u16);

impl DeviceProtocolVersion {
    /// Current project-owned protocol version.
    pub const CURRENT: Self = Self(1);

    /// Creates a validated protocol version.
    pub const fn new(value: u16) -> Result<Self, DeviceProtocolVersionError> {
        if value == 0 {
            return Err(DeviceProtocolVersionError::ZeroVersion);
        }

        Ok(Self(value))
    }

    /// Returns the numeric protocol version.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DeviceProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceProtocolVersion, DeviceProtocolVersionError};

    #[test]
    fn zero_protocol_version_is_rejected() {
        assert_eq!(
            DeviceProtocolVersion::new(0),
            Err(DeviceProtocolVersionError::ZeroVersion)
        );
    }

    #[test]
    fn current_protocol_version_is_one() {
        assert_eq!(DeviceProtocolVersion::CURRENT.value(), 1);
    }
}
