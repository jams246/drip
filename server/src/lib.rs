//! SQLite-backed enrollment, device administration, and selection barriers.

mod admin;
mod api_error;
mod auth;
mod database;
mod directories;
mod enrollment;
mod entries;
mod entry_files;
mod http;
mod inventory;
mod metadata_revisions;
mod mirror;
mod selection_paths;
mod selections;
mod store;
mod upload_commit;
mod upload_data;
mod upload_exchange;
mod upload_files;
mod upload_metadata;
mod upload_paths;
mod upload_sessions;
mod upload_staging;
mod upload_store;
mod upload_times;

#[cfg(test)]
mod selection_mutation_tests;
#[cfg(test)]
mod upload_race_tests;
#[cfg(test)]
pub(crate) use upload_race_tests::common as upload_race_test_common;
#[cfg(test)]
mod entry_race_tests;
#[cfg(all(test, target_os = "linux"))]
mod exchange_fault_tests;
#[cfg(test)]
mod upload_session_tests;
#[cfg(test)]
mod upload_test_support;

pub use api_error::ApiFailure;
pub use http::{router, serve};
pub use store::{DeviceIdentity, DeviceRecord, Store, StoreError, StoreResult};
