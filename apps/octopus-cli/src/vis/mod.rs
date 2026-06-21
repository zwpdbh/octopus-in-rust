pub struct VisServer;

impl VisServer {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self) {
        println!("Visualizer server not yet implemented");
    }
}

impl Default for VisServer {
    fn default() -> Self {
        Self::new()
    }
}
