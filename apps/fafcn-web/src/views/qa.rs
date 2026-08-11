use dioxus::prelude::*;
use faf_dioxus_ui::components::{AgentChat, AgentChatConfig};

const ASK_STREAM_URL: &str = "http://localhost:3000/api/ask/stream";
const STORAGE_KEY: &str = "fafcn_qa_state";

#[component]
pub fn Qa() -> Element {
    rsx! {
        AgentChat {
            config: AgentChatConfig {
                stream_url: ASK_STREAM_URL.to_string(),
                storage_key: Some(STORAGE_KEY.to_string()),
                title: "FAF Q&A".to_string(),
                subtitle: Some(
                    "Ask anything about Forged Alliance Forever units and economy.".to_string(),
                ),
                placeholder: "Ask about a unit, build order, or economy...".to_string(),
                suggestions: vec![
                    "Explain the Cybran Monkey Lord".to_string(),
                    "What is a good build order for UEF?".to_string(),
                    "How do mass extractors work?".to_string(),
                ],
            },
        }
    }
}
