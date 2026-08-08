use std::{env, fmt::Display};

use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions, types::Json};
use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
    FarePolicyVersion, FareTransactionId, Money, ReaderId, SynchronizationBatchId,
};
use transitguard_persistence::{
    PostgresReconciliationRepository, PreparedReconciliationPersistence,
    ReconciliationPersistenceDisposition, ReconciliationRepositoryError, run_postgres_migrations,
};
use transitguard_reconciliation::{
    ReconciliationDecision, ReconciliationEvidence, ReconciliationId,
    ReconciliationProductEvidence, ReconciliationRecord, ReconciliationTime,
};
use uuid::Uuid;

const SOURCE_TIME: i64 = 1_700_000_100_000;
const RECONCILIATION_TIME: i64 = 1_700_000_200_000;

#[derive(Clone, Copy, Debug)]
struct SourceFixture {
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    transaction_id: FareTransactionId,
}

fn must<T, E>(result: Result<T, E>, operation: &str) -> T
where
    E: Display,
{
    match result {
        Ok(value) => value,

        Err(error) => {
            panic!("{operation} failed: {error}")
        }
    }
}

fn test_database_url() -> String {
    match env::var("TRANSITGUARD_TEST_DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,

        _ => {
            panic!(
                "TRANSITGUARD_TEST_DATABASE_URL must reference \
                 the isolated PostgreSQL test database"
            )
        }
    }
}

fn policy_version() -> FarePolicyVersion {
    must(FarePolicyVersion::new(1), "create fare policy version")
}

fn event_time() -> EventTime {
    must(
        EventTime::from_unix_milliseconds(1_700_000_000_000),
        "create fare event time",
    )
}

fn reconciliation_time() -> ReconciliationTime {
    must(
        ReconciliationTime::from_unix_milliseconds(RECONCILIATION_TIME),
        "create reconciliation time",
    )
}

fn evidence(policy_id: FarePolicyId, minor_units: i64) -> ReconciliationEvidence {
    let amount = Money::from_minor_units(minor_units, Currency::Usd);

    ReconciliationEvidence::test_fixture(
        policy_id,
        policy_version(),
        event_time(),
        ReconciliationDecision::Approved {
            charged_amount: amount,
            reason: FareApprovalReason::StandardFare,
        },
        EligibilityClassification::Standard,
        Money::zero(Currency::Usd),
        false,
        Money::zero(Currency::Usd),
        Money::zero(Currency::Usd),
        false,
        false,
        ReconciliationProductEvidence::NotPresented,
        Money::zero(Currency::Usd),
        amount,
    )
}

fn reconciliation_record(
    id: ReconciliationId,
    source: SourceFixture,
    reader_id: ReaderId,
    source_batch_id: Option<SynchronizationBatchId>,
    reader_evidence: ReconciliationEvidence,
    backend_evidence: ReconciliationEvidence,
) -> ReconciliationRecord {
    must(
        ReconciliationRecord::create(
            id,
            source.transaction_id,
            source_batch_id,
            reader_id,
            reader_evidence,
            backend_evidence,
            reconciliation_time(),
        ),
        "create reconciliation record",
    )
}

fn prepared(
    record: ReconciliationRecord,
    reader_evidence: ReconciliationEvidence,
    backend_evidence: ReconciliationEvidence,
) -> PreparedReconciliationPersistence {
    must(
        PreparedReconciliationPersistence::prepare(record, reader_evidence, backend_evidence),
        "prepare reconciliation persistence",
    )
}

async fn seed_source(pool: &PgPool) -> SourceFixture {
    let reader_id = ReaderId::generate();
    let batch_id = SynchronizationBatchId::generate();
    let transaction_id = FareTransactionId::generate();

    must(
        sqlx::query(
            r#"
            INSERT INTO reader_equipment (
                id,
                equipment_key_id,
                status,
                disablement_reason,
                revocation_reason,
                aggregate_version
            )
            VALUES (
                $1,
                $2,
                'active',
                NULL,
                NULL,
                1
            )
            "#,
        )
        .bind(reader_id.into_uuid())
        .bind(Uuid::now_v7())
        .execute(pool)
        .await,
        "seed reader equipment",
    );

    must(
        sqlx::query(
            r#"
            INSERT INTO synchronization_ingest_batches (
                batch_id,
                reader_id,
                protocol_version,
                environment_id,
                reader_software_version,
                first_local_sequence_number,
                last_local_sequence_number,
                submitted_at_unix_milliseconds,
                received_at_unix_milliseconds,
                entry_count,
                request_fingerprint,
                canonical_request_json,
                acknowledgement_fingerprint,
                canonical_acknowledgement_json
            )
            VALUES (
                $1,
                $2,
                1,
                'phase8-integration-test',
                'repository-test',
                1,
                1,
                $3,
                $3,
                1,
                $4,
                $5,
                $6,
                $7
            )
            "#,
        )
        .bind(batch_id.into_uuid())
        .bind(reader_id.into_uuid())
        .bind(SOURCE_TIME)
        .bind("a".repeat(64))
        .bind(Json(json!({})))
        .bind("b".repeat(64))
        .bind(Json(json!({})))
        .execute(pool)
        .await,
        "seed synchronization batch",
    );

    must(
        sqlx::query(
            r#"
            INSERT INTO synchronization_ingest_transactions (
                fare_transaction_id,
                reader_id,
                local_sequence_number,
                transaction_fingerprint,
                canonical_transaction_envelope_json,
                first_seen_batch_id,
                current_resolution,
                first_received_at_unix_milliseconds,
                last_resolved_at_unix_milliseconds
            )
            VALUES (
                $1,
                $2,
                1,
                $3,
                $4,
                $5,
                'acknowledged',
                $6,
                $6
            )
            "#,
        )
        .bind(transaction_id.into_uuid())
        .bind(reader_id.into_uuid())
        .bind("c".repeat(64))
        .bind(Json(json!({})))
        .bind(batch_id.into_uuid())
        .bind(SOURCE_TIME)
        .execute(pool)
        .await,
        "seed synchronized transaction",
    );

    must(
        sqlx::query(
            r#"
            INSERT INTO synchronization_ingest_entries (
                batch_id,
                reader_id,
                entry_position,
                fare_transaction_id,
                local_sequence_number,
                outcome,
                failure_category,
                next_retry_at_unix_milliseconds,
                resolved_at_unix_milliseconds
            )
            VALUES (
                $1,
                $2,
                0,
                $3,
                1,
                'acknowledged',
                NULL,
                NULL,
                $4
            )
            "#,
        )
        .bind(batch_id.into_uuid())
        .bind(reader_id.into_uuid())
        .bind(transaction_id.into_uuid())
        .bind(SOURCE_TIME)
        .execute(pool)
        .await,
        "seed synchronization entry",
    );

    SourceFixture {
        reader_id,
        batch_id,
        transaction_id,
    }
}

async fn reconciliation_count(pool: &PgPool, transaction_id: FareTransactionId) -> i64 {
    must(
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM reconciliation_records
            WHERE fare_transaction_id = $1
            "#,
        )
        .bind(transaction_id.into_uuid())
        .fetch_one(pool)
        .await,
        "count reconciliation records",
    )
}

async fn discrepancy_count(pool: &PgPool, transaction_id: FareTransactionId) -> i64 {
    must(
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM reconciliation_discrepancy_cases
            WHERE fare_transaction_id = $1
            "#,
        )
        .bind(transaction_id.into_uuid())
        .fetch_one(pool)
        .await,
        "count discrepancy cases",
    )
}

async fn adjustment_count(pool: &PgPool, transaction_id: FareTransactionId) -> i64 {
    must(
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM reconciliation_proposed_adjustments
            WHERE fare_transaction_id = $1
            "#,
        )
        .bind(transaction_id.into_uuid())
        .fetch_one(pool)
        .await,
        "count proposed adjustments",
    )
}

async fn assert_no_reconciliation_rows(pool: &PgPool, transaction_id: FareTransactionId) {
    assert_eq!(reconciliation_count(pool, transaction_id).await, 0);

    assert_eq!(discrepancy_count(pool, transaction_id).await, 0);

    assert_eq!(adjustment_count(pool, transaction_id).await, 0);
}

async fn validate_concurrent_store_replay_and_reload(pool: &PgPool) {
    let source = seed_source(pool).await;
    let policy_id = FarePolicyId::generate();

    let reader_evidence = evidence(policy_id, 250);
    let backend_evidence = evidence(policy_id, 300);

    let reconciliation_id = ReconciliationId::generate();

    let record = reconciliation_record(
        reconciliation_id,
        source,
        source.reader_id,
        Some(source.batch_id),
        reader_evidence,
        backend_evidence,
    );

    let first_prepared = prepared(record, reader_evidence, backend_evidence);

    let second_prepared = prepared(record, reader_evidence, backend_evidence);

    let first_repository = PostgresReconciliationRepository::new(pool.clone());

    let second_repository = PostgresReconciliationRepository::new(pool.clone());

    let (first, second) = tokio::join!(
        first_repository.store(&first_prepared),
        second_repository.store(&second_prepared),
    );

    let first = must(first, "first concurrent reconciliation store");

    let second = must(second, "second concurrent reconciliation store");

    assert!(matches!(
        (first, second),
        (
            ReconciliationPersistenceDisposition::Stored,
            ReconciliationPersistenceDisposition::Replayed
        ) | (
            ReconciliationPersistenceDisposition::Replayed,
            ReconciliationPersistenceDisposition::Stored
        )
    ));

    assert_eq!(reconciliation_count(pool, source.transaction_id).await, 1);

    assert_eq!(discrepancy_count(pool, source.transaction_id).await, 1);

    assert_eq!(adjustment_count(pool, source.transaction_id).await, 1);

    let repository = PostgresReconciliationRepository::new(pool.clone());

    let loaded_by_transaction = must(
        repository
            .load_by_transaction_id(source.transaction_id)
            .await,
        "load reconciliation by transaction",
    );

    let loaded_by_transaction = match loaded_by_transaction {
        Some(value) => value,

        None => {
            panic!("stored reconciliation was not found by transaction")
        }
    };

    assert_eq!(loaded_by_transaction.record(), record);

    assert_eq!(loaded_by_transaction.reader_evidence(), reader_evidence);

    assert_eq!(loaded_by_transaction.backend_evidence(), backend_evidence);

    let loaded_by_id = must(
        repository.load_by_id(reconciliation_id).await,
        "load reconciliation by identity",
    );

    assert!(matches!(
        loaded_by_id,
        Some(value) if value == loaded_by_transaction
    ));

    let conflicting_backend = evidence(policy_id, 350);

    let conflicting_record = reconciliation_record(
        reconciliation_id,
        source,
        source.reader_id,
        Some(source.batch_id),
        reader_evidence,
        conflicting_backend,
    );

    let conflicting_prepared = prepared(conflicting_record, reader_evidence, conflicting_backend);

    let conflict = repository.store(&conflicting_prepared).await;

    assert!(matches!(
        conflict,
        Err(
            ReconciliationRepositoryError::IdentityConflict {
                transaction_id
            }
        ) if transaction_id == source.transaction_id
    ));

    assert_eq!(reconciliation_count(pool, source.transaction_id).await, 1);

    assert_eq!(discrepancy_count(pool, source.transaction_id).await, 1);

    assert_eq!(adjustment_count(pool, source.transaction_id).await, 1);
}

async fn validate_source_provenance(pool: &PgPool) {
    let policy_id = FarePolicyId::generate();
    let reader_evidence = evidence(policy_id, 250);
    let backend_evidence = evidence(policy_id, 300);

    let missing_source = SourceFixture {
        reader_id: ReaderId::generate(),
        batch_id: SynchronizationBatchId::generate(),
        transaction_id: FareTransactionId::generate(),
    };

    let record = reconciliation_record(
        ReconciliationId::generate(),
        missing_source,
        missing_source.reader_id,
        None,
        reader_evidence,
        backend_evidence,
    );

    let value = prepared(record, reader_evidence, backend_evidence);

    let repository = PostgresReconciliationRepository::new(pool.clone());

    let result = repository.store(&value).await;

    assert!(matches!(
        result,
        Err(
            ReconciliationRepositoryError::SourceTransactionNotFound {
                transaction_id
            }
        ) if transaction_id == missing_source.transaction_id
    ));

    assert_no_reconciliation_rows(pool, missing_source.transaction_id).await;

    let source = seed_source(pool).await;

    let wrong_reader = ReaderId::generate();

    let record = reconciliation_record(
        ReconciliationId::generate(),
        source,
        wrong_reader,
        Some(source.batch_id),
        reader_evidence,
        backend_evidence,
    );

    let value = prepared(record, reader_evidence, backend_evidence);

    let result = repository.store(&value).await;

    assert!(matches!(
        result,
        Err(
            ReconciliationRepositoryError::SourceReaderConflict {
                transaction_id
            }
        ) if transaction_id == source.transaction_id
    ));

    assert_no_reconciliation_rows(pool, source.transaction_id).await;

    let source = seed_source(pool).await;

    let wrong_batch = SynchronizationBatchId::generate();

    let record = reconciliation_record(
        ReconciliationId::generate(),
        source,
        source.reader_id,
        Some(wrong_batch),
        reader_evidence,
        backend_evidence,
    );

    let value = prepared(record, reader_evidence, backend_evidence);

    let result = repository.store(&value).await;

    assert!(matches!(
        result,
        Err(
            ReconciliationRepositoryError::SourceBatchConflict {
                batch_id,
                transaction_id
            }
        )
            if batch_id == wrong_batch
                && transaction_id == source.transaction_id
    ));

    assert_no_reconciliation_rows(pool, source.transaction_id).await;
}

async fn validate_transaction_rollback(pool: &PgPool) {
    let source = seed_source(pool).await;
    let policy_id = FarePolicyId::generate();

    let reader_evidence = evidence(policy_id, 250);
    let backend_evidence = evidence(policy_id, 300);

    let record = reconciliation_record(
        ReconciliationId::generate(),
        source,
        source.reader_id,
        Some(source.batch_id),
        reader_evidence,
        backend_evidence,
    );

    let value = prepared(record, reader_evidence, backend_evidence);

    must(
        sqlx::query(
            r#"
            CREATE FUNCTION
                phase8_reject_proposed_adjustment()
            RETURNS TRIGGER
            LANGUAGE plpgsql
            AS $$
            BEGIN
                RAISE EXCEPTION
                    'forced Phase 8 adjustment failure';
            END
            $$
            "#,
        )
        .execute(pool)
        .await,
        "create rollback test function",
    );

    must(
        sqlx::query(
            r#"
            CREATE TRIGGER
                phase8_reject_proposed_adjustment_trigger
            BEFORE INSERT
            ON reconciliation_proposed_adjustments
            FOR EACH ROW
            EXECUTE FUNCTION
                phase8_reject_proposed_adjustment()
            "#,
        )
        .execute(pool)
        .await,
        "create rollback test trigger",
    );

    let repository = PostgresReconciliationRepository::new(pool.clone());

    let result = repository.store(&value).await;

    assert!(result.is_err());

    must(
        sqlx::query(
            r#"
            DROP TRIGGER
                phase8_reject_proposed_adjustment_trigger
            ON reconciliation_proposed_adjustments
            "#,
        )
        .execute(pool)
        .await,
        "drop rollback test trigger",
    );

    must(
        sqlx::query(
            r#"
            DROP FUNCTION
                phase8_reject_proposed_adjustment()
            "#,
        )
        .execute(pool)
        .await,
        "drop rollback test function",
    );

    assert_no_reconciliation_rows(pool, source.transaction_id).await;

    let loaded = must(
        repository
            .load_by_transaction_id(source.transaction_id)
            .await,
        "load rolled back reconciliation",
    );

    assert!(loaded.is_none());
}

#[tokio::test]
async fn postgres_reconciliation_repository_contract() {
    let database_url = test_database_url();

    let pool = must(
        PgPoolOptions::new()
            .min_connections(1)
            .max_connections(8)
            .connect(&database_url)
            .await,
        "connect isolated PostgreSQL",
    );

    if let Err(error) = run_postgres_migrations(&pool).await {
        panic!("run PostgreSQL migrations failed: {error:?}");
    }

    validate_concurrent_store_replay_and_reload(&pool).await;

    validate_source_provenance(&pool).await;

    validate_transaction_rollback(&pool).await;

    pool.close().await;
}
