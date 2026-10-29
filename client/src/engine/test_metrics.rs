use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

use drip_core::SourcePath;
use serde::Serialize;

#[derive(Clone, Copy)]
pub(crate) enum ReadKind {
    Hashing,
    Chunking,
    RangeReread,
}

#[derive(Default, Serialize)]
struct FileMetrics {
    hashing_bytes: u64,
    chunking_bytes: u64,
    range_reread_bytes: u64,
    upload_plan_requests: u64,
    upload_payload_bytes: u64,
}

static METRICS: OnceLock<Mutex<BTreeMap<String, FileMetrics>>> = OnceLock::new();

#[derive(Serialize)]
pub(crate) struct ProcessResources {
    pub cpu_100ns: u64,
    pub peak_working_set_bytes: u64,
}

fn metrics() -> &'static Mutex<BTreeMap<String, FileMetrics>> {
    METRICS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(crate) fn register_source(path: &SourcePath) {
    metrics()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .entry(path.to_windows_path())
        .or_default();
}

pub(crate) fn source_read(path: &SourcePath, kind: ReadKind, bytes: usize) {
    let mut all = metrics().lock().unwrap_or_else(|error| error.into_inner());
    let file = all.entry(path.to_windows_path()).or_default();
    let field = match kind {
        ReadKind::Hashing => &mut file.hashing_bytes,
        ReadKind::Chunking => &mut file.chunking_bytes,
        ReadKind::RangeReread => &mut file.range_reread_bytes,
    };
    *field = field.saturating_add(bytes as u64);
}

pub(crate) fn plan_request(path: &SourcePath) {
    let mut all = metrics().lock().unwrap_or_else(|error| error.into_inner());
    let file = all.entry(path.to_windows_path()).or_default();
    file.upload_plan_requests = file.upload_plan_requests.saturating_add(1);
}

pub(crate) fn upload_payload(path: &SourcePath, bytes: usize) {
    let mut all = metrics().lock().unwrap_or_else(|error| error.into_inner());
    let file = all.entry(path.to_windows_path()).or_default();
    file.upload_payload_bytes = file.upload_payload_bytes.saturating_add(bytes as u64);
}

pub(crate) fn write(path: &Path) -> std::io::Result<()> {
    let all = metrics().lock().unwrap_or_else(|error| error.into_inner());
    let bytes = serde_json::to_vec_pretty(&*all).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

#[expect(
    unsafe_code,
    reason = "test acceptance samples its Windows worker process"
)]
pub(crate) fn process_resources() -> std::io::Result<ProcessResources> {
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{GetCurrentProcess, GetProcessTimes},
    };

    fn ticks(value: windows::Win32::Foundation::FILETIME) -> u64 {
        (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
    }

    let process = unsafe { GetCurrentProcess() };
    let (mut creation, mut exit, mut kernel, mut user) = Default::default();
    unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
        .map_err(std::io::Error::other)?;
    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>())
            .map_err(std::io::Error::other)?,
        ..Default::default()
    };
    unsafe {
        GetProcessMemoryInfo(
            process,
            &mut memory,
            u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>())
                .map_err(std::io::Error::other)?,
        )
    }
    .map_err(std::io::Error::other)?;
    Ok(ProcessResources {
        cpu_100ns: ticks(kernel).saturating_add(ticks(user)),
        peak_working_set_bytes: memory.PeakWorkingSetSize as u64,
    })
}
