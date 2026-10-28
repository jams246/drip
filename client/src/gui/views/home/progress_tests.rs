use super::*;
use drip_core::SourcePath;

fn progress(phase: UploadPhase, processed: u64, total: Option<u64>) -> SelectionProgress {
    SelectionProgress {
        selection_id: "root".into(),
        operation_id: "upload".into(),
        source_bytes_processed: processed,
        uploaded_file_bytes: 0,
        file: drip_client::ProgressSnapshot {
            phase,
            source_path: SourcePath::new('C', vec!["file.bin".into()]).unwrap(),
            processed_bytes: processed,
            total_bytes: total,
            source_hashed_bytes: processed,
            source_chunked_bytes: processed,
            chunk_count: 1,
            planned_payload_bytes: processed,
            file_payload_sent: 0,
        },
    }
}

#[test]
fn stage_measurement_clamps_zero_partial_complete_and_over_total() {
    let phase = UploadPhase::Hashing;
    assert_eq!(measurement(&progress(phase, 0, Some(10))), Some(0));
    assert_eq!(measurement(&progress(phase, 5, Some(10))), Some(50));
    assert_eq!(measurement(&progress(phase, 10, Some(10))), Some(100));
    assert_eq!(measurement(&progress(phase, 15, Some(10))), Some(100));
}

#[test]
fn unknown_or_zero_total_has_no_measurement() {
    assert_eq!(measurement(&progress(UploadPhase::Chunking, 3, None)), None);
    assert_eq!(
        measurement(&progress(UploadPhase::Uploading, 3, Some(0))),
        None
    );
}

#[test]
fn large_counters_use_overflow_safe_ratio() {
    assert_eq!(
        measurement(&progress(
            UploadPhase::Hashing,
            u64::MAX / 2,
            Some(u64::MAX)
        )),
        Some(49)
    );
}

#[test]
fn verifying_never_exposes_numeric_progress() {
    let progress = progress(UploadPhase::Verifying, 10, Some(10));
    assert_eq!(measurement(&progress), None);
    assert_eq!(stage(UploadPhase::Verifying).label, "Checking upload");
}

#[test]
fn phase_change_uses_new_stage_and_its_measurement() {
    let hashing = progress(UploadPhase::Hashing, 10, Some(10));
    let chunking = progress(UploadPhase::Chunking, 0, Some(10));
    assert_eq!(stage(hashing.file.phase).label, "Preparing file");
    assert_eq!(measurement(&hashing), Some(100));
    assert_eq!(stage(chunking.file.phase).label, "Preparing changes");
    assert_eq!(measurement(&chunking), Some(0));
}

#[test]
fn indeterminate_animation_runs_only_when_intersecting_its_clip() {
    let visible = Bounds::new(gpui::point(px(10.), px(20.)), gpui::size(px(20.), px(8.)));
    let clip = Bounds::new(gpui::point(px(0.), px(24.)), gpui::size(px(100.), px(40.)));
    let offscreen = Bounds::new(gpui::point(px(10.), px(70.)), gpui::size(px(20.), px(8.)));

    assert!(animation_visible(visible, clip));
    assert!(!animation_visible(offscreen, clip));
}
