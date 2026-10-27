//! JSON messages shared across `/v1/` control endpoints.

mod enrollment;
mod entry_mutation;
mod error;
mod selection;
mod upload;

pub use enrollment::{EnrollmentRequest, EnrollmentResponse};
pub use entry_mutation::{
    DirectoryCreateRequest, DirectoryCreateResponse, EntryDeleteRequest, EntryDeleteResponse,
};
pub use error::{ApiError, ApiErrorCode};
pub use selection::{Selection, SelectionSet, SelectionUpdate, SelectionUpdateResponse};
pub use upload::{
    Entry, EntryKind, EntryPage, EntryPrecondition, UploadCommitResponse, UploadPrepareRequest,
    UploadPrepareResponse, UploadState, UploadStateResponse,
};
