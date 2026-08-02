use core::{fmt, time::Duration};
use std::path::{Path, PathBuf};

use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;
use transitguard_device_protocol::DeviceProtocolVersion;
use transitguard_domain::ReaderId;

static READER_MIGRATOR: Migrator = sqlx::migrate!("./reader_migrations");

const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Validated configuration for one reader-local SQLite database.
#[derive(Clone)]
pub struct ReaderSqliteConfig {
    database_path: PathBuf,
    acquire_timeout: Duration,
    busy_timeout: Duration,
}

impl ReaderSqliteConfig {
    /// Creates a reader-local SQLite configuration.
    pub fn new(database_path: impl Into<PathBuf>) -> Result<Self, ReaderStorageError> {
        let database_path = database_path.into();

        if database_path.as_os_str().is_empty() {
            return Err(ReaderStorageError::EmptyDatabasePath);
        }

        Ok(Self {
            database_path,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
        })
    }

    /// Sets the connection-acquisition timeout.
    pub fn with_acquire_timeout(
        mut self,
        acquire_timeout: Duration,
    ) -> Result<Self, ReaderStorageError> {
        if acquire_timeout.is_zero() {
            return Err(ReaderStorageError::ZeroAcquireTimeout);
        }

        self.acquire_timeout = acquire_timeout;

        Ok(self)
    }

    /// Sets the SQLite lock busy timeout.
    pub fn with_busy_timeout(mut self, busy_timeout: Duration) -> Result<Self, ReaderStorageError> {
        if busy_timeout.is_zero() {
            return Err(ReaderStorageError::ZeroBusyTimeout);
        }

        self.busy_timeout = busy_timeout;

        Ok(self)
    }

    /// Returns the configured database path.
    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Returns the pool acquisition timeout.
    #[must_use]
    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }

    /// Returns the SQLite busy timeout.
    #[must_use]
    pub const fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }
}

impl fmt::Debug for ReaderSqliteConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReaderSqliteConfig")
            .field("database_path", &"<redacted>")
            .field("acquire_timeout", &self.acquire_timeout)
            .field("busy_timeout", &self.busy_timeout)
            .finish()
    }
}

/// Expected identity and metadata for one reader database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReaderDatabaseIdentity {
    reader_id: ReaderId,
    environment_id: String,
    software_version: String,
    protocol_version: DeviceProtocolVersion,
    initialized_at_unix_milliseconds: i64,
}

impl ReaderDatabaseIdentity {
    /// Creates validated expected reader-database identity.
    pub fn new(
        reader_id: ReaderId,
        environment_id: impl Into<String>,
        software_version: impl Into<String>,
        protocol_version: DeviceProtocolVersion,
        initialized_at_unix_milliseconds: i64,
    ) -> Result<Self, ReaderStorageError> {
        let environment_id = environment_id.into().trim().to_owned();

        if environment_id.is_empty() {
            return Err(ReaderStorageError::EmptyEnvironmentId);
        }

        let software_version = software_version.into().trim().to_owned();

        if software_version.is_empty() {
            return Err(ReaderStorageError::EmptySoftwareVersion);
        }

        if initialized_at_unix_milliseconds < 0 {
            return Err(ReaderStorageError::NegativeInitializationTime {
                unix_milliseconds: initialized_at_unix_milliseconds,
            });
        }

        Ok(Self {
            reader_id,
            environment_id,
            software_version,
            protocol_version,
            initialized_at_unix_milliseconds,
        })
    }

    /// Returns the configured reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the backend environment binding.
    #[must_use]
    pub fn environment_id(&self) -> &str {
        &self.environment_id
    }

    /// Returns the reader software version.
    #[must_use]
    pub fn software_version(&self) -> &str {
        &self.software_version
    }

    /// Returns the project-owned protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the initialization timestamp.
    #[must_use]
    pub const fn initialized_at_unix_milliseconds(&self) -> i64 {
        self.initialized_at_unix_milliseconds
    }
}

/// Durable metadata loaded from a reader database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReaderDatabaseState {
    reader_id: ReaderId,
    environment_id: String,
    software_version: String,
    protocol_version: DeviceProtocolVersion,
    next_local_sequence: u64,
    last_acknowledged_sequence: u64,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
}

impl ReaderDatabaseState {
    /// Returns the stored reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the stored environment identity.
    #[must_use]
    pub fn environment_id(&self) -> &str {
        &self.environment_id
    }

    /// Returns the stored software version.
    #[must_use]
    pub fn software_version(&self) -> &str {
        &self.software_version
    }

    /// Returns the stored protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the next available local sequence.
    #[must_use]
    pub const fn next_local_sequence(&self) -> u64 {
        self.next_local_sequence
    }

    /// Returns the last acknowledged local sequence.
    #[must_use]
    pub const fn last_acknowledged_sequence(&self) -> u64 {
        self.last_acknowledged_sequence
    }

    /// Returns the database creation timestamp.
    #[must_use]
    pub const fn created_at_unix_milliseconds(&self) -> i64 {
        self.created_at_unix_milliseconds
    }

    /// Returns the last metadata update timestamp.
    #[must_use]
    pub const fn updated_at_unix_milliseconds(&self) -> i64 {
        self.updated_at_unix_milliseconds
    }
}

/// Stable failures exposed by reader-local storage.
#[derive(Debug, Error)]
pub enum ReaderStorageError {
    /// No usable reader database path was supplied.
    #[error("reader SQLite database path must not be empty")]
    EmptyDatabasePath,

    /// Connection acquisition must be bounded.
    #[error("reader SQLite acquire timeout must be greater than zero")]
    ZeroAcquireTimeout,

    /// Lock waiting must be bounded.
    #[error("reader SQLite busy timeout must be greater than zero")]
    ZeroBusyTimeout,

    /// Reader databases must identify an environment.
    #[error("reader database environment identifier must not be empty")]
    EmptyEnvironmentId,

    /// Reader databases must identify software.
    #[error("reader software version must not be empty")]
    EmptySoftwareVersion,

    /// Initialization time cannot predate the epoch.
    #[error("reader database initialization time cannot be negative: {unix_milliseconds}")]
    NegativeInitializationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// The database belongs to another reader.
    #[error("stored reader {actual} does not match configured reader {expected}")]
    ReaderIdentityMismatch {
        /// Reader expected by the process.
        expected: ReaderId,

        /// Reader recorded by SQLite.
        actual: ReaderId,
    },

    /// The database belongs to another environment.
    #[error(
        "stored reader environment `{actual}` does not match configured environment `{expected}`"
    )]
    EnvironmentMismatch {
        /// Environment expected by the process.
        expected: String,

        /// Environment recorded by SQLite.
        actual: String,
    },

    /// SQLite contains invalid durable metadata.
    #[error("reader SQLite record contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A named SQLite operation failed.
    #[error("reader SQLite operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },

    /// A versioned reader migration failed.
    #[error("reader SQLite migration failed")]
    Migration {
        /// Original migration failure.
        #[source]
        source: sqlx::migrate::MigrateError,
    },
}

impl ReaderStorageError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    fn migration(source: sqlx::migrate::MigrateError) -> Self {
        Self::Migration { source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

#[derive(sqlx::FromRow)]
struct StoredReaderState {
    reader_id: String,
    environment_id: String,
    software_version: String,
    protocol_version: i64,
    next_local_sequence: i64,
    last_acknowledged_sequence: i64,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
}

/// Opens one reader-local SQLite database.
///
/// Every connection enables foreign keys, WAL mode, a bounded busy
/// timeout, and full synchronous durability.
pub async fn connect_reader_sqlite(
    config: &ReaderSqliteConfig,
) -> Result<SqlitePool, ReaderStorageError> {
    let options = SqliteConnectOptions::new()
        .filename(config.database_path())
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(config.busy_timeout());

    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .acquire_timeout(config.acquire_timeout())
        .connect_with(options)
        .await
        .map_err(|source| ReaderStorageError::database("connect", source))
}

/// Applies all embedded reader-local migrations.
pub async fn run_reader_sqlite_migrations(pool: &SqlitePool) -> Result<(), ReaderStorageError> {
    READER_MIGRATOR
        .run(pool)
        .await
        .map_err(ReaderStorageError::migration)
}

/// Initializes or verifies the durable reader identity.
///
/// Insertion and verification occur inside one SQLite transaction.
/// An existing database assigned to another reader or environment is
/// rejected rather than silently overwritten.
pub async fn bind_reader_database(
    pool: &SqlitePool,
    identity: &ReaderDatabaseIdentity,
) -> Result<ReaderDatabaseState, ReaderStorageError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|source| ReaderStorageError::database("begin identity binding", source))?;

    sqlx::query(
        r#"
        INSERT INTO reader_state (
            singleton,
            reader_id,
            environment_id,
            software_version,
            protocol_version,
            next_local_sequence,
            last_acknowledged_sequence,
            created_at_unix_milliseconds,
            updated_at_unix_milliseconds
        )
        VALUES (1, ?, ?, ?, ?, 1, 0, ?, ?)
        ON CONFLICT(singleton) DO NOTHING
        "#,
    )
    .bind(identity.reader_id().to_string())
    .bind(identity.environment_id())
    .bind(identity.software_version())
    .bind(i64::from(identity.protocol_version().value()))
    .bind(identity.initialized_at_unix_milliseconds())
    .bind(identity.initialized_at_unix_milliseconds())
    .execute(&mut *transaction)
    .await
    .map_err(|source| ReaderStorageError::database("initialize reader identity", source))?;

    let stored = sqlx::query_as::<_, StoredReaderState>(
        r#"
        SELECT
            reader_id,
            environment_id,
            software_version,
            protocol_version,
            next_local_sequence,
            last_acknowledged_sequence,
            created_at_unix_milliseconds,
            updated_at_unix_milliseconds
        FROM reader_state
        WHERE singleton = 1
        "#,
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|source| ReaderStorageError::database("load reader identity", source))?;

    let state = decode_stored_state(stored)?;

    if state.reader_id() != identity.reader_id() {
        return Err(ReaderStorageError::ReaderIdentityMismatch {
            expected: identity.reader_id(),
            actual: state.reader_id(),
        });
    }

    if state.environment_id() != identity.environment_id() {
        return Err(ReaderStorageError::EnvironmentMismatch {
            expected: identity.environment_id().to_owned(),
            actual: state.environment_id().to_owned(),
        });
    }

    transaction
        .commit()
        .await
        .map_err(|source| ReaderStorageError::database("commit identity binding", source))?;

    Ok(state)
}

fn decode_stored_state(
    stored: StoredReaderState,
) -> Result<ReaderDatabaseState, ReaderStorageError> {
    let reader_id = stored
        .reader_id
        .parse::<ReaderId>()
        .map_err(|_| ReaderStorageError::invalid_stored_value("reader_id"))?;

    let protocol_value = u16::try_from(stored.protocol_version)
        .map_err(|_| ReaderStorageError::invalid_stored_value("protocol_version"))?;

    let protocol_version = DeviceProtocolVersion::new(protocol_value)
        .map_err(|_| ReaderStorageError::invalid_stored_value("protocol_version"))?;

    let next_local_sequence = u64::try_from(stored.next_local_sequence)
        .map_err(|_| ReaderStorageError::invalid_stored_value("next_local_sequence"))?;

    if next_local_sequence == 0 {
        return Err(ReaderStorageError::invalid_stored_value(
            "next_local_sequence",
        ));
    }

    let last_acknowledged_sequence = u64::try_from(stored.last_acknowledged_sequence)
        .map_err(|_| ReaderStorageError::invalid_stored_value("last_acknowledged_sequence"))?;

    if last_acknowledged_sequence >= next_local_sequence {
        return Err(ReaderStorageError::invalid_stored_value(
            "last_acknowledged_sequence",
        ));
    }

    if stored.created_at_unix_milliseconds < 0 {
        return Err(ReaderStorageError::invalid_stored_value(
            "created_at_unix_milliseconds",
        ));
    }

    if stored.updated_at_unix_milliseconds < stored.created_at_unix_milliseconds {
        return Err(ReaderStorageError::invalid_stored_value(
            "updated_at_unix_milliseconds",
        ));
    }

    Ok(ReaderDatabaseState {
        reader_id,
        environment_id: stored.environment_id,
        software_version: stored.software_version,
        protocol_version,
        next_local_sequence,
        last_acknowledged_sequence,
        created_at_unix_milliseconds: stored.created_at_unix_milliseconds,
        updated_at_unix_milliseconds: stored.updated_at_unix_milliseconds,
    })
}

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
    };

    use sqlx::SqlitePool;
    use transitguard_device_protocol::DeviceProtocolVersion;
    use transitguard_domain::ReaderId;
    use uuid::Uuid;

    use super::{
        ReaderDatabaseIdentity, ReaderSqliteConfig, ReaderStorageError, bind_reader_database,
        connect_reader_sqlite, run_reader_sqlite_migrations,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn temporary_database_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-{test_name}-{}.sqlite3",
            Uuid::now_v7()
        ))
    }

    fn identity(reader_id: ReaderId, environment_id: &str) -> ReaderDatabaseIdentity {
        match ReaderDatabaseIdentity::new(
            reader_id,
            environment_id,
            "0.1.0",
            DeviceProtocolVersion::CURRENT,
            TEST_TIME,
        ) {
            Ok(identity) => identity,

            Err(error) => {
                panic!("valid reader identity failed: {error}")
            }
        }
    }

    async fn open_test_database(test_name: &str) -> (PathBuf, SqlitePool) {
        let path = temporary_database_path(test_name);

        let config = match ReaderSqliteConfig::new(path.clone()) {
            Ok(config) => config,

            Err(error) => {
                panic!("valid SQLite configuration failed: {error}")
            }
        };

        let pool = match connect_reader_sqlite(&config).await {
            Ok(pool) => pool,

            Err(error) => {
                remove_database_files(&path);

                panic!("SQLite connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&pool).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("reader migration failed: {error}");
        }

        (path, pool)
    }

    fn related_path(path: &Path, suffix: &str) -> PathBuf {
        let mut value = OsString::from(path.as_os_str());

        value.push(suffix);

        PathBuf::from(value)
    }

    fn remove_database_files(path: &Path) {
        let _ = std::fs::remove_file(path);

        let _ = std::fs::remove_file(related_path(path, "-wal"));

        let _ = std::fs::remove_file(related_path(path, "-shm"));
    }

    #[test]
    fn configuration_defaults_are_bounded() {
        let path = temporary_database_path("configuration-defaults");

        let config = match ReaderSqliteConfig::new(&path) {
            Ok(config) => config,

            Err(error) => {
                panic!("valid configuration failed: {error}")
            }
        };

        assert_eq!(config.acquire_timeout(), Duration::from_secs(5));

        assert_eq!(config.busy_timeout(), Duration::from_secs(5));

        let debug_output = format!("{config:?}");

        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains("configuration-defaults"));
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(matches!(
            ReaderSqliteConfig::new(PathBuf::new()),
            Err(ReaderStorageError::EmptyDatabasePath)
        ));

        let path = temporary_database_path("invalid-timeouts");

        let config = match ReaderSqliteConfig::new(path) {
            Ok(config) => config,

            Err(error) => {
                panic!("valid configuration failed: {error}")
            }
        };

        assert!(matches!(
            config.clone().with_acquire_timeout(Duration::ZERO),
            Err(ReaderStorageError::ZeroAcquireTimeout)
        ));

        assert!(matches!(
            config.with_busy_timeout(Duration::ZERO),
            Err(ReaderStorageError::ZeroBusyTimeout)
        ));
    }

    #[tokio::test]
    async fn required_sqlite_pragmas_are_enabled() {
        let (path, pool) = open_test_database("required-pragmas").await;

        let foreign_keys = match sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("foreign-key query failed: {error}")
            }
        };

        let journal_mode = match sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("journal-mode query failed: {error}")
            }
        };

        let busy_timeout = match sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(&pool)
            .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("busy-timeout query failed: {error}")
            }
        };

        let synchronous = match sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
            .fetch_one(&pool)
            .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("synchronous query failed: {error}")
            }
        };

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!(busy_timeout, 5_000);
        assert_eq!(synchronous, 2);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn reader_identity_survives_reopen() {
        let path = temporary_database_path("identity-reopen");

        let config = match ReaderSqliteConfig::new(path.clone()) {
            Ok(config) => config,

            Err(error) => {
                panic!("valid configuration failed: {error}")
            }
        };

        let reader_id = ReaderId::generate();
        let expected = identity(reader_id, "development");

        let first_pool = match connect_reader_sqlite(&config).await {
            Ok(pool) => pool,

            Err(error) => {
                remove_database_files(&path);

                panic!("first connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&first_pool).await {
            first_pool.close().await;
            remove_database_files(&path);

            panic!("first migration failed: {error}");
        }

        let first_state = match bind_reader_database(&first_pool, &expected).await {
            Ok(state) => state,

            Err(error) => {
                first_pool.close().await;
                remove_database_files(&path);

                panic!("first identity binding failed: {error}")
            }
        };

        first_pool.close().await;

        let second_pool = match connect_reader_sqlite(&config).await {
            Ok(pool) => pool,

            Err(error) => {
                remove_database_files(&path);

                panic!("second connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&second_pool).await {
            second_pool.close().await;
            remove_database_files(&path);

            panic!("second migration failed: {error}");
        }

        let second_state = match bind_reader_database(&second_pool, &expected).await {
            Ok(state) => state,

            Err(error) => {
                second_pool.close().await;
                remove_database_files(&path);

                panic!("second identity binding failed: {error}")
            }
        };

        assert_eq!(first_state, second_state);
        assert_eq!(second_state.reader_id(), reader_id);
        assert_eq!(second_state.environment_id(), "development");
        assert_eq!(second_state.next_local_sequence(), 1);
        assert_eq!(second_state.last_acknowledged_sequence(), 0);

        second_pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn different_reader_is_rejected() {
        let (path, pool) = open_test_database("reader-mismatch").await;

        let stored_reader = ReaderId::generate();
        let configured_reader = ReaderId::generate();

        let stored_identity = identity(stored_reader, "development");

        if let Err(error) = bind_reader_database(&pool, &stored_identity).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("initial identity binding failed: {error}");
        }

        let configured_identity = identity(configured_reader, "development");

        let result = bind_reader_database(&pool, &configured_identity).await;

        assert!(matches!(
            result,
            Err(
                ReaderStorageError::
                    ReaderIdentityMismatch {
                        expected,
                        actual,
                    }
            ) if expected == configured_reader
                && actual == stored_reader
        ));

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn different_environment_is_rejected() {
        let (path, pool) = open_test_database("environment-mismatch").await;

        let reader_id = ReaderId::generate();

        let stored_identity = identity(reader_id, "development");

        if let Err(error) = bind_reader_database(&pool, &stored_identity).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("initial identity binding failed: {error}");
        }

        let configured_identity = identity(reader_id, "staging");

        let result = bind_reader_database(&pool, &configured_identity).await;

        assert!(matches!(
            result,
            Err(
                ReaderStorageError::
                    EnvironmentMismatch {
                        expected,
                        actual,
                    }
            ) if expected == "staging"
                && actual == "development"
        ));

        pool.close().await;
        remove_database_files(&path);
    }
}
