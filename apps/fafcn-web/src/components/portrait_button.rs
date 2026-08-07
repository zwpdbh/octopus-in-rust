use dioxus::prelude::*;

#[component]
pub fn PortraitButton(
    unit_id: String,
    label: String,
    selected: bool,
    on_click: EventHandler<()>,
) -> Element {
    let portrait_url = format!(
        "http://localhost:3000/api/portraits/{}",
        unit_id.to_ascii_uppercase()
    );
    let ring = if selected { "ring-2 ring-blue-500" } else { "" };

    rsx! {
        button {
            class: "flex flex-col items-center gap-1 p-2 rounded-lg bg-neutral-800 border border-neutral-700 hover:bg-neutral-750 transition-colors {ring}",
            onclick: move |_| on_click.call(()),
            img {
                class: "w-12 h-12 object-contain",
                src: "{portrait_url}",
                alt: "{label}",
            }
            span { class: "text-[10px] text-neutral-300 truncate max-w-[4.5rem]", "{label}" }
        }
    }
}
