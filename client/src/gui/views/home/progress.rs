use std::time::Duration;

use drip_client::{SelectionProgress, UploadPhase};
use gpui::{IntoElement, div, prelude::*, px, relative, rgb};

use crate::gui::theme;

pub(super) struct Stage {
    pub label: &'static str,
    pub percent: Option<u8>,
}

pub(super) fn stage(phase: UploadPhase) -> Stage {
    Stage {
        label: match phase {
            UploadPhase::Hashing => "Preparing file",
            UploadPhase::Chunking => "Preparing changes",
            UploadPhase::Uploading => "Sending changes",
            UploadPhase::Verifying => "Checking upload",
        },
        percent: None,
    }
}

pub(super) fn measurement(progress: &SelectionProgress) -> Option<u8> {
    if progress.file.phase == UploadPhase::Verifying {
        return None;
    }
    let total = progress.file.total_bytes.filter(|total| *total > 0)?;
    Some(((u128::from(progress.file.processed_bytes) * 100) / u128::from(total)).min(100) as u8)
}

pub(super) fn render(
    progress: &SelectionProgress,
    animation_offset: Option<f32>,
) -> impl IntoElement {
    let mut stage = stage(progress.file.phase);
    stage.percent = measurement(progress);
    let indicator = div()
        .id(format!("home-progress-{}", progress.operation_id))
        .flex()
        .flex_col()
        .gap_2()
        .pt_2();
    if let Some(percent) = stage.percent {
        indicator = indicator;
    }
    let mut labels = div()
        .flex()
        .justify_between()
        .text_sm()
        .text_color(rgb(theme::TEXT))
        .child(stage.label);
    if let Some(percent) = stage.percent {
        labels = labels.child(format!("{percent}%"));
    }
    let fill = if let Some(percent) = stage.percent {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .rounded_full()
            .w(relative(f32::from(percent) / 100.))
            .bg(rgb(theme::ACCENT))
    } else {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(animation_offset.unwrap_or(0.)))
            .rounded_full()
            .w(relative(0.25))
            .bg(rgb(theme::ACCENT))
    };
    indicator
        .child(labels)
        .child(
            div()
                .id(format!("home-progress-track-{}", progress.operation_id))
                .relative()
                .w_full()
                .h(px(8.))
                .rounded_full()
                .bg(rgb(theme::TRACK))
                .child(fill),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child("Progress for this stage"),
        )
}

pub(super) fn animation_offset(elapsed: Duration) -> f32 {
    (elapsed.as_millis() % 1200) as f32 / 1200. * 0.75
}

#[cfg(test)]
mod tests {
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
}
