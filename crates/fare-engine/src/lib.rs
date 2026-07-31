//! Deterministic TransitGuard fare-policy evaluation.
//!
//! This crate contains database-independent and network-independent fare
//! policy models and evaluation rules shared by backend applications and the
//! reader simulator.

pub mod evaluation;
pub mod policy;

pub use evaluation::{
    FareCalculationStage, FareDecisionEvidence, FareEvaluation, FareEvaluationError,
    FareEvaluationInput, FareEvaluationOutcome, evaluate_fare,
};

pub use policy::{
    DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition, FarePolicyError,
    FarePolicyValueError, TransferWindow, ZoneId,
};
