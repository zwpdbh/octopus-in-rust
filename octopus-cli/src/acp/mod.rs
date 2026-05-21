pub struct AcpServer;

impl AcpServer {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self) {
        println!("ACP server not yet implemented");
    }
}

impl Default for AcpServer {
    fn default() -> Self {
        Self::new()
    }
}
