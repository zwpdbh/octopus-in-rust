use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Per-group settings that override the global `Config` when a `Brain` is created.
///
/// Each allowed QQ group can have its own system prompt and its own set of
/// enabled/disabled plugins. If a group's profile does not exist, the global
/// defaults are used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupProfile {
    /// Optional system prompt that replaces the global one for this group.
    #[serde(default)]
    pub system_prompt: Option<String>,

    /// If set, only these plugin file stems are loaded for the group.
    /// If `None`, all installed plugins are loaded (minus `disabled_plugins`).
    #[serde(default)]
    pub enabled_plugins: Option<Vec<String>>,

    /// Plugin file stems that are always excluded for this group.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,

    /// Seconds between progress updates while the bot is working on a
    /// long-running answer. Default: 10.
    #[serde(default)]
    pub progress_interval_secs: Option<u64>,
}

impl GroupProfile {
    /// Load a group's profile from `<data_dir>/groups/<group_id>.toml`.
    ///
    /// Returns `None` if the file does not exist.
    pub fn load(data_dir: &Path, group_id: i64) -> anyhow::Result<Option<Self>> {
        let path = profile_path(data_dir, group_id);
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        let profile: Self = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("invalid TOML in {}: {e}", path.display()))?;
        Ok(Some(profile))
    }

    /// Save a group's profile, creating the `groups/` directory if needed.
    pub fn save(data_dir: &Path, group_id: i64, profile: &Self) -> anyhow::Result<()> {
        let dir = groups_dir(data_dir);
        std::fs::create_dir_all(&dir)?;
        let path = profile_path(data_dir, group_id);
        let contents = toml::to_string_pretty(profile)?;
        std::fs::write(&path, contents)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
        Ok(())
    }

    /// Return the effective system prompt for the group.
    pub fn system_prompt_or<'a>(&'a self, global: &'a str) -> &'a str {
        self.system_prompt.as_deref().unwrap_or(global)
    }

    /// Return the effective progress interval in seconds.
    pub fn progress_interval_secs(&self) -> u64 {
        self.progress_interval_secs.unwrap_or(30)
    }

    /// Decide whether a plugin (by file stem) is allowed for this group.
    pub fn is_plugin_allowed(&self, plugin_name: &str) -> bool {
        if self.disabled_plugins.iter().any(|n| n == plugin_name) {
            return false;
        }
        match &self.enabled_plugins {
            Some(allowed) => allowed.iter().any(|n| n == plugin_name),
            None => true,
        }
    }

    /// Convenience: add a plugin to `enabled_plugins` if not already there.
    pub fn enable_plugin(&mut self, plugin_name: &str) {
        let mut plugins = self.enabled_plugins.take().unwrap_or_default();
        if !plugins.iter().any(|n| n == plugin_name) {
            plugins.push(plugin_name.to_string());
        }
        self.enabled_plugins = Some(plugins);
        self.disabled_plugins.retain(|n| n != plugin_name);
    }

    /// Convenience: add a plugin to `disabled_plugins` and remove from enabled.
    pub fn disable_plugin(&mut self, plugin_name: &str) {
        if let Some(plugins) = self.enabled_plugins.as_mut() {
            plugins.retain(|n| n != plugin_name);
        }
        if !self.disabled_plugins.iter().any(|n| n == plugin_name) {
            self.disabled_plugins.push(plugin_name.to_string());
        }
    }

    /// Return the plugin file stems that should be loaded for this group,
    /// given all installed plugin names.
    pub fn filter_plugins<'a>(&self, installed: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        installed
            .into_iter()
            .filter(|name| self.is_plugin_allowed(name))
            .map(|s| s.to_string())
            .collect()
    }
}

pub fn groups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("groups")
}

pub fn profile_path(data_dir: &Path, group_id: i64) -> PathBuf {
    groups_dir(data_dir).join(format!("{group_id}.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_plugin_allowed() {
        let profile = GroupProfile {
            enabled_plugins: Some(vec!["summary".to_string()]),
            disabled_plugins: vec!["bad".to_string()],
            ..Default::default()
        };
        assert!(profile.is_plugin_allowed("summary"));
        assert!(!profile.is_plugin_allowed("bad"));
        assert!(!profile.is_plugin_allowed("other"));
    }

    #[test]
    fn test_filter_plugins_defaults() {
        let profile = GroupProfile::default();
        let names = vec!["summary", "example-http"];
        assert_eq!(
            profile.filter_plugins(names.into_iter()),
            vec!["summary".to_string(), "example-http".to_string()]
        );
    }

    #[test]
    fn test_enable_and_disable_plugin() {
        let mut profile = GroupProfile::default();
        profile.enable_plugin("summary");
        assert!(profile.is_plugin_allowed("summary"));

        profile.disable_plugin("summary");
        assert!(!profile.is_plugin_allowed("summary"));
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let profile = GroupProfile {
            system_prompt: Some("Group-specific prompt.".to_string()),
            enabled_plugins: Some(vec!["summary".to_string()]),
            disabled_plugins: vec![],
            ..Default::default()
        };
        GroupProfile::save(data_dir, 123456, &profile).unwrap();
        let loaded = GroupProfile::load(data_dir, 123456).unwrap().unwrap();
        assert_eq!(loaded.system_prompt, profile.system_prompt);
        assert_eq!(loaded.enabled_plugins, profile.enabled_plugins);
    }
}
