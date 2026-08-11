use dioxus::prelude::*;

use super::types::ChatHistoryItem;

/// A narrow sidebar with a "New chat" button and a list of recent chats.
#[component]
pub fn ChatSidebar(
    items: ReadSignal<Vec<ChatHistoryItem>>,
    active_id: Option<String>,
    on_new_chat: EventHandler,
    on_select: EventHandler<String>,
    #[props(default)] on_delete: Option<EventHandler<String>>,
) -> Element {
    rsx! {
        aside { class: "flex h-full w-64 shrink-0 flex-col border-r border-neutral-800/70 bg-neutral-900/40 text-neutral-100",
            div { class: "p-3",
                button {
                    class: "flex w-full items-center gap-2 rounded-xl border border-neutral-700/70 bg-neutral-800/60 px-3.5 py-2.5 text-sm font-medium text-neutral-100 hover:bg-neutral-800 transition-colors cursor-pointer",
                    onclick: move |_| on_new_chat.call(()),
                    svg {
                        class: "h-4 w-4 text-neutral-400",
                        fill: "none",
                        stroke: "currentColor",
                        stroke_width: "2",
                        view_box: "0 0 24 24",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            d: "M12 5v14m-7-7h14",
                        }
                    }
                    "New chat"
                }
            }
            div { class: "flex-1 overflow-y-auto px-3 pb-3",
                if !items.read().is_empty() {
                    div { class: "px-2 pb-2 text-xs font-medium text-neutral-500", "Recent" }
                }
                div { class: "flex flex-col gap-0.5",
                    for item in items.read().iter().cloned() {
                        ChatSidebarItem {
                            id: item.id.clone(),
                            title: item.title.clone(),
                            active: active_id.as_ref() == Some(&item.id),
                            on_select,
                            on_delete,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ChatSidebarItem(
    id: String,
    title: String,
    active: bool,
    on_select: EventHandler<String>,
    on_delete: Option<EventHandler<String>>,
) -> Element {
    let row_class = if active {
        "bg-neutral-800 text-neutral-100"
    } else {
        "text-neutral-400 hover:bg-neutral-800/60 hover:text-neutral-200"
    };
    let select_id = id.clone();
    rsx! {
        div {
            class: "group flex cursor-pointer items-center rounded-lg px-3 py-2 text-sm transition-colors {row_class}",
            onclick: move |_| on_select.call(select_id.clone()),
            title: "{title}",
            span { class: "flex-1 truncate", "{title}" }
            if let Some(on_delete) = on_delete {
                button {
                    class: "ml-1 hidden h-6 w-6 shrink-0 items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-700 hover:text-neutral-200 group-hover:flex",
                    aria_label: "Delete chat",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_delete.call(id.clone());
                    },
                    "✕"
                }
            }
        }
    }
}
