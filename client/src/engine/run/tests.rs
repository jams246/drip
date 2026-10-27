use super::*;

#[test]
fn worker_future_keeps_large_buffers_off_the_main_stack() {
    let engine = Engine::beside_executable().unwrap();
    let (_control, receiver) = watch::channel(RunControl::default());
    let (events, _receiver) = tokio::sync::mpsc::channel(1);
    let (progress, _) = watch::channel(None);
    let future = engine.run(receiver, EventSink::new(events, progress));
    assert!(std::mem::size_of_val(&future) < 64 * 1024);
}
