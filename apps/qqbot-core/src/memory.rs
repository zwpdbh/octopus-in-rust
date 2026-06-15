use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// A fixed-size message buffer for one group.
#[derive(Debug, Clone)]
pub struct GroupMemory {
    max_messages: usize,
    messages: VecDeque<(i64, String)>,
}

#[allow(dead_code)]
impl GroupMemory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            messages: VecDeque::new(),
        }
    }

    pub fn push(&mut self, user_id: i64, text: String) {
        self.messages.push_back((user_id, text));
        if self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<(i64, String)> {
        self.messages
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Shared store of per-group memories.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    inner: Arc<Mutex<HashMap<i64, GroupMemory>>>,
    max_messages: usize,
}

#[allow(dead_code)]
impl MemoryStore {
    pub fn new(max_messages: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_messages,
        }
    }

    pub fn push(&self, group_id: i64, user_id: i64, text: String) {
        let mut map = self.inner.lock().unwrap();
        map.entry(group_id)
            .or_insert_with(|| GroupMemory::new(self.max_messages))
            .push(user_id, text);
    }

    pub fn recent(&self, group_id: i64, limit: usize) -> Vec<(i64, String)> {
        let map = self.inner.lock().unwrap();
        map.get(&group_id)
            .map(|m| m.recent(limit))
            .unwrap_or_default()
    }

    pub fn len(&self, group_id: i64) -> usize {
        let map = self.inner.lock().unwrap();
        map.get(&group_id).map(|m| m.len()).unwrap_or(0)
    }
}
