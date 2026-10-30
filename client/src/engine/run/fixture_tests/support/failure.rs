use super::*;

impl Fixture {
    pub(super) fn failure_summary(&self, events: &[LifecycleEvent]) -> String {
        let directory = self.engine.settings.directory().to_string_lossy();
        events
            .iter()
            .rev()
            .take(24)
            .map(|event| {
                let message = event
                    .message
                    .replace(self.pairing.credential(), "<credential>")
                    .replace(self.pairing.server_url(), "<server>")
                    .replace(directory.as_ref(), "<fixture>");
                format!("{:?}: {message}", event.kind)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn pass_while_offline(&mut self) -> Result<bool, crate::engine::EngineError> {
        self.signal.sweep();
        // Port zero cannot name a listening TCP service.
        let unavailable = HttpTransport::new("https://127.0.0.1:0").unwrap();
        let context = PassContext {
            cache: &self.cache,
            store: &self.engine.settings,
            transport: &unavailable,
            pairing: &self.pairing,
            events: &self.events,
            signal: &self.signal,
            control: &self.control,
        };
        self.engine
            .continuous_pass(&context, &mut self.state, self.barrier)
            .await
    }
}
