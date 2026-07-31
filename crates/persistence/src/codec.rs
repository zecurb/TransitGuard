use transitguard_domain::{
    AggregateVersion, Currency, DomainAggregateId, DomainEventTime, EligibilityClassification,
    EquipmentKeyId, FareCredentialId, FareCredentialKind, FareCredentialStatus, FareTransactionId,
    ReaderDisablementReason, ReaderEquipmentStatus, ReaderId, ReaderRevocationReason,
    RevocationReason, TransitAccountId, TransitAccountStatus,
};
use uuid::Uuid;

use crate::PersistenceError;

/// Converts TransitGuard domain values to and from stable PostgreSQL values.
///
/// Database representation details remain owned by the persistence crate.
/// Domain and application crates do not depend on these encodings.
pub struct PostgresValueCodec;

impl PostgresValueCodec {
    /// Encodes a supported currency.
    #[must_use]
    pub const fn encode_currency(value: Currency) -> &'static str {
        match value {
            Currency::Usd => "USD",
            Currency::Cad => "CAD",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
        }
    }

    /// Decodes a stored currency.
    pub fn decode_currency(value: &str) -> Result<Currency, PersistenceError> {
        match value {
            "USD" => Ok(Currency::Usd),
            "CAD" => Ok(Currency::Cad),
            "EUR" => Ok(Currency::Eur),
            "GBP" => Ok(Currency::Gbp),

            _ => Err(invalid("transit_accounts.stored_value_currency")),
        }
    }

    /// Encodes a transit-account status.
    #[must_use]
    pub const fn encode_account_status(value: TransitAccountStatus) -> &'static str {
        match value {
            TransitAccountStatus::Active => "active",
            TransitAccountStatus::Suspended => "suspended",
            TransitAccountStatus::Closed => "closed",
        }
    }

    /// Decodes a transit-account status.
    pub fn decode_account_status(value: &str) -> Result<TransitAccountStatus, PersistenceError> {
        match value {
            "active" => Ok(TransitAccountStatus::Active),

            "suspended" => Ok(TransitAccountStatus::Suspended),

            "closed" => Ok(TransitAccountStatus::Closed),

            _ => Err(invalid("transit_accounts.status")),
        }
    }

    /// Encodes a fare-eligibility classification.
    #[must_use]
    pub const fn encode_eligibility(value: EligibilityClassification) -> &'static str {
        match value {
            EligibilityClassification::Standard => "standard",

            EligibilityClassification::Youth => "youth",

            EligibilityClassification::Senior => "senior",

            EligibilityClassification::ReducedFare => "reduced_fare",

            EligibilityClassification::EmployeeTestAccount => "employee_test_account",
        }
    }

    /// Decodes a fare-eligibility classification.
    pub fn decode_eligibility(value: &str) -> Result<EligibilityClassification, PersistenceError> {
        match value {
            "standard" => Ok(EligibilityClassification::Standard),

            "youth" => Ok(EligibilityClassification::Youth),

            "senior" => Ok(EligibilityClassification::Senior),

            "reduced_fare" => Ok(EligibilityClassification::ReducedFare),

            "employee_test_account" => Ok(EligibilityClassification::EmployeeTestAccount),

            _ => Err(invalid("transit_accounts.eligibility")),
        }
    }

    /// Encodes a fare-credential kind.
    #[must_use]
    pub const fn encode_credential_kind(value: FareCredentialKind) -> &'static str {
        match value {
            FareCredentialKind::Card => "card",
            FareCredentialKind::Mobile => "mobile",

            FareCredentialKind::DevelopmentTestToken => "development_test_token",
        }
    }

    /// Decodes a fare-credential kind.
    pub fn decode_credential_kind(value: &str) -> Result<FareCredentialKind, PersistenceError> {
        match value {
            "card" => Ok(FareCredentialKind::Card),

            "mobile" => Ok(FareCredentialKind::Mobile),

            "development_test_token" => Ok(FareCredentialKind::DevelopmentTestToken),

            _ => Err(invalid("fare_credentials.kind")),
        }
    }

    /// Encodes a fare-credential status.
    #[must_use]
    pub const fn encode_credential_status(value: FareCredentialStatus) -> &'static str {
        match value {
            FareCredentialStatus::Pending => "pending",
            FareCredentialStatus::Active => "active",

            FareCredentialStatus::Suspended => "suspended",

            FareCredentialStatus::Revoked => "revoked",
            FareCredentialStatus::Expired => "expired",
            FareCredentialStatus::Replaced => "replaced",
        }
    }

    /// Decodes a fare-credential status.
    pub fn decode_credential_status(value: &str) -> Result<FareCredentialStatus, PersistenceError> {
        match value {
            "pending" => Ok(FareCredentialStatus::Pending),

            "active" => Ok(FareCredentialStatus::Active),

            "suspended" => Ok(FareCredentialStatus::Suspended),

            "revoked" => Ok(FareCredentialStatus::Revoked),

            "expired" => Ok(FareCredentialStatus::Expired),

            "replaced" => Ok(FareCredentialStatus::Replaced),

            _ => Err(invalid("fare_credentials.status")),
        }
    }

    /// Encodes a credential-revocation reason.
    #[must_use]
    pub const fn encode_credential_revocation_reason(value: RevocationReason) -> &'static str {
        match value {
            RevocationReason::ReportedLost => "reported_lost",

            RevocationReason::ReportedStolen => "reported_stolen",

            RevocationReason::Replaced => "replaced",

            RevocationReason::AccountSuspended => "account_suspended",

            RevocationReason::AdministrativeAction => "administrative_action",

            RevocationReason::SecurityIncident => "security_incident",

            RevocationReason::TestCleanup => "test_cleanup",
        }
    }

    /// Decodes a credential-revocation reason.
    pub fn decode_credential_revocation_reason(
        value: &str,
    ) -> Result<RevocationReason, PersistenceError> {
        match value {
            "reported_lost" => Ok(RevocationReason::ReportedLost),

            "reported_stolen" => Ok(RevocationReason::ReportedStolen),

            "replaced" => Ok(RevocationReason::Replaced),

            "account_suspended" => Ok(RevocationReason::AccountSuspended),

            "administrative_action" => Ok(RevocationReason::AdministrativeAction),

            "security_incident" => Ok(RevocationReason::SecurityIncident),

            "test_cleanup" => Ok(RevocationReason::TestCleanup),

            _ => Err(invalid("fare_credentials.revocation_reason")),
        }
    }

    /// Encodes a reader-equipment status.
    #[must_use]
    pub const fn encode_reader_status(value: ReaderEquipmentStatus) -> &'static str {
        match value {
            ReaderEquipmentStatus::PendingRegistration => "pending_registration",

            ReaderEquipmentStatus::Active => "active",
            ReaderEquipmentStatus::Offline => "offline",

            ReaderEquipmentStatus::Disabled => "disabled",

            ReaderEquipmentStatus::Revoked => "revoked",

            ReaderEquipmentStatus::Decommissioned => "decommissioned",
        }
    }

    /// Decodes a reader-equipment status.
    pub fn decode_reader_status(value: &str) -> Result<ReaderEquipmentStatus, PersistenceError> {
        match value {
            "pending_registration" => Ok(ReaderEquipmentStatus::PendingRegistration),

            "active" => Ok(ReaderEquipmentStatus::Active),

            "offline" => Ok(ReaderEquipmentStatus::Offline),

            "disabled" => Ok(ReaderEquipmentStatus::Disabled),

            "revoked" => Ok(ReaderEquipmentStatus::Revoked),

            "decommissioned" => Ok(ReaderEquipmentStatus::Decommissioned),

            _ => Err(invalid("reader_equipment.status")),
        }
    }

    /// Encodes a reader-disablement reason.
    #[must_use]
    pub const fn encode_reader_disablement_reason(value: ReaderDisablementReason) -> &'static str {
        match value {
            ReaderDisablementReason::SuspectedCompromise => "suspected_compromise",

            ReaderDisablementReason::LostEquipment => "lost_equipment",

            ReaderDisablementReason::InvalidConfiguration => "invalid_configuration",

            ReaderDisablementReason::AdministrativeAction => "administrative_action",

            ReaderDisablementReason::TestCleanup => "test_cleanup",
        }
    }

    /// Decodes a reader-disablement reason.
    pub fn decode_reader_disablement_reason(
        value: &str,
    ) -> Result<ReaderDisablementReason, PersistenceError> {
        match value {
            "suspected_compromise" => Ok(ReaderDisablementReason::SuspectedCompromise),

            "lost_equipment" => Ok(ReaderDisablementReason::LostEquipment),

            "invalid_configuration" => Ok(ReaderDisablementReason::InvalidConfiguration),

            "administrative_action" => Ok(ReaderDisablementReason::AdministrativeAction),

            "test_cleanup" => Ok(ReaderDisablementReason::TestCleanup),

            _ => Err(invalid("reader_equipment.disablement_reason")),
        }
    }

    /// Encodes a reader-revocation reason.
    #[must_use]
    pub const fn encode_reader_revocation_reason(value: ReaderRevocationReason) -> &'static str {
        match value {
            ReaderRevocationReason::SuspectedCompromise => "suspected_compromise",

            ReaderRevocationReason::CredentialExposure => "credential_exposure",

            ReaderRevocationReason::AdministrativeAction => "administrative_action",

            ReaderRevocationReason::SecurityIncident => "security_incident",

            ReaderRevocationReason::TestCleanup => "test_cleanup",
        }
    }

    /// Decodes a reader-revocation reason.
    pub fn decode_reader_revocation_reason(
        value: &str,
    ) -> Result<ReaderRevocationReason, PersistenceError> {
        match value {
            "suspected_compromise" => Ok(ReaderRevocationReason::SuspectedCompromise),

            "credential_exposure" => Ok(ReaderRevocationReason::CredentialExposure),

            "administrative_action" => Ok(ReaderRevocationReason::AdministrativeAction),

            "security_incident" => Ok(ReaderRevocationReason::SecurityIncident),

            "test_cleanup" => Ok(ReaderRevocationReason::TestCleanup),

            _ => Err(invalid("reader_equipment.revocation_reason")),
        }
    }

    /// Converts an aggregate version into PostgreSQL `BIGINT`.
    pub fn encode_aggregate_version(version: AggregateVersion) -> Result<i64, PersistenceError> {
        i64::try_from(version.value()).map_err(|_| numeric_out_of_range("aggregate_version"))
    }

    /// Reconstructs an aggregate version from PostgreSQL `BIGINT`.
    pub fn decode_aggregate_version(value: i64) -> Result<AggregateVersion, PersistenceError> {
        let value = u64::try_from(value).map_err(|_| numeric_out_of_range("aggregate_version"))?;

        AggregateVersion::new(value).map_err(|_| invalid("aggregate_version"))
    }

    /// Converts a domain-event time to Unix milliseconds.
    #[must_use]
    pub const fn encode_event_time(value: DomainEventTime) -> i64 {
        value.unix_milliseconds()
    }

    /// Reconstructs a domain-event time from Unix milliseconds.
    pub fn decode_event_time(value: i64) -> Result<DomainEventTime, PersistenceError> {
        DomainEventTime::from_unix_milliseconds(value)
            .map_err(|_| invalid("domain_events.occurred_at_unix_ms"))
    }

    /// Splits an aggregate identifier into its PostgreSQL kind and UUID.
    #[must_use]
    pub const fn encode_aggregate_id(value: DomainAggregateId) -> (&'static str, Uuid) {
        match value {
            DomainAggregateId::TransitAccount(id) => ("transit_account", id.into_uuid()),

            DomainAggregateId::FareCredential(id) => ("fare_credential", id.into_uuid()),

            DomainAggregateId::ReaderEquipment(id) => ("reader_equipment", id.into_uuid()),

            DomainAggregateId::FareTransaction(id) => ("fare_transaction", id.into_uuid()),
        }
    }

    /// Reconstructs a strongly typed aggregate identifier.
    pub fn decode_aggregate_id(
        kind: &str,
        identifier: Uuid,
    ) -> Result<DomainAggregateId, PersistenceError> {
        match kind {
            "transit_account" => TransitAccountId::try_from(identifier)
                .map(DomainAggregateId::TransitAccount)
                .map_err(|_| invalid("domain_events.aggregate_id")),

            "fare_credential" => FareCredentialId::try_from(identifier)
                .map(DomainAggregateId::FareCredential)
                .map_err(|_| invalid("domain_events.aggregate_id")),

            "reader_equipment" => ReaderId::try_from(identifier)
                .map(DomainAggregateId::ReaderEquipment)
                .map_err(|_| invalid("domain_events.aggregate_id")),

            "fare_transaction" => FareTransactionId::try_from(identifier)
                .map(DomainAggregateId::FareTransaction)
                .map_err(|_| invalid("domain_events.aggregate_id")),

            _ => Err(invalid("domain_events.aggregate_kind")),
        }
    }

    /// Converts a reader identifier to its PostgreSQL UUID.
    #[must_use]
    pub const fn encode_reader_id(value: ReaderId) -> Uuid {
        value.into_uuid()
    }

    /// Converts an equipment-key identifier to its PostgreSQL UUID.
    #[must_use]
    pub const fn encode_equipment_key_id(value: EquipmentKeyId) -> Uuid {
        value.into_uuid()
    }
}

const fn invalid(field: &'static str) -> PersistenceError {
    PersistenceError::InvalidStoredValue { field }
}

const fn numeric_out_of_range(field: &'static str) -> PersistenceError {
    PersistenceError::NumericValueOutOfRange { field }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        AggregateVersion, Currency, DomainAggregateId, DomainEventTime, EligibilityClassification,
        FareCredentialId, FareCredentialKind, FareCredentialStatus, FareTransactionId,
        ReaderDisablementReason, ReaderEquipmentStatus, ReaderId, ReaderRevocationReason,
        RevocationReason, TransitAccountId, TransitAccountStatus,
    };
    use uuid::Uuid;

    use super::PostgresValueCodec;
    use crate::PersistenceError;

    fn version(value: u64) -> AggregateVersion {
        match AggregateVersion::new(value) {
            Ok(version) => version,

            Err(error) => {
                panic!(
                    "valid aggregate version failed: \
                     {error}"
                )
            }
        }
    }

    fn event_time(value: i64) -> DomainEventTime {
        match DomainEventTime::from_unix_milliseconds(value) {
            Ok(time) => time,

            Err(error) => {
                panic!(
                    "valid domain-event time failed: \
                     {error}"
                )
            }
        }
    }

    #[test]
    fn currencies_round_trip() {
        for value in [Currency::Usd, Currency::Cad, Currency::Eur, Currency::Gbp] {
            let encoded = PostgresValueCodec::encode_currency(value);

            let decoded = PostgresValueCodec::decode_currency(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn account_statuses_round_trip() {
        for value in [
            TransitAccountStatus::Active,
            TransitAccountStatus::Suspended,
            TransitAccountStatus::Closed,
        ] {
            let encoded = PostgresValueCodec::encode_account_status(value);

            let decoded = PostgresValueCodec::decode_account_status(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn eligibility_values_round_trip() {
        for value in [
            EligibilityClassification::Standard,
            EligibilityClassification::Youth,
            EligibilityClassification::Senior,
            EligibilityClassification::ReducedFare,
            EligibilityClassification::EmployeeTestAccount,
        ] {
            let encoded = PostgresValueCodec::encode_eligibility(value);

            let decoded = PostgresValueCodec::decode_eligibility(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn credential_kinds_round_trip() {
        for value in [
            FareCredentialKind::Card,
            FareCredentialKind::Mobile,
            FareCredentialKind::DevelopmentTestToken,
        ] {
            let encoded = PostgresValueCodec::encode_credential_kind(value);

            let decoded = PostgresValueCodec::decode_credential_kind(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn credential_statuses_round_trip() {
        for value in [
            FareCredentialStatus::Pending,
            FareCredentialStatus::Active,
            FareCredentialStatus::Suspended,
            FareCredentialStatus::Revoked,
            FareCredentialStatus::Expired,
            FareCredentialStatus::Replaced,
        ] {
            let encoded = PostgresValueCodec::encode_credential_status(value);

            let decoded = PostgresValueCodec::decode_credential_status(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn credential_revocation_reasons_round_trip() {
        for value in [
            RevocationReason::ReportedLost,
            RevocationReason::ReportedStolen,
            RevocationReason::Replaced,
            RevocationReason::AccountSuspended,
            RevocationReason::AdministrativeAction,
            RevocationReason::SecurityIncident,
            RevocationReason::TestCleanup,
        ] {
            let encoded = PostgresValueCodec::encode_credential_revocation_reason(value);

            let decoded = PostgresValueCodec::decode_credential_revocation_reason(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn reader_statuses_round_trip() {
        for value in [
            ReaderEquipmentStatus::PendingRegistration,
            ReaderEquipmentStatus::Active,
            ReaderEquipmentStatus::Offline,
            ReaderEquipmentStatus::Disabled,
            ReaderEquipmentStatus::Revoked,
            ReaderEquipmentStatus::Decommissioned,
        ] {
            let encoded = PostgresValueCodec::encode_reader_status(value);

            let decoded = PostgresValueCodec::decode_reader_status(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn reader_disablement_reasons_round_trip() {
        for value in [
            ReaderDisablementReason::SuspectedCompromise,
            ReaderDisablementReason::LostEquipment,
            ReaderDisablementReason::InvalidConfiguration,
            ReaderDisablementReason::AdministrativeAction,
            ReaderDisablementReason::TestCleanup,
        ] {
            let encoded = PostgresValueCodec::encode_reader_disablement_reason(value);

            let decoded = PostgresValueCodec::decode_reader_disablement_reason(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn reader_revocation_reasons_round_trip() {
        for value in [
            ReaderRevocationReason::SuspectedCompromise,
            ReaderRevocationReason::CredentialExposure,
            ReaderRevocationReason::AdministrativeAction,
            ReaderRevocationReason::SecurityIncident,
            ReaderRevocationReason::TestCleanup,
        ] {
            let encoded = PostgresValueCodec::encode_reader_revocation_reason(value);

            let decoded = PostgresValueCodec::decode_reader_revocation_reason(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == value
            ));
        }
    }

    #[test]
    fn aggregate_versions_round_trip() {
        for value in [1, 2, 42, i64::MAX as u64] {
            let original = version(value);

            let encoded = PostgresValueCodec::encode_aggregate_version(original);

            let encoded = match encoded {
                Ok(encoded) => encoded,

                Err(error) => {
                    panic!(
                        "valid version encoding failed: \
                         {error}"
                    )
                }
            };

            let decoded = PostgresValueCodec::decode_aggregate_version(encoded);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == original
            ));
        }
    }

    #[test]
    fn aggregate_version_overflow_is_rejected() {
        let result = PostgresValueCodec::encode_aggregate_version(version(u64::MAX));

        assert!(matches!(
            result,
            Err(PersistenceError::NumericValueOutOfRange {
                field: "aggregate_version"
            })
        ));
    }

    #[test]
    fn invalid_stored_versions_are_rejected() {
        let zero = PostgresValueCodec::decode_aggregate_version(0);

        assert!(matches!(
            zero,
            Err(PersistenceError::InvalidStoredValue {
                field: "aggregate_version"
            })
        ));

        let negative = PostgresValueCodec::decode_aggregate_version(-1);

        assert!(matches!(
            negative,
            Err(PersistenceError::NumericValueOutOfRange {
                field: "aggregate_version"
            })
        ));
    }

    #[test]
    fn event_times_round_trip() {
        let original = event_time(1_700_000_000_000);

        let encoded = PostgresValueCodec::encode_event_time(original);

        let decoded = PostgresValueCodec::decode_event_time(encoded);

        assert!(matches!(
            decoded,
            Ok(decoded) if decoded == original
        ));
    }

    #[test]
    fn negative_event_time_is_rejected() {
        let result = PostgresValueCodec::decode_event_time(-1);

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "domain_events.\
                             occurred_at_unix_ms"
            })
        ));
    }

    #[test]
    fn aggregate_identifiers_round_trip() {
        for original in [
            DomainAggregateId::TransitAccount(TransitAccountId::generate()),
            DomainAggregateId::FareCredential(FareCredentialId::generate()),
            DomainAggregateId::ReaderEquipment(ReaderId::generate()),
            DomainAggregateId::FareTransaction(FareTransactionId::generate()),
        ] {
            let (kind, identifier) = PostgresValueCodec::encode_aggregate_id(original);

            let decoded = PostgresValueCodec::decode_aggregate_id(kind, identifier);

            assert!(matches!(
                decoded,
                Ok(decoded) if decoded == original
            ));
        }
    }

    #[test]
    fn unknown_aggregate_kind_is_rejected() {
        let result = PostgresValueCodec::decode_aggregate_id(
            "unknown",
            TransitAccountId::generate().into_uuid(),
        );

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "domain_events.\
                             aggregate_kind"
            })
        ));
    }

    #[test]
    fn invalid_aggregate_identifier_is_rejected() {
        let result = PostgresValueCodec::decode_aggregate_id("transit_account", Uuid::nil());

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "domain_events.\
                             aggregate_id"
            })
        ));
    }

    #[test]
    fn invalid_enum_value_is_rejected() {
        let result = PostgresValueCodec::decode_credential_status("unexpected_status");

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.status"
            })
        ));
    }
}
