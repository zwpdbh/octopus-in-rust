//! Web frontend for the faf-ml data platform.
//!
//! Talks to `faf-ml-server` over HTTP (see `crate::net`) and provides the
//! collect → review → snapshot workflow: upload screenshots in the gallery,
//! review/edit pre-existing bounding boxes in the label view, and freeze
//! labeled data into immutable dataset snapshots.

use dioxus::prelude::*;

mod net;
mod views;

use views::{Datasets, Gallery, Home, Label, Navbar};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/gallery")]
        Gallery {},
        #[route("/label/:id")]
        Label { id: String },
        #[route("/datasets")]
        Datasets {},
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
