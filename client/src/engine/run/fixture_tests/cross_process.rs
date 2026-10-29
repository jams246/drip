mod harness;
mod worker;

#[path = "../../../../tests/common/deterministic.rs"]
mod deterministic;

#[tokio::test]
#[ignore = "requires the owned HTTPS fixture and an isolated copied test executable"]
async fn persistent_cache_process_restart_acceptance() {
    require_release_build();
    if let Some(stage) = std::env::var_os("DRIP_PERSISTENCE_STAGE") {
        worker::run(&stage.to_string_lossy()).await;
    } else {
        harness::run().await;
    }
}

#[test]
#[ignore = "continues the owned release dataset with an isolated test executable"]
fn persistent_cache_release_scenarios_acceptance() {
    require_release_build();
    harness::run_existing_release_scenarios();
}

fn require_release_build() {
    #[cfg(debug_assertions)]
    assert_ne!(
        std::env::var("DRIP_PERSISTENCE_DATASET").as_deref(),
        Ok("release"),
        "release acceptance requires cargo test --release"
    );
}
