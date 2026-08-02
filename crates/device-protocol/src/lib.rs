//! Project-owned protocol types used by TransitGuard reader simulators.
//!
//! These types do not implement or claim compatibility with any real
//! transit-card, mobile-wallet, or fare-reader protocol.

pub mod presentation;
pub mod version;

pub use presentation::{
    CredentialMedium, CredentialPresentation, CredentialPresentationDefinition,
    PresentationValueError, ProtocolZoneId,
};

pub use version::{DeviceProtocolVersion, DeviceProtocolVersionError};
