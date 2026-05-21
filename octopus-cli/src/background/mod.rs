use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub status: String,
}

pub struct BackgroundTaskManager {
    tasks: HashMap<String, BackgroundTask>,
}

impl BackgroundTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    pub fn create_task(&mut self, command: String) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let task = BackgroundTask {
            id: id.clone(),
            command,
            status: "running".to_string(),
        };
        self.tasks.insert(id.clone(), task);
        id
    }

    pub fn get_task(&self, id: &str) -> Option<&BackgroundTask> {
        self.tasks.get(id)
    }

    pub fn list_tasks(&self) -> Vec<&BackgroundTask> {
        self.tasks.values().collect()
    }
}

impl Default for BackgroundTaskManager {
    fn default() -> Self {
        Self::new()
    }
}
