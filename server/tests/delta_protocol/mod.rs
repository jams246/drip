mod chunk_lifecycle;
#[path = "../common/mod.rs"]
mod common;
mod expiry;
mod http_bounds;
mod plan_validation;
mod stale;
mod support;
mod transfer;

#[test]
fn fixture_names_differ_when_a_process_id_is_reused() {
    let previous = common::fixture_directory_name(1800, "protocol", 10, 0);
    let current = common::fixture_directory_name(1800, "protocol", 11, 0);
    assert_ne!(previous, current);
}
