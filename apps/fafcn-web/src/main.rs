//! Web frontend for the FAF construction-plan simulator.
//!
//! Connects to `fafcn-server` via WebSocket to run simulations and renders
//! the streamed economy events with components from `faf-dioxus-ui`.

use dioxus::prelude::*;

mod components;
mod state;
mod utils;
mod views;

use views::{Home, Navbar, Simulate};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/simulate")]
        Simulate {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
    }
}
