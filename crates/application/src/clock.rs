use transitguard_domain::DomainEventTime;

use crate::ClockError;

/// Provides authoritative time to application use cases.
///
/// Application services depend on this abstraction instead of reading the
/// operating-system clock directly. Tests may supply deterministic fixed time.
pub trait Clock: Send + Sync {
    /// Returns the current authoritative domain-event time.
    fn now(&self) -> Result<DomainEventTime, ClockError>;
}

#[cfg(test)]
mod tests {
    use transitguard_domain::DomainEventTime;

    use super::Clock;

    struct FixedClock {
        current_time: DomainEventTime,
    }

    impl Clock for FixedClock {
        fn now(&self) -> Result<DomainEventTime, crate::ClockError> {
            Ok(self.current_time)
        }
    }

    fn valid_time() -> DomainEventTime {
        match DomainEventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid fixed time failed: {error}")
            }
        }
    }

    #[test]
    fn fixed_clock_returns_deterministic_time() {
        let expected = valid_time();
        let clock = FixedClock {
            current_time: expected,
        };

        let result = clock.now();

        assert_eq!(result, Ok(expected));
    }

    #[test]
    fn clock_trait_is_object_safe() {
        let clock = FixedClock {
            current_time: valid_time(),
        };
        let object: &dyn Clock = &clock;

        assert!(object.now().is_ok());
    }
}
