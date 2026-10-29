mod harness;
mod worker;

#[path = "../../../../tests/common/deterministic.rs"]
mod deterministic;

#[tokio::test]
#[ignore = "requires the owned HTTPS fixture and an isolated copied test executable"]
async fn persistent_cache_process_restart_acceptance() {
    if let Some(stage) = std::env::var_os("DRIP_PERSISTENCE_STAGE") {
        worker::run(&stage.to_string_lossy()).await;
    } else {
        harness::run().await;
    }
}

#[test]
#[ignore = "continues the owned release dataset with an isolated test executable"]
fn persistent_cache_release_scenarios_acceptance() {
    harness::run_existing_release_scenarios();
}
