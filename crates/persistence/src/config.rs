use core::{fmt, time::Duration};

use crate::PersistenceError;

const DEFAULT_MIN_CONNECTIONS: u32 = 1;
const DEFAULT_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Validated PostgreSQL connection-pool configuration.
///
/// Database URLs may contain credentials, so the custom `Debug`
/// implementation always redacts the URL.
#[derive(Clone)]
pub struct PostgresConfig {
    database_url: String,
    min_connections: u32,
    max_connections: u32,
    acquire_timeout: Duration,
}

impl PostgresConfig {
    /// Creates a PostgreSQL configuration with conservative defaults.
    pub fn new(database_url: impl Into<String>) -> Result<Self, PersistenceError> {
        let database_url = database_url.into();

        if database_url.trim().is_empty() {
            return Err(PersistenceError::EmptyDatabaseUrl);
        }

        Ok(Self {
            database_url,
            min_connections: DEFAULT_MIN_CONNECTIONS,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        })
    }

    /// Sets validated minimum and maximum pool sizes.
    pub fn with_pool_bounds(
        mut self,
        minimum: u32,
        maximum: u32,
    ) -> Result<Self, PersistenceError> {
        if maximum == 0 {
            return Err(PersistenceError::ZeroMaxConnections);
        }

        if minimum > maximum {
            return Err(PersistenceError::InvalidPoolBounds { minimum, maximum });
        }

        self.min_connections = minimum;
        self.max_connections = maximum;

        Ok(self)
    }

    /// Sets how long callers may wait to acquire a pooled connection.
    pub fn with_acquire_timeout(
        mut self,
        acquire_timeout: Duration,
    ) -> Result<Self, PersistenceError> {
        if acquire_timeout.is_zero() {
            return Err(PersistenceError::ZeroAcquireTimeout);
        }

        self.acquire_timeout = acquire_timeout;

        Ok(self)
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    pub(crate) const fn min_connections(&self) -> u32 {
        self.min_connections
    }

    pub(crate) const fn max_connections(&self) -> u32 {
        self.max_connections
    }

    pub(crate) const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }
}

impl fmt::Debug for PostgresConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresConfig")
            .field("database_url", &"<redacted>")
            .field("min_connections", &self.min_connections)
            .field("max_connections", &self.max_connections)
            .field("acquire_timeout", &self.acquire_timeout)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use super::PostgresConfig;
    use crate::PersistenceError;

    fn valid_config() -> PostgresConfig {
        match PostgresConfig::new(
            "postgres://transitguard:secret@\
             localhost/transitguard",
        ) {
            Ok(config) => config,

            Err(error) => {
                panic!(
                    "valid configuration failed: \
                     {error}"
                )
            }
        }
    }

    #[test]
    fn defaults_are_bounded() {
        let config = valid_config();

        assert_eq!(config.min_connections(), 1);
        assert_eq!(config.max_connections(), 10);

        assert_eq!(config.acquire_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn blank_database_url_is_rejected() {
        let result = PostgresConfig::new("   ");

        assert!(matches!(result, Err(PersistenceError::EmptyDatabaseUrl)));
    }

    #[test]
    fn zero_maximum_is_rejected() {
        let result = valid_config().with_pool_bounds(0, 0);

        assert!(matches!(result, Err(PersistenceError::ZeroMaxConnections)));
    }

    #[test]
    fn minimum_cannot_exceed_maximum() {
        let result = valid_config().with_pool_bounds(8, 4);

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidPoolBounds {
                minimum: 8,
                maximum: 4,
            })
        ));
    }

    #[test]
    fn zero_acquire_timeout_is_rejected() {
        let result = valid_config().with_acquire_timeout(Duration::ZERO);

        assert!(matches!(result, Err(PersistenceError::ZeroAcquireTimeout)));
    }

    #[test]
    fn debug_output_redacts_database_url() {
        let output = format!("{:?}", valid_config());

        assert!(!output.contains("secret"));
        assert!(!output.contains("localhost"));
        assert!(output.contains("<redacted>"));
    }
}
