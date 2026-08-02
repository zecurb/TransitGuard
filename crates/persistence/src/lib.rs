//! TransitGuard persistence adapters.
//!
//! This crate owns concrete PostgreSQL and reader-local SQLite integration,
//! connection pooling, migrations, database records, and translation between
//! persistence records and core TransitGuard types.
//!
//! SQLx and PostgreSQL types must not leak into the domain crate.

mod codec;
mod config;
mod error;
mod postgres;
mod reader_queue;
mod reader_sqlite;
mod repositories;
mod transaction;

pub use codec::PostgresValueCodec;
pub use config::PostgresConfig;
pub use error::PersistenceError;

pub use postgres::{connect_postgres, run_postgres_migrations};

pub use reader_queue::{
    OfflineQueueState, OfflineTransactionDraft, QueuedOfflineTransaction, ReaderQueueError,
    enqueue_offline_transaction, load_offline_queue,
};

pub use reader_sqlite::{
    ReaderDatabaseIdentity, ReaderDatabaseState, ReaderSqliteConfig, ReaderStorageError,
    bind_reader_database, connect_reader_sqlite, run_reader_sqlite_migrations,
};

pub use repositories::{
    PostgresDomainEventRepository, PostgresFareCredentialRepository,
    PostgresReaderEquipmentRepository, PostgresTransitAccountRepository,
};

pub use transaction::PostgresTransactionManager;
