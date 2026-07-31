//! Deterministic TransitGuard fare-policy evaluation.
//!
//! This crate contains database-independent and network-independent fare
//! policy models and evaluation rules shared by backend applications and the
//! reader simulator.

pub mod policy;

pub use policy::{
    DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition, FarePolicyError,
    FarePolicyValueError, TransferWindow, ZoneId,
};
