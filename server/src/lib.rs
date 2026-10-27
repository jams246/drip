//! SQLite-backed enrollment, device administration, and selection barriers.

mod admin;
mod api_error;
mod auth;
mod database;
mod enrollment;
mod http;
mod mirror;
mod selections;
mod store;

#[cfg(test)]
mod selection_mutation_tests;

pub use api_error::ApiFailure;
pub use http::{router, serve};
pub use store::{DeviceIdentity, DeviceRecord, Store, StoreError, StoreResult};
