use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;
use wasmtime::{Engine, Instance, Memory, Module, Store, TypedFunc};

const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginAction {
    #[serde(rename = "send_group_msg")]
    SendGroupMsg { group_id: i64, text: String },
    #[serde(rename = "log")]
    Log { level: String, message: String },
    #[serde(rename = "llm_request")]
    LlmRequest { group_id: i64, prompt: String },
}

pub struct Plugin {
    name: String,
    #[allow(dead_code)]
    engine: Engine,
    store: Store<()>,
    memory: Memory,
    malloc: TypedFunc<(i32,), i32>,
    free: TypedFunc<(i32, i32), ()>,
    on_message_fn: TypedFunc<(i32, i32, i32, i32), i32>,
    on_command_fn: TypedFunc<(i32, i32, i32, i32, i32, i32), i32>,
}

impl Plugin {
    pub fn load(path: PathBuf) -> Result<Self> {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let engine = Engine::default();
        let module = Module::from_file(&engine, &path)
            .with_context(|| format!("failed to load plugin module {}", path.display()))?;

        let mut store = Store::new(&engine, ());
        let instance =
            Instance::new(&mut store, &module, &[]).context("failed to instantiate plugin")?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .context("plugin does not export memory")?;

        let malloc = instance
            .get_typed_func::<(i32,), i32>(&mut store, "malloc")
            .context("plugin does not export malloc")?;
        let free = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "free")
            .context("plugin does not export free")?;
        let on_message_fn = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "on_message")
            .context("plugin does not export on_message")?;
        let on_command_fn = instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, "on_command")
            .context("plugin does not export on_command")?;

        info!(name = %name, path = %path.display(), "loaded plugin");

        Ok(Self {
            name,
            engine,
            store,
            memory,
            malloc,
            free,
            on_message_fn,
            on_command_fn,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn on_message(&mut self, event_json: &str) -> Result<Vec<PluginAction>> {
        self.call_message(event_json)
    }

    pub fn on_command(&mut self, cmd: &str, event_json: &str) -> Result<Vec<PluginAction>> {
        self.call_command(cmd, event_json)
    }

    fn alloc_bytes(&mut self, bytes: &[u8]) -> Result<(i32, i32)> {
        let len = bytes.len() as i32;
        let ptr = self.malloc.call(&mut self.store, (len,))?;
        if ptr == 0 {
            anyhow::bail!("plugin malloc returned null");
        }
        self.memory.write(&mut self.store, ptr as usize, bytes)?;
        Ok((ptr, len))
    }

    fn free_bytes(&mut self, ptr: i32, len: i32) {
        let _ = self.free.call(&mut self.store, (ptr, len));
    }

    fn read_output(&mut self, ptr: i32, len: i32) -> Result<Vec<PluginAction>> {
        if len < 0 {
            anyhow::bail!("plugin returned negative length");
        }
        let len = len as usize;
        let mut buf = vec![0u8; len];
        self.memory.read(&self.store, ptr as usize, &mut buf)?;
        let actions: Vec<PluginAction> =
            serde_json::from_slice(&buf).context("plugin returned invalid JSON")?;
        Ok(actions)
    }

    fn call_message(&mut self, event_json: &str) -> Result<Vec<PluginAction>> {
        let (event_ptr, event_len) = self.alloc_bytes(event_json.as_bytes())?;
        let out_cap = MAX_OUTPUT as i32;
        let out_ptr = self.malloc.call(&mut self.store, (out_cap,))?;
        if out_ptr == 0 {
            self.free_bytes(event_ptr, event_len);
            anyhow::bail!("plugin malloc returned null for output buffer");
        }

        let written = self
            .on_message_fn
            .call(&mut self.store, (event_ptr, event_len, out_ptr, out_cap))?;

        self.free_bytes(event_ptr, event_len);

        if written < 0 {
            self.free_bytes(out_ptr, out_cap);
            anyhow::bail!("plugin on_message returned error code {written}");
        }

        let actions = self.read_output(out_ptr, written)?;
        self.free_bytes(out_ptr, out_cap);
        Ok(actions)
    }

    fn call_command(&mut self, cmd: &str, event_json: &str) -> Result<Vec<PluginAction>> {
        let (event_ptr, event_len) = self.alloc_bytes(event_json.as_bytes())?;
        let (cmd_ptr, cmd_len) = self.alloc_bytes(cmd.as_bytes())?;
        let out_cap = MAX_OUTPUT as i32;
        let out_ptr = self.malloc.call(&mut self.store, (out_cap,))?;
        if out_ptr == 0 {
            self.free_bytes(event_ptr, event_len);
            self.free_bytes(cmd_ptr, cmd_len);
            anyhow::bail!("plugin malloc returned null for output buffer");
        }

        let written = self.on_command_fn.call(
            &mut self.store,
            (cmd_ptr, cmd_len, event_ptr, event_len, out_ptr, out_cap),
        )?;

        self.free_bytes(event_ptr, event_len);
        self.free_bytes(cmd_ptr, cmd_len);

        if written < 0 {
            self.free_bytes(out_ptr, out_cap);
            anyhow::bail!("plugin on_command returned error code {written}");
        }

        let actions = self.read_output(out_ptr, written)?;
        self.free_bytes(out_ptr, out_cap);
        Ok(actions)
    }
}

pub fn discover_plugins(plugin_dir: &str) -> Vec<PathBuf> {
    let path = PathBuf::from(plugin_dir);
    if !path.is_dir() {
        return vec![];
    }
    std::fs::read_dir(&path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wasm"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("release")
            .join("summary.wasm")
    }

    #[test]
    fn load_summary_plugin() {
        let path = wasm_path();
        if !path.exists() {
            eprintln!("skip: summary.wasm not found at {}", path.display());
            return;
        }
        let mut plugin = Plugin::load(path).expect("plugin should load");
        assert_eq!(plugin.name(), "summary");

        let event = r#"{"post_type":"message","message_type":"group","group_id":123,"user_id":456,"message":"hello world"}"#;
        let actions = plugin.on_message(event).expect("on_message should succeed");
        assert!(actions.is_empty());

        let actions = plugin
            .on_command("summary", event)
            .expect("on_command should succeed");
        assert!(!actions.is_empty());
        let has_llm_request = actions
            .iter()
            .any(|a| matches!(a, PluginAction::LlmRequest { .. }));
        assert!(has_llm_request, "summary command should request LLM");
    }

    #[test]
    fn status_command_replies_directly() {
        let path = wasm_path();
        if !path.exists() {
            eprintln!("skip: summary.wasm not found at {}", path.display());
            return;
        }
        let mut plugin = Plugin::load(path).expect("plugin should load");
        let event = r#"{"post_type":"message","message_type":"group","group_id":123,"user_id":456,"message":"hello"}"#;
        plugin.on_message(event).unwrap();

        let actions = plugin
            .on_command("status", event)
            .expect("status should succeed");
        let has_send = actions
            .iter()
            .any(|a| matches!(a, PluginAction::SendGroupMsg { .. }));
        assert!(has_send, "status command should reply directly");
    }
}
