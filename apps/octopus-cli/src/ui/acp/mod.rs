pub struct AcpUI;

impl AcpUI {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self) {
        println!("ACP UI not yet implemented");
    }
}

impl Default for AcpUI {
    fn default() -> Self {
        Self::new()
    }
}
