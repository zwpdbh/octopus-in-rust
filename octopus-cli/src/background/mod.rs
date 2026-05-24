use std::collections::HashMap;
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Running,
    Completed(i32),
    Failed(String),
    Killed,
}

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub description: String,
    pub status: TaskStatus,
    pub output: String,
}

struct TaskHandle {
    task: BackgroundTask,
    child: Arc<Mutex<Child>>,
    output: Arc<Mutex<String>>,
}

#[derive(Clone)]
pub struct BackgroundTaskManager {
    tasks: Arc<std::sync::Mutex<HashMap<String, TaskHandle>>>,
}

impl std::fmt::Debug for BackgroundTaskManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tasks = self.tasks.lock().unwrap();
        f.debug_struct("BackgroundTaskManager")
            .field("task_count", &tasks.len())
            .finish()
    }
}

impl BackgroundTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new background task and return its ID.
    pub async fn spawn(&self, command: String, description: String) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c")
            .arg(&command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn background task: {}", e))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let child = Arc::new(Mutex::new(child));
        let output = Arc::new(Mutex::new(String::new()));

        // Spawn output reader
        let output_clone = output.clone();
        let child_clone = child.clone();
        let id_clone = id.clone();
        let tasks_clone = self.tasks.clone();

        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut stdout_reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();

            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => {
                                let mut out = output_clone.lock().await;
                                out.push_str(&l);
                                out.push('\n');
                            }
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => {
                                let mut out = output_clone.lock().await;
                                out.push_str(&l);
                                out.push('\n');
                            }
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                }
            }

            // Wait for exit
            let status = {
                let mut child = child_clone.lock().await;
                child.wait().await
            };

            let mut tasks = tasks_clone.lock().unwrap();
            if let Some(handle) = tasks.get_mut(&id_clone) {
                handle.task.status = match status {
                    Ok(s) if s.success() => TaskStatus::Completed(s.code().unwrap_or(0)),
                    Ok(s) => TaskStatus::Completed(s.code().unwrap_or(-1)),
                    Err(e) => TaskStatus::Failed(e.to_string()),
                };
            }
        });

        let task = BackgroundTask {
            id: id.clone(),
            command: command.clone(),
            description,
            status: TaskStatus::Running,
            output: String::new(),
        };

        let handle = TaskHandle {
            task,
            child,
            output,
        };

        self.tasks.lock().unwrap().insert(id.clone(), handle);
        Ok(id)
    }

    /// Get the current output and status of a task.
    pub async fn get_output(&self, id: &str) -> Option<(TaskStatus, String)> {
        let output_arc = {
            let tasks = self.tasks.lock().unwrap();
            let handle = tasks.get(id)?;
            (handle.task.status.clone(), handle.output.clone())
        };
        let output = output_arc.1.lock().await;
        Some((output_arc.0, output.clone()))
    }

    /// Stop (kill) a running background task.
    pub async fn stop(&self, id: &str) -> Result<(), String> {
        let child_arc = {
            let tasks = self.tasks.lock().unwrap();
            let handle = tasks.get(id).ok_or("Task not found")?;
            handle.child.clone()
        };
        let mut child = child_arc.lock().await;
        child
            .kill()
            .await
            .map_err(|e| format!("Failed to kill task: {}", e))?;

        let mut tasks = self.tasks.lock().unwrap();
        if let Some(handle) = tasks.get_mut(id) {
            handle.task.status = TaskStatus::Killed;
        }
        Ok(())
    }

    /// Get a snapshot of a task (without current output).
    pub fn get_task(&self, id: &str) -> Option<BackgroundTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(id).map(|h| h.task.clone())
    }

    /// List all tasks.
    pub fn list_tasks(&self) -> Vec<BackgroundTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().map(|h| h.task.clone()).collect()
    }

    /// Shutdown all tasks.
    pub async fn shutdown(&self) {
        let child_arcs: Vec<Arc<Mutex<Child>>> = {
            let tasks = self.tasks.lock().unwrap();
            tasks.values().map(|h| h.child.clone()).collect()
        };
        for child_arc in child_arcs {
            let mut child = child_arc.lock().await;
            let _ = child.kill().await;
        }
        let mut tasks = self.tasks.lock().unwrap();
        tasks.clear();
    }
}

impl Default for BackgroundTaskManager {
    fn default() -> Self {
        Self::new()
    }
}
