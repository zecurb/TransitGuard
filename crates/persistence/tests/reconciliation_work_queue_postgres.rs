use std::{collections::HashSet, env, fmt::Display};

use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions, types::Json};
use transitguard_domain::{FareTransactionId, ReaderId, SynchronizationBatchId};
use transitguard_persistence::{
    ClaimedReconciliationWork, PostgresReconciliationWorkQueue, ReconciliationWorkQueueError,
    ReconciliationWorkerId, run_postgres_migrations,
};
use uuid::Uuid;

const SOURCE_TIME: i64 = 1_700_100_000_000;
const QUEUE_TIME: i64 = 1_700_100_001_000;
const INITIAL_CLAIM_TIME: i64 = 1_700_100_002_000;
const INITIAL_LEASE_DURATION: i64 = 1_000;

#[derive(Clone, Debug)]
struct SourceFixture {
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
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
                "TRANSITGUARD_TEST_DATABASE_URL must reference \
                 the isolated PostgreSQL test database"
            )
        }
    }
}

async fn seed_acknowledged_transactions(pool: &PgPool, count: usize) -> SourceFixture {
    assert!(count > 0);
    assert!(count <= 256);

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
        "seed reconciliation queue reader",
    );

    let count_i64 = match i64::try_from(count) {
        Ok(value) => value,

        Err(error) => {
            panic!("test source count does not fit i64: {error}")
        }
    };

    let count_i32 = match i32::try_from(count) {
        Ok(value) => value,

        Err(error) => {
            panic!("test source count does not fit i32: {error}")
        }
    };

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
                'phase8-work-queue-integration',
                'queue-integration-test',
                1,
                $3,
                $4,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9
            )
            "#,
        )
        .bind(batch_id.into_uuid())
        .bind(reader_id.into_uuid())
        .bind(count_i64)
        .bind(SOURCE_TIME)
        .bind(count_i32)
        .bind("a".repeat(64))
        .bind(Json(json!({})))
        .bind("b".repeat(64))
        .bind(Json(json!({})))
        .execute(pool)
        .await,
        "seed reconciliation queue synchronization batch",
    );

    let mut transaction_ids = Vec::with_capacity(count);

    for position in 0..count {
        let transaction_id = FareTransactionId::generate();

        let sequence = match i64::try_from(position + 1) {
            Ok(value) => value,

            Err(error) => {
                panic!("test sequence does not fit i64: {error}")
            }
        };

        let fingerprint = format!("{sequence:064x}");

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
            .bind(fingerprint)
            .bind(Json(json!({})))
            .bind(batch_id.into_uuid())
            .bind(SOURCE_TIME + sequence)
            .execute(pool)
            .await,
            "seed acknowledged synchronization transaction",
        );

        transaction_ids.push(transaction_id);
    }

    SourceFixture {
        reader_id,
        batch_id,
        transaction_ids,
    }
}

fn transaction_set(claims: &[ClaimedReconciliationWork]) -> HashSet<FareTransactionId> {
    claims.iter().map(|claim| claim.transaction_id()).collect()
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
        "count reconciliation work state",
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
        "load reconciliation attempt count",
    )
}

fn assert_lease_lost(
    result: Result<(), ReconciliationWorkQueueError>,
    expected_transaction_id: FareTransactionId,
) {
    assert!(matches!(
        result,
        Err(
            ReconciliationWorkQueueError::LeaseLost {
                transaction_id
            }
        ) if transaction_id == expected_transaction_id
    ));
}

#[tokio::test]
async fn reconciliation_work_queue_concurrency_contract() {
    let database_url = test_database_url();

    let pool = must(
        PgPoolOptions::new()
            .min_connections(1)
            .max_connections(12)
            .connect(&database_url)
            .await,
        "connect isolated PostgreSQL",
    );

    must(
        run_postgres_migrations(&pool).await,
        "run reconciliation work queue migrations",
    );

    let source = seed_acknowledged_transactions(&pool, 6).await;

    assert_eq!(source.transaction_ids.len(), 6);

    let queue = PostgresReconciliationWorkQueue::new(pool.clone());

    /*
     * Bootstrap is bounded and idempotent.
     */
    assert_eq!(
        must(
            queue.enqueue_ready(QUEUE_TIME, 2,).await,
            "enqueue first bounded reconciliation work batch",
        ),
        2
    );

    assert_eq!(
        must(
            queue.enqueue_ready(QUEUE_TIME, 2,).await,
            "enqueue second bounded reconciliation work batch",
        ),
        2
    );

    assert_eq!(
        must(
            queue.enqueue_ready(QUEUE_TIME, 10,).await,
            "enqueue remaining reconciliation work",
        ),
        2
    );

    assert_eq!(
        must(
            queue.enqueue_ready(QUEUE_TIME, 10,).await,
            "verify reconciliation enqueue idempotency",
        ),
        0
    );

    assert_eq!(state_count(&pool, "pending").await, 6);

    /*
     * Two workers concurrently claim the six available rows.
     *
     * Each claim uses PostgreSQL FOR UPDATE SKIP LOCKED.
     */
    let worker_a = ReconciliationWorkerId::generate();

    let worker_b = ReconciliationWorkerId::generate();

    let queue_a = PostgresReconciliationWorkQueue::new(pool.clone());

    let queue_b = PostgresReconciliationWorkQueue::new(pool.clone());

    let (claims_a, claims_b) = tokio::join!(
        queue_a.claim_ready(worker_a, INITIAL_CLAIM_TIME, INITIAL_LEASE_DURATION, 3,),
        queue_b.claim_ready(worker_b, INITIAL_CLAIM_TIME, INITIAL_LEASE_DURATION, 3,),
    );

    let claims_a = must(claims_a, "worker A concurrent claim");

    let claims_b = must(claims_b, "worker B concurrent claim");

    assert_eq!(claims_a.len(), 3);
    assert_eq!(claims_b.len(), 3);

    let set_a = transaction_set(&claims_a);
    let set_b = transaction_set(&claims_b);

    assert!(
        set_a.is_disjoint(&set_b),
        "concurrent workers claimed overlapping transactions"
    );

    let all_claimed: HashSet<_> = set_a.union(&set_b).copied().collect();

    assert_eq!(all_claimed.len(), 6);

    assert_eq!(state_count(&pool, "pending").await, 0);

    assert_eq!(state_count(&pool, "in_progress").await, 6);

    for claim in claims_a.iter().chain(&claims_b) {
        assert_eq!(claim.attempt_count(), 1);

        assert_eq!(claim.reader_id(), source.reader_id);

        assert_eq!(claim.source_batch_id(), source.batch_id);
    }

    /*
     * Complete one item normally.
     */
    let completed_claim = claims_a[0];

    must(
        queue
            .complete(
                completed_claim.transaction_id(),
                worker_a,
                INITIAL_CLAIM_TIME + 100,
            )
            .await,
        "complete actively leased reconciliation work",
    );

    assert_eq!(
        attempt_count(&pool, completed_claim.transaction_id()).await,
        1
    );

    /*
     * A different worker cannot touch completed work.
     */
    assert_lease_lost(
        queue
            .complete(
                completed_claim.transaction_id(),
                worker_b,
                INITIAL_CLAIM_TIME + 101,
            )
            .await,
        completed_claim.transaction_id(),
    );

    /*
     * Release one active item for retry.
     */
    let retry_claim = claims_a[1];
    let retry_available_at = INITIAL_CLAIM_TIME + 200;

    must(
        queue
            .retry(
                retry_claim.transaction_id(),
                worker_a,
                INITIAL_CLAIM_TIME + 100,
                retry_available_at,
            )
            .await,
        "release reconciliation work for retry",
    );

    assert_eq!(attempt_count(&pool, retry_claim.transaction_id()).await, 1);

    /*
     * Worker B takes the retry. The durable attempt increments to 2.
     */
    let retry_reclaimed = must(
        queue
            .claim_ready(worker_b, retry_available_at, 2_000, 1)
            .await,
        "reclaim retryable reconciliation work",
    );

    assert_eq!(retry_reclaimed.len(), 1);

    let retry_reclaimed = retry_reclaimed[0];

    assert_eq!(
        retry_reclaimed.transaction_id(),
        retry_claim.transaction_id()
    );

    assert_eq!(retry_reclaimed.attempt_count(), 2);

    /*
     * Worker A's old ownership can no longer mutate this transaction.
     */
    assert_lease_lost(
        queue
            .renew_lease(
                retry_claim.transaction_id(),
                worker_a,
                retry_available_at + 1,
                1_000,
            )
            .await,
        retry_claim.transaction_id(),
    );

    assert_lease_lost(
        queue
            .retry(
                retry_claim.transaction_id(),
                worker_a,
                retry_available_at + 1,
                retry_available_at + 500,
            )
            .await,
        retry_claim.transaction_id(),
    );

    assert_lease_lost(
        queue
            .complete(
                retry_claim.transaction_id(),
                worker_a,
                retry_available_at + 1,
            )
            .await,
        retry_claim.transaction_id(),
    );

    /*
     * Current owner can renew and complete normally.
     */
    must(
        queue
            .renew_lease(
                retry_claim.transaction_id(),
                worker_b,
                retry_available_at + 1,
                3_000,
            )
            .await,
        "renew current reconciliation lease",
    );

    must(
        queue
            .complete(
                retry_claim.transaction_id(),
                worker_b,
                retry_available_at + 100,
            )
            .await,
        "complete retried reconciliation work",
    );

    /*
     * Four original claims remain intentionally unfinished.
     * Simulate process termination by letting their leases expire.
     */
    let recovery_time = INITIAL_CLAIM_TIME + INITIAL_LEASE_DURATION + 1;

    let recovered = must(
        queue.recover_expired(recovery_time, 128).await,
        "recover expired reconciliation leases",
    );

    assert_eq!(recovered, 4);

    assert_eq!(state_count(&pool, "pending").await, 4);

    assert_eq!(state_count(&pool, "in_progress").await, 0);

    /*
     * A fresh worker represents the restarted process.
     */
    let restart_worker = ReconciliationWorkerId::generate();

    let restarted_claims = must(
        queue
            .claim_ready(restart_worker, recovery_time, 2_000, 4)
            .await,
        "claim restart-recovered reconciliation work",
    );

    assert_eq!(restarted_claims.len(), 4);

    for claim in &restarted_claims {
        assert_eq!(claim.attempt_count(), 2);

        /*
         * Determine which original worker held the expired lease.
         */
        let stale_worker = if set_a.contains(&claim.transaction_id()) {
            worker_a
        } else {
            worker_b
        };

        /*
         * The terminated/stale worker cannot complete the new owner's claim.
         */
        assert_lease_lost(
            queue
                .complete(claim.transaction_id(), stale_worker, recovery_time + 1)
                .await,
            claim.transaction_id(),
        );

        must(
            queue
                .complete(claim.transaction_id(), restart_worker, recovery_time + 1)
                .await,
            "complete restart-recovered reconciliation work",
        );
    }

    /*
     * The queue has converged. Every source transaction is completed exactly
     * once as durable work, although retried/recovered items retain attempts.
     */
    assert_eq!(state_count(&pool, "pending").await, 0);

    assert_eq!(state_count(&pool, "in_progress").await, 0);

    assert_eq!(state_count(&pool, "completed").await, 6);

    let total_rows = must(
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM reconciliation_work_items
            "#,
        )
        .fetch_one(&pool)
        .await,
        "count final reconciliation work rows",
    );

    assert_eq!(total_rows, 6);

    pool.close().await;
}
