use std::{
    env,
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use transitguard_persistence::{
    MAX_RECONCILIATION_WORK_BATCH_SIZE, PostgresConfig, PostgresReconciliationWorkQueue,
    connect_postgres, run_postgres_migrations,
};

const DEFAULT_MAINTENANCE_LIMIT: u16 = 64;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = env::var("DATABASE_URL")?;

    let postgres_config = PostgresConfig::new(database_url)?;

    let pool = connect_postgres(&postgres_config).await?;

    run_postgres_migrations(&pool).await?;

    let queue = PostgresReconciliationWorkQueue::new(pool.clone());

    let now_unix_milliseconds = current_unix_milliseconds()?;

    let enqueued = queue
        .enqueue_ready(now_unix_milliseconds, DEFAULT_MAINTENANCE_LIMIT)
        .await?;

    let recovered = queue
        .recover_expired(now_unix_milliseconds, DEFAULT_MAINTENANCE_LIMIT)
        .await?;

    println!(
        "TransitGuard reconciliation worker foundation ready: \
         enqueued={enqueued} recovered={recovered} \
         maintenance_limit={DEFAULT_MAINTENANCE_LIMIT} \
         queue_maximum={MAX_RECONCILIATION_WORK_BATCH_SIZE}"
    );

    println!(
        "Reconciliation claim processing remains disabled \
         until the authoritative fare-evaluation processor is wired."
    );

    pool.close().await;

    Ok(())
}

fn current_unix_milliseconds() -> Result<i64, Box<dyn Error>> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;

    let milliseconds = i64::try_from(elapsed.as_millis())?;

    Ok(milliseconds)
}
