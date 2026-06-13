use crate::onebot::GroupMessage;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// A single buffered message with its metadata.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BufferedMessage {
    pub user_id: i64,
    pub nickname: String,
    pub text: String,
    pub time: DateTime<Utc>,
}

impl From<&GroupMessage> for BufferedMessage {
    fn from(msg: &GroupMessage) -> Self {
        Self {
            user_id: msg.user_id,
            nickname: msg.sender_nickname.clone(),
            text: msg.raw_message.clone(),
            time: DateTime::from_timestamp(msg.time, 0).unwrap_or_else(Utc::now),
        }
    }
}

/// Per-group ring buffer of recent messages.
pub struct MessageBuffer {
    messages: HashMap<i64, Vec<BufferedMessage>>,
    max_size: usize,
    window_secs: u64,
}

impl MessageBuffer {
    pub fn new(max_size: usize, window_secs: u64) -> Self {
        Self {
            messages: HashMap::new(),
            max_size,
            window_secs,
        }
    }

    /// Add a message to the buffer for its group, then prune stale entries.
    pub fn push(&mut self, group_id: i64, msg: BufferedMessage) {
        let entries = self.messages.entry(group_id).or_default();
        entries.push(msg);
        self.prune(group_id);
    }

    /// Return the current conversation log for a group, oldest first.
    pub fn format_context(&self, group_id: i64) -> String {
        let entries = self.messages.get(&group_id).cloned().unwrap_or_default();
        entries
            .iter()
            .map(|m| format!("[{}] {}: {}", m.time.format("%H:%M"), m.nickname, m.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Return the number of buffered messages for a group.
    pub fn len(&self, group_id: i64) -> usize {
        self.messages.get(&group_id).map(Vec::len).unwrap_or(0)
    }

    fn prune(&mut self, group_id: i64) {
        let now = Utc::now();
        let window = chrono::Duration::seconds(self.window_secs as i64);

        if let Some(entries) = self.messages.get_mut(&group_id) {
            // Drop messages older than the time window.
            entries.retain(|m| now - m.time <= window);
            // Keep only the most recent messages up to max_size.
            if entries.len() > self.max_size {
                let start = entries.len() - self.max_size;
                *entries = entries.split_off(start);
            }
        }
    }
}
