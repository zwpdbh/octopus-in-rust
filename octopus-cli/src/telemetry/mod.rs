pub struct Telemetry;

impl Telemetry {
    pub fn new() -> Self {
        Self
    }

    pub fn track(&self, _event: &str, _data: serde_json::Value) {
        // TODO: implement telemetry tracking
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}
