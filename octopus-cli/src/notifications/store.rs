use std::path::{Path, PathBuf};

use crate::notifications::{NotificationDelivery, NotificationEvent, NotificationView};

#[derive(Clone)]
pub struct NotificationStore {
    root: PathBuf,
}

impl NotificationStore {
    pub const EVENT_FILE: &'static str = "event.json";
    pub const DELIVERY_FILE: &'static str = "delivery.json";

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn ensure_root(&self) -> std::io::Result<&Path> {
        std::fs::create_dir_all(&self.root)?;
        Ok(&self.root)
    }

    pub fn notification_dir(&self, notification_id: &str) -> std::io::Result<PathBuf> {
        let path = self.ensure_root()?.join(notification_id);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn event_path(&self, notification_id: &str) -> PathBuf {
        self.root.join(notification_id).join(Self::EVENT_FILE)
    }

    pub fn delivery_path(&self, notification_id: &str) -> PathBuf {
        self.root.join(notification_id).join(Self::DELIVERY_FILE)
    }

    pub fn create_notification(
        &self,
        event: &NotificationEvent,
        delivery: &NotificationDelivery,
    ) -> std::io::Result<()> {
        let dir = self.notification_dir(&event.id)?;
        let event_json = serde_json::to_vec_pretty(event)?;
        let delivery_json = serde_json::to_vec_pretty(delivery)?;
        std::fs::write(dir.join(Self::EVENT_FILE), event_json)?;
        std::fs::write(dir.join(Self::DELIVERY_FILE), delivery_json)?;
        Ok(())
    }

    pub fn list_notification_ids(&self) -> Vec<String> {
        if !self.root.exists() {
            return Vec::new();
        }
        let mut ids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if !path.join(Self::EVENT_FILE).exists() {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    ids.push(name.to_string());
                }
            }
        }
        ids.sort();
        ids
    }

    pub fn read_event(&self, notification_id: &str) -> Option<NotificationEvent> {
        let path = self.event_path(notification_id);
        if !path.exists() {
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn write_event(&self, event: &NotificationEvent) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(event)?;
        std::fs::write(self.event_path(&event.id), json)?;
        Ok(())
    }

    pub fn read_delivery(&self, notification_id: &str) -> NotificationDelivery {
        let path = self.delivery_path(notification_id);
        if !path.exists() {
            return NotificationDelivery::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(e) => {
                tracing::warn!(
                    "Failed to read notification delivery for {}: {}",
                    notification_id,
                    e
                );
                NotificationDelivery::default()
            }
        }
    }

    pub fn write_delivery(
        &self,
        notification_id: &str,
        delivery: &NotificationDelivery,
    ) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(delivery)?;
        std::fs::write(self.delivery_path(notification_id), json)?;
        Ok(())
    }

    pub fn merged_view(&self, notification_id: &str) -> Option<NotificationView> {
        let event = self.read_event(notification_id)?;
        let delivery = self.read_delivery(notification_id);
        Some(NotificationView { event, delivery })
    }

    pub fn list_views(&self) -> Vec<NotificationView> {
        let mut views = Vec::new();
        for id in self.list_notification_ids() {
            if let Some(view) = self.merged_view(&id) {
                views.push(view);
            }
        }
        views.sort_by(|a, b| {
            a.event
                .created_at
                .partial_cmp(&b.event.created_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .reverse()
        });
        views
    }
}
