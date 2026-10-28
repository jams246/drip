use std::sync::OnceLock;
use std::time::Instant;

use drip_client::{SelectionProgress, UploadPhase};
use gpui::{
    Bounds, Context, IntoElement, Pixels, Render, Window, div, prelude::*, px, relative, rgb,
};

use crate::gui::theme;

static ANIMATION_ORIGIN: OnceLock<Instant> = OnceLock::new();

#[derive(Default)]
pub(in crate::gui) struct ProgressView {
    progress: Option<SelectionProgress>,
}

impl ProgressView {
    pub(super) fn set_progress(
        &mut self,
        progress: Option<SelectionProgress>,
        cx: &mut Context<Self>,
    ) {
        if self.progress != progress {
            self.progress = progress;
            cx.notify();
        }
    }
}

impl Render for ProgressView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match self.progress.as_ref() {
            Some(progress) => render(progress).into_any_element(),
            None => div().into_any_element(),
        }
    }
}

#[derive(Clone, Copy)]
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

fn animation_visible(bounds: Bounds<Pixels>, mask: Bounds<Pixels>) -> bool {
    bounds.intersects(&mask)
}

fn indeterminate_segment() -> impl IntoElement {
    let origin = *ANIMATION_ORIGIN.get_or_init(Instant::now);
    gpui::canvas(
        |bounds, window, _| {
            let visible = animation_visible(bounds, window.content_mask().bounds);
            if visible {
                window.request_animation_frame();
            }
            visible
        },
        move |bounds, visible, window, _| {
            if !visible {
                return;
            }
            let phase = origin.elapsed().as_secs_f32() / 1.2 % 1.0;
            let width = bounds.size.width * 0.25;
            let travel = bounds.size.width - width;
            let segment = Bounds::new(
                gpui::point(bounds.origin.x + travel * phase, bounds.origin.y),
                gpui::size(width, bounds.size.height),
            );
            window.paint_quad(gpui::fill(segment, rgb(theme::ACCENT)).corner_radii(px(4.)));
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .left_0()
    .right_0()
}

pub(super) fn render(progress: &SelectionProgress) -> impl IntoElement {
    let mut stage = stage(progress.file.phase);
    stage.percent = measurement(progress);
    let indicator = div()
        .id(format!("home-progress-{}", progress.operation_id))
        .flex()
        .flex_col()
        .gap_2()
        .pt_2();
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
            .into_any_element()
    } else {
        indeterminate_segment().into_any_element()
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

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
