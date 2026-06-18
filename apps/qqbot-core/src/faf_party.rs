use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, FixedOffset};
use kosong::tooling::ToolReturnValue;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::group_brain::GroupBrainManager;
use crate::onebot::ActionTx;
use crate::onebot::types::Action;

/// Minimum number of overlapping candidates needed to trigger a 3v3 match.
const PARTY_SIZE: usize = 6;

/// Retry delays after the initial notification.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(30), Duration::from_secs(30)];

/// Persistent state for one group.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PartyState {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Candidate {
    user_id: i64,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
    joined_at: DateTime<FixedOffset>,
}

/// Public snapshot of the current party status, returned by `faf_party_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyStatus {
    pub count: usize,
    pub candidates: Vec<PartyCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyCandidate {
    pub user_id: i64,
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
}

/// Read the current party status for a group without modifying it.
pub async fn read_party_status(data_dir: &Path, group_id: i64) -> Result<PartyStatus> {
    let path = data_dir.join(format!("faf-party-{group_id}.json"));
    if !path.exists() {
        return Ok(PartyStatus {
            count: 0,
            candidates: Vec::new(),
        });
    }
    let text = tokio::fs::read_to_string(&path)
        .await
        .context("failed to read party state file")?;
    let state: PartyState = serde_json::from_str(&text).context("invalid party state JSON")?;
    Ok(PartyStatus {
        count: state.candidates.len(),
        candidates: state
            .candidates
            .into_iter()
            .map(|c| PartyCandidate {
                user_id: c.user_id,
                start: c.start,
                end: c.end,
            })
            .collect(),
    })
}

/// Host-provided tool that lets the LLM query the current FAF party candidate list.
pub struct FafPartyStatusTool {
    data_dir: PathBuf,
    group_id: i64,
}

impl FafPartyStatusTool {
    pub fn new(data_dir: PathBuf, group_id: i64) -> Self {
        Self { data_dir, group_id }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct FafPartyStatusParams {
    /// No parameters needed; status is always for the current group.
    #[allow(dead_code)]
    _noop: Option<String>,
}

#[async_trait]
impl kosong::tooling::CallableTool2 for FafPartyStatusTool {
    type Params = FafPartyStatusParams;

    fn name(&self) -> &str {
        "faf_party_status"
    }

    fn description(&self) -> &str {
        "Get the current FAF party candidate list and overlap window for this group."
    }

    fn prompt_fragment(&self) -> Option<&str> {
        Some("When the user asks how many people are in the FAF party, who is playing, or when the party will happen, call faf_party_status to get the current candidates.")
    }

    async fn call_typed(&self, _params: FafPartyStatusParams) -> ToolReturnValue {
        match read_party_status(&self.data_dir, self.group_id).await {
            Ok(status) => {
                let text = serde_json::to_string(&status)
                    .unwrap_or_else(|_| "{\"count\":0,\"candidates\":[]}".to_string());
                ToolReturnValue::ok(text)
            }
            Err(e) => ToolReturnValue::error(format!("failed to read party status: {e}")),
        }
    }
}

/// Result returned by the `faf_party_parse_message` plugin tool.
#[derive(Debug, Clone, Deserialize)]
struct ParseResult {
    intent: String,
    availability: Option<AvailabilityJson>,
}

#[derive(Debug, Clone, Deserialize)]
struct AvailabilityJson {
    start: String,
    end: String,
}

/// Host-side service that owns FAF party scheduling state and timers.
#[derive(Clone)]
pub struct FafPartyHostService {
    data_dir: PathBuf,
    action_tx: ActionTx,
    active_notifications: Arc<Mutex<HashMap<i64, JoinHandle<()>>>>,
}

impl FafPartyHostService {
    pub fn new(data_dir: PathBuf, _config: &Config, action_tx: ActionTx) -> Self {
        Self {
            data_dir,
            action_tx,
            active_notifications: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Process an addressed message for party scheduling intent.
    ///
    /// This is called before the normal Brain turn starts so that notifications
    /// can be sent immediately when enough players overlap.
    pub async fn process_message(
        &self,
        manager: &GroupBrainManager,
        group_id: i64,
        user_id: i64,
        message: &str,
        now: DateTime<FixedOffset>,
    ) {
        if let Err(e) = self.try_process_message(manager, group_id, user_id, message, now).await {
            error!(group_id, user_id, error = %e, "faf-party processing failed");
        }
    }

    async fn try_process_message(
        &self,
        manager: &GroupBrainManager,
        group_id: i64,
        user_id: i64,
        message: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<()> {
        // Call the plugin to parse intent and availability.
        let parse = self.call_parse_message(manager, group_id, message, now).await?;

        let mut state = self.load_state(group_id).await?;

        match parse.intent.as_str() {
            "leave" => {
                let before = state.candidates.len();
                state.candidates.retain(|c| c.user_id != user_id);
                let removed = before - state.candidates.len();
                if removed > 0 {
                    self.save_state(group_id, &state).await?;
                    info!(group_id, user_id, "removed user from faf-party candidates");
                }
            }
            "join" => {
                let Some(avail) = parse.availability else {
                    return Ok(());
                };
                let start = DateTime::parse_from_rfc3339(&avail.start)
                    .context("invalid availability start")?
                    .with_timezone(&FixedOffset::east_opt(0).unwrap());
                let end = DateTime::parse_from_rfc3339(&avail.end)
                    .context("invalid availability end")?
                    .with_timezone(&FixedOffset::east_opt(0).unwrap());

                // Remove stale entry for this user, then add the new one.
                state.candidates.retain(|c| c.user_id != user_id);
                state.candidates.push(Candidate {
                    user_id,
                    start,
                    end,
                    joined_at: now,
                });
                self.save_state(group_id, &state).await?;
                info!(group_id, user_id, start = %start, end = %end, "added faf-party candidate");

                if let Some(window) = find_overlap_window(&state.candidates) {
                    self.schedule_notifications(group_id, state.candidates.clone(), window)
                        .await;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn call_parse_message(
        &self,
        manager: &GroupBrainManager,
        group_id: i64,
        message: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<ParseResult> {
        let brain = manager
            .get_or_create_brain(group_id)
            .await
            .context("failed to get group brain")?;
        let tool = brain
            .registry()
            .find("faf_party_parse_message")
            .context("faf_party_parse_message tool not found")?;

        let args = serde_json::json!({
            "message": message,
            "now": now.to_rfc3339(),
        });

        let result = tool.call_raw(args).await;
        if result.is_error {
            return Err(anyhow::anyhow!(
                "plugin error: {}",
                result.message.unwrap_or_default()
            ));
        }

        let output_str = result
            .output
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .context("plugin returned no output")?;

        serde_json::from_str(&output_str).context("failed to parse plugin output")
    }

    async fn load_state(&self, group_id: i64) -> Result<PartyState> {
        let path = self.state_path(group_id);
        if !path.exists() {
            return Ok(PartyState::default());
        }
        let text = tokio::fs::read_to_string(&path)
            .await
            .context("failed to read party state file")?;
        serde_json::from_str(&text).context("invalid party state JSON")
    }

    async fn save_state(&self, group_id: i64, state: &PartyState) -> Result<()> {
        let path = self.state_path(group_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("failed to create party state directory")?;
        }
        let text = serde_json::to_string_pretty(state).context("failed to serialize party state")?;
        tokio::fs::write(&path, text)
            .await
            .context("failed to write party state file")?;
        Ok(())
    }

    fn state_path(&self, group_id: i64) -> PathBuf {
        self.data_dir.join(format!("faf-party-{group_id}.json"))
    }

    async fn schedule_notifications(
        &self,
        group_id: i64,
        candidates: Vec<Candidate>,
        window: (DateTime<FixedOffset>, DateTime<FixedOffset>),
    ) {
        // Cancel any pending retry loop for this group.
        {
            let mut active = self.active_notifications.lock().await;
            if let Some(old) = active.remove(&group_id) {
                old.abort();
            }
        }

        let action_tx = self.action_tx.clone();
        let data_dir = self.data_dir.clone();
        let active_notifications = self.active_notifications.clone();

        let handle = tokio::spawn(async move {
            let user_ids: Vec<i64> = candidates.iter().map(|c| c.user_id).collect();
            let count = candidates.len();
            let (win_start, win_end) = window;

            for attempt in 0..=RETRY_DELAYS.len() {
                if attempt > 0 {
                    tokio::time::sleep(RETRY_DELAYS[attempt - 1]).await;
                }

                let text = format_party_notification(count, win_start, win_end, attempt);
                let action = if attempt == 0 {
                    Action::send_group_msg_with_mentions(group_id, &user_ids, &text, None)
                } else {
                    Action::send_group_msg(group_id, text, None)
                };

                let _ = action_tx.send(action);
                info!(group_id, attempt, "sent faf-party notification");
            }

            // After the final retry, clear the candidate list.
            let state_path = data_dir.join(format!("faf-party-{group_id}.json"));
            if let Err(e) = tokio::fs::write(&state_path, b"{\"candidates\":[]}").await {
                warn!(group_id, error = %e, "failed to clear party state after retries");
            }

            let mut active = active_notifications.lock().await;
            active.remove(&group_id);
        });

        let mut active = self.active_notifications.lock().await;
        active.insert(group_id, handle);
    }
}

fn format_party_notification(
    count: usize,
    win_start: DateTime<FixedOffset>,
    win_end: DateTime<FixedOffset>,
    attempt: usize,
) -> String {
    let window_desc = format!("{} - {}", format_time(win_start), format_time(win_end));
    match attempt {
        0 => format!("🎮 已有 {count} 位玩家可以组队开黑！重叠时间：{window_desc}。速来！"),
        1 => format!("🎮 还有 {count} 位玩家在等，重叠时间：{window_desc}。还有人吗？"),
        _ => format!("🎮 最后召集：{count} 位玩家，重叠时间：{window_desc}。"),
    }
}

fn format_time(dt: DateTime<FixedOffset>) -> String {
    dt.format("%H:%M").to_string()
}

/// Find a time window where at least `PARTY_SIZE` candidates overlap.
fn find_overlap_window(
    candidates: &[Candidate],
) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    if candidates.len() < PARTY_SIZE {
        return None;
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Start,
        End,
    }

    let mut events: Vec<(DateTime<FixedOffset>, Kind)> = Vec::new();
    for c in candidates {
        events.push((c.start, Kind::Start));
        events.push((c.end, Kind::End));
    }
    // Sort by time; for equal times, Start comes before End so overlapping at
    // the boundary still counts.
    events.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| match (a.1, b.1) {
            (Kind::Start, Kind::End) => std::cmp::Ordering::Less,
            (Kind::End, Kind::Start) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    });

    let mut active = 0;
    let mut current_start: Option<DateTime<FixedOffset>> = None;
    let mut best: Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> = None;

    for (time, kind) in events {
        match kind {
            Kind::Start => {
                active += 1;
                if active >= PARTY_SIZE && current_start.is_none() {
                    current_start = Some(time);
                }
            }
            Kind::End => {
                if active >= PARTY_SIZE {
                    if let Some(start) = current_start {
                        best = Some((start, time));
                    }
                }
                active = active.saturating_sub(1);
                if active < PARTY_SIZE {
                    current_start = None;
                }
            }
        }
    }

    best
}

#[allow(dead_code)]
fn _state_path_for_group(data_dir: &Path, group_id: i64) -> PathBuf {
    data_dir.join(format!("faf-party-{group_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(hour: u32, minute: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 6, 18, hour, minute, 0)
            .unwrap()
    }

    fn candidate(user_id: i64, start: DateTime<FixedOffset>, end: DateTime<FixedOffset>) -> Candidate {
        Candidate {
            user_id,
            start,
            end,
            joined_at: start,
        }
    }

    #[test]
    fn no_overlap_with_five_candidates() {
        let candidates = vec![
            candidate(1, dt(19, 0), dt(22, 0)),
            candidate(2, dt(19, 0), dt(22, 0)),
            candidate(3, dt(19, 0), dt(22, 0)),
            candidate(4, dt(19, 0), dt(22, 0)),
            candidate(5, dt(19, 0), dt(22, 0)),
        ];
        assert!(find_overlap_window(&candidates).is_none());
    }

    #[test]
    fn six_candidates_overlap() {
        let candidates = vec![
            candidate(1, dt(19, 0), dt(22, 0)),
            candidate(2, dt(19, 0), dt(22, 0)),
            candidate(3, dt(19, 0), dt(22, 0)),
            candidate(4, dt(19, 0), dt(22, 0)),
            candidate(5, dt(19, 0), dt(22, 0)),
            candidate(6, dt(19, 0), dt(22, 0)),
        ];
        let (start, end) = find_overlap_window(&candidates).unwrap();
        assert_eq!(start, dt(19, 0));
        assert_eq!(end, dt(22, 0));
    }

    #[test]
    fn partial_overlap_finds_common_window() {
        let candidates = vec![
            candidate(1, dt(19, 0), dt(22, 0)),
            candidate(2, dt(19, 0), dt(22, 0)),
            candidate(3, dt(19, 0), dt(22, 0)),
            candidate(4, dt(20, 0), dt(22, 0)),
            candidate(5, dt(20, 0), dt(22, 0)),
            candidate(6, dt(20, 0), dt(21, 0)),
        ];
        let (start, end) = find_overlap_window(&candidates).unwrap();
        assert_eq!(start, dt(20, 0));
        assert_eq!(end, dt(21, 0));
    }
}
