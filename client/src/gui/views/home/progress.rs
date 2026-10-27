use drip_client::{SelectionProgress, UploadPhase};
use gpui::{div, prelude::*, rgb};

fn labels(progress: &SelectionProgress) -> (&'static str, String, String, String, String) {
    let file = &progress.file;
    let phase = match file.phase {
        UploadPhase::Hashing => "Hashing with BLAKE3",
        UploadPhase::Chunking => "Chunking / hashing with FastCDC + BLAKE3",
        UploadPhase::Uploading => "Uploading",
        UploadPhase::Verifying => "Verifying",
    };
    let stage_count = match file.total_bytes {
        Some(total) => format!("{} of {} bytes", file.processed_bytes, total),
        None => format!("{} bytes; total unknown", file.processed_bytes),
    };
    let hash_count = format!("BLAKE3 hashed: {} bytes", progress.source_bytes_processed);
    let chunk_count = format!(
        "FastCDC: {} bytes, {} chunks",
        file.source_chunked_bytes, file.chunk_count
    );
    let payload_count = format!(
        "Payload planned: {} bytes; sent: {} bytes",
        file.planned_payload_bytes, progress.uploaded_file_bytes
    );
    (phase, stage_count, hash_count, chunk_count, payload_count)
}

pub(super) fn render(progress: &SelectionProgress) -> impl IntoElement {
    let (phase, stage_count, hash_count, chunk_count, payload_count) = labels(progress);
    div()
        .id(format!("home-progress-{}", progress.operation_id))
        .flex()
        .flex_col()
        .gap_1()
        .pt_2()
        .text_sm()
        .text_color(rgb(0xc5d2dc))
        .child(div().child(format!("{phase}: {stage_count}")))
        .child(div().child(hash_count))
        .child(div().child(chunk_count))
        .child(div().child(payload_count))
}
