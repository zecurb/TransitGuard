use std::{env, fmt::Display, io};

use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions, types::Json};
use transitguard_domain::{FareTransactionId, ReaderId, SynchronizationBatchId};
use transitguard_persistence::{
    PostgresReconciliationWorkQueue, ReconciliationWorkerId, run_postgres_migrations,
};
use transitguard_worker::{
    ReconciliationProcessDisposition, ReconciliationWorkerConfig, ReconciliationWorkerCycleError,
    run_reconciliation_worker_cycle,
};
use uuid::Uuid;

const SOURCE_TIME: i64 = 1_701_000_000_000;
const FIRST_CYCLE_TIME: i64 = 1_701_000_001_000;
const RETRY_AVAILABLE_TIME: i64 = FIRST_CYCLE_TIME + 500;
const LEASE_DURATION: i64 = 1_000;

#[derive(Clone, Debug)]
struct SourceFixture {
    transaction_ids: Vec<FareTransactionId>,
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
                "TRANSITGUARD_TEST_DATABASE_URL must \
                 reference the isolated PostgreSQL test database"
            )
        }
    }
}

async fn seed_source(pool: &PgPool) -> SourceFixture {
    let reader_id = ReaderId::generate();

    let batch_id = SynchronizationBatchId::generate();

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
        "seed reconciliation worker reader",
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
                'phase8-worker-integration',
                'worker-integration-test',
                1,
                4,
                $3,
                $3,
                4,
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
        "seed reconciliation worker batch",
    );

    let mut transaction_ids = Vec::with_capacity(4);

    for sequence in 1_i64..=4_i64 {
        let transaction_id = FareTransactionId::generate();

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
                    $3,
                    $4,
                    $5,
                    $6,
                    'acknowledged',
                    $7,
                    $7
                )
                "#,
            )
            .bind(transaction_id.into_uuid())
            .bind(reader_id.into_uuid())
            .bind(sequence)
            .bind(format!("{sequence:064x}"))
            .bind(Json(json!({})))
            .bind(batch_id.into_uuid())
            .bind(SOURCE_TIME + sequence)
            .execute(pool)
            .await,
            "seed acknowledged worker transaction",
        );

        transaction_ids.push(transaction_id);
    }

    SourceFixture { transaction_ids }
}

async fn state_count(pool: &PgPool, state: &str) -> i64 {
    must(
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM reconciliation_work_items
            WHERE state = $1
            "#,
        )
        .bind(state)
        .fetch_one(pool)
        .await,
        "count reconciliation worker state",
    )
}

async fn attempt_count(pool: &PgPool, transaction_id: FareTransactionId) -> i32 {
    must(
        sqlx::query_scalar::<_, i32>(
            r#"
            SELECT attempt_count
            FROM reconciliation_work_items
            WHERE fare_transaction_id = $1
            "#,
        )
        .bind(transaction_id.into_uuid())
        .fetch_one(pool)
        .await,
        "load worker attempt count",
    )
}

#[tokio::test]
async fn worker_cycle_is_restart_safe() {
    let database_url = test_database_url();

    let pool = must(
        PgPoolOptions::new()
            .min_connections(1)
            .max_connections(12)
            .connect(&database_url)
            .await,
        "connect worker PostgreSQL test database",
    );

    must(
        run_postgres_migrations(&pool).await,
        "run worker PostgreSQL migrations",
    );

    let source = seed_source(&pool).await;

    let queue = PostgresReconciliationWorkQueue::new(pool.clone());

    /*
     * First cycle:
     *
     * - enqueue all four;
     * - claim the first two;
     * - complete transaction 1;
     * - schedule transaction 2 for retry.
     */
    let worker_a = ReconciliationWorkerId::generate();

    let first_config = must(
        ReconciliationWorkerConfig::new(4, 4, 2, LEASE_DURATION),
        "create first worker configuration",
    );

    let first_id = source.transaction_ids[0];

    let second_id = source.transaction_ids[1];

    let first_report = must(
        run_reconciliation_worker_cycle(
            &queue,
            worker_a,
            FIRST_CYCLE_TIME,
            first_config,
            move |claim| async move {
                if claim.transaction_id() == first_id {
                    Ok::<_, io::Error>(ReconciliationProcessDisposition::Complete {
                        completed_at_unix_milliseconds: FIRST_CYCLE_TIME + 100,
                    })
                } else if claim.transaction_id() == second_id {
                    Ok::<_, io::Error>(ReconciliationProcessDisposition::Retry {
                        observed_at_unix_milliseconds: FIRST_CYCLE_TIME + 100,
                        available_at_unix_milliseconds: RETRY_AVAILABLE_TIME,
                    })
                } else {
                    Err(io::Error::other(
                        "unexpected transaction in first worker cycle",
                    ))
                }
            },
        )
        .await,
        "run first reconciliation worker cycle",
    );

    assert_eq!(first_report.enqueued(), 4);

    assert_eq!(first_report.recovered(), 0);

    assert_eq!(first_report.claimed(), 2);

    assert_eq!(first_report.completed(), 1);

    assert_eq!(first_report.retried(), 1);

    assert_eq!(state_count(&pool, "completed").await, 1);

    assert_eq!(state_count(&pool, "pending").await, 3);

    assert_eq!(state_count(&pool, "in_progress").await, 0);

    /*
     * Second worker claims transactions 3 and 4 before transaction 2's retry
     * becomes available.
     *
     * The simulated processor crashes on its first item. Because the batch was
     * already claimed, both rows remain durably leased.
     */
    let worker_b = ReconciliationWorkerId::generate();

    let second_cycle_time = FIRST_CYCLE_TIME + 200;

    let failure = run_reconciliation_worker_cycle(
        &queue,
        worker_b,
        second_cycle_time,
        first_config,
        |_claim| async {
            Err::<ReconciliationProcessDisposition, io::Error>(io::Error::other(
                "simulated reconciliation processor failure",
            ))
        },
    )
    .await;

    assert!(matches!(
        failure,
        Err(ReconciliationWorkerCycleError::Processor { .. })
    ));

    assert_eq!(state_count(&pool, "completed").await, 1);

    assert_eq!(state_count(&pool, "pending").await, 1);

    assert_eq!(state_count(&pool, "in_progress").await, 2);

    /*
     * Wait until worker B's leases expire.
     *
     * Transaction 2's deliberate retry is also available by this time.
     */
    let restart_time = second_cycle_time + LEASE_DURATION + 1;

    let restart_worker = ReconciliationWorkerId::generate();

    let restart_config = must(
        ReconciliationWorkerConfig::new(4, 4, 4, LEASE_DURATION),
        "create restart worker configuration",
    );

    let restart_report = must(
        run_reconciliation_worker_cycle(
            &queue,
            restart_worker,
            restart_time,
            restart_config,
            |_claim| async move {
                Ok::<_, io::Error>(ReconciliationProcessDisposition::Complete {
                    completed_at_unix_milliseconds: restart_time + 100,
                })
            },
        )
        .await,
        "run restart reconciliation worker cycle",
    );

    assert_eq!(restart_report.enqueued(), 0);

    assert_eq!(restart_report.recovered(), 2);

    assert_eq!(restart_report.claimed(), 3);

    assert_eq!(restart_report.completed(), 3);

    assert_eq!(restart_report.retried(), 0);

    assert_eq!(state_count(&pool, "pending").await, 0);

    assert_eq!(state_count(&pool, "in_progress").await, 0);

    assert_eq!(state_count(&pool, "completed").await, 4);

    /*
     * Original success required one attempt.
     */
    assert_eq!(attempt_count(&pool, source.transaction_ids[0],).await, 1);

    /*
     * Explicit retry required a second claim.
     */
    assert_eq!(attempt_count(&pool, source.transaction_ids[1],).await, 2);

    /*
     * Both rows leased by the failed process were restart-recovered and then
     * successfully claimed a second time.
     */
    assert_eq!(attempt_count(&pool, source.transaction_ids[2],).await, 2);

    assert_eq!(attempt_count(&pool, source.transaction_ids[3],).await, 2);

    pool.close().await;
}
