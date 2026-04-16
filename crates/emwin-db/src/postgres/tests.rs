use super::connection::{connect_pool_call_count, reset_connect_pool_calls};
use super::{PostgresConfig, PostgresMetadataSink};

#[tokio::test]
async fn ensure_pool_single_flights_concurrent_initialization() {
    let Some(database_url) = std::env::var("EMWIN_PG_TEST_DATABASE_URL").ok() else {
        return;
    };

    reset_connect_pool_calls();

    let mut config = PostgresConfig::new(database_url);
    config.application_name = "emwin-db-unit-test".to_string();
    let sink = PostgresMetadataSink::new(config);

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let sink = sink.clone();
        tasks.push(tokio::spawn(async move {
            sink.ensure_pool()
                .await
                .expect("pool initialization should succeed");
        }));
    }

    for task in tasks {
        task.await.expect("task should join");
    }

    assert_eq!(connect_pool_call_count(), 1);
}
