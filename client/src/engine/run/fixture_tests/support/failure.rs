use super::*;

impl Fixture {
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
