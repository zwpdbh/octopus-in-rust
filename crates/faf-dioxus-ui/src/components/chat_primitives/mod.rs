//! Reusable, presentation-only chat components styled after modern assistant
//! UIs (e.g. kimi.com/chat): centered transcript, plain assistant messages,
//! rounded composer, welcome hero and a history sidebar.

mod conversation;
mod history;
mod input;
mod message;
mod sidebar;
mod types;
mod welcome;

pub use conversation::Chat;
pub use history::ChatHistory;
pub use input::ChatInputArea;
pub use message::ChatMessage;
pub use sidebar::ChatSidebar;
pub use types::{ChatHistoryItem, ChatMessageItem, ToolCall};
pub use welcome::ChatWelcome;
