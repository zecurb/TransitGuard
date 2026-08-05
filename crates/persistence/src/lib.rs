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
mod reader_acknowledgement;
mod reader_acknowledgement_application;
mod reader_health;
mod reader_queue;
mod reader_queue_state;
mod reader_sqlite;
mod reader_sync;
mod reader_sync_request;
mod reader_sync_state;
mod repositories;
mod synchronization_ingest_record;
mod transaction;

pub use codec::PostgresValueCodec;
pub use config::PostgresConfig;
pub use error::PersistenceError;

pub use postgres::{connect_postgres, run_postgres_migrations};

pub use reader_acknowledgement::{
    ReaderAcknowledgementError, StoredSynchronizationAcknowledgement,
    SynchronizationAcknowledgement, SynchronizationAcknowledgementEntry,
    SynchronizationEntryResolution, store_synchronization_acknowledgement,
};

pub use reader_acknowledgement_application::{
    ReaderAcknowledgementApplicationError, SynchronizationAcknowledgementApplication,
    apply_synchronization_acknowledgement,
};

pub use reader_health::{
    ReaderHealthError, ReaderQueueHealthCounts, ReaderQueueHealthSnapshot,
    ReaderSynchronizationHealthCounts, load_reader_queue_health,
};

pub use reader_queue::{
    OfflineQueueState, OfflineTransactionDraft, QueuedOfflineTransaction, ReaderQueueError,
    enqueue_offline_transaction, load_offline_queue,
};

pub use reader_queue_state::{
    ReaderQueueStateError, load_ready_offline_transactions, mark_offline_transaction_in_flight,
    record_manual_review_required, record_permanent_queue_failure, record_retryable_queue_failure,
    recover_interrupted_offline_queue,
};

pub use reader_sqlite::{
    ReaderDatabaseIdentity, ReaderDatabaseState, ReaderSqliteConfig, ReaderStorageError,
    bind_reader_database, connect_reader_sqlite, run_reader_sqlite_migrations,
};

pub use reader_sync::{
    ReaderSynchronizationError, SynchronizationBatch, SynchronizationBatchEntry,
    SynchronizationBatchState, create_synchronization_batch, load_synchronization_batch,
};

pub use reader_sync_request::{
    ReaderSynchronizationRequestError, load_synchronization_batch_request,
};

pub use reader_sync_state::{
    ReaderSynchronizationStateError, load_ready_synchronization_batches,
    mark_synchronization_batch_in_flight, record_synchronization_retryable_failure,
    recover_interrupted_synchronization_batches,
};

pub use repositories::{
    PostgresDomainEventRepository, PostgresFareCredentialRepository,
    PostgresReaderEquipmentRepository, PostgresSynchronizationIngestRepository,
    PostgresTransitAccountRepository, SynchronizationIngestDisposition,
    SynchronizationIngestPersistenceError,
};

pub use synchronization_ingest_record::{
    PreparedSynchronizationIngest, PreparedSynchronizationIngestEntry,
    SynchronizationIngestRecordError,
};

pub use transaction::PostgresTransactionManager;
