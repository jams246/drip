//! JSON messages shared across `/v1/` control endpoints.

mod enrollment;
mod error;
mod selection;
mod upload;

pub use enrollment::{EnrollmentRequest, EnrollmentResponse};
pub use error::{ApiError, ApiErrorCode};
pub use selection::{Selection, SelectionSet, SelectionUpdate, SelectionUpdateResponse};
pub use upload::{
    Entry, EntryKind, EntryPage, UploadCommitResponse, UploadPrepareRequest, UploadPrepareResponse,
    UploadState, UploadStateResponse,
};
