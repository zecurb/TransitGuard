//! TransitGuard persistence adapters.
//!
//! This crate owns concrete PostgreSQL integration, connection pooling,
//! migrations, database record representations, and translation between
//! persistence records and core TransitGuard types.
//!
//! SQLx and PostgreSQL types must not leak into the domain crate.

mod codec;
mod config;
mod error;
mod postgres;
mod repositories;

pub use codec::PostgresValueCodec;
pub use config::PostgresConfig;
pub use error::PersistenceError;

pub use postgres::{connect_postgres, run_postgres_migrations};

pub use repositories::PostgresTransitAccountRepository;
