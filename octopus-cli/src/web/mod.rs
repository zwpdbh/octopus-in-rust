pub struct WebServer;

impl WebServer {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self) {
        println!("Web server not yet implemented");
    }
}

impl Default for WebServer {
    fn default() -> Self {
        Self::new()
    }
}
