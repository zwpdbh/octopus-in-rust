use dioxus::prelude::*;
use faf_dioxus_ui::components::{AgentChat, AgentChatConfig};

use crate::i18n::{self, Text};

const STORAGE_KEY: &str = "fafcn_qa_state";

#[component]
pub fn Qa() -> Element {
    let t = i18n::use_t();
    rsx! {
        AgentChat {
            config: AgentChatConfig {
                stream_url: crate::net::api_url("/api/ask/stream"),
                storage_key: Some(STORAGE_KEY.to_string()),
                title: t.t(Text::QaTitle).to_string(),
                subtitle: Some(t.t(Text::QaSubtitle).to_string()),
                placeholder: t.t(Text::QaPlaceholder).to_string(),
                suggestions: vec![
                    t.t(Text::QaSuggestionMonkeylord).to_string(),
                    t.t(Text::QaSuggestionBuildOrder).to_string(),
                    t.t(Text::QaSuggestionMex).to_string(),
                ],
            },
        }
    }
}
