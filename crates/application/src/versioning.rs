use transitguard_domain::AggregateVersion;

/// A condition that an infrastructure adapter must enforce atomically when
/// saving an aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveCondition {
    /// The aggregate must not already exist.
    MustNotExist,

    /// The stored aggregate must currently have the supplied version.
    IfVersion(AggregateVersion),
}

/// An aggregate loaded together with its authoritative persistence version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionedAggregate<T> {
    aggregate: T,
    version: AggregateVersion,
}

impl<T> VersionedAggregate<T> {
    /// Creates a versioned aggregate returned by a persistence adapter.
    #[must_use]
    pub fn new(aggregate: T, version: AggregateVersion) -> Self {
        Self { aggregate, version }
    }

    /// Borrows the aggregate.
    #[must_use]
    pub fn aggregate(&self) -> &T {
        &self.aggregate
    }

    /// Consumes the wrapper and returns the aggregate.
    #[must_use]
    pub fn into_aggregate(self) -> T {
        self.aggregate
    }

    /// Returns the authoritative version loaded from persistence.
    #[must_use]
    pub const fn version(&self) -> AggregateVersion {
        self.version
    }

    /// Calculates the version produced by the next successful mutation.
    #[must_use]
    pub fn next_version(&self) -> Option<AggregateVersion> {
        self.version
            .value()
            .checked_add(1)
            .and_then(|value| AggregateVersion::new(value).ok())
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::AggregateVersion;

    use super::{SaveCondition, VersionedAggregate};

    fn version(value: u64) -> AggregateVersion {
        match AggregateVersion::new(value) {
            Ok(version) => version,
            Err(error) => {
                panic!("valid aggregate version failed: {error}")
            }
        }
    }

    #[test]
    fn wrapper_preserves_aggregate_and_version() {
        let wrapped = VersionedAggregate::new("credential", version(4));

        assert_eq!(wrapped.aggregate(), &"credential");
        assert_eq!(wrapped.version(), version(4));
    }

    #[test]
    fn wrapper_calculates_next_version() {
        let wrapped = VersionedAggregate::new("credential", version(7));

        assert_eq!(wrapped.next_version(), Some(version(8)));
    }

    #[test]
    fn save_conditions_are_explicit() {
        assert_ne!(
            SaveCondition::MustNotExist,
            SaveCondition::IfVersion(version(1))
        );
    }
}
