use dioxus::prelude::*;
use dioxus_router::Router;
use gloo_net::http::Request;

use crate::route::Route;
use crate::types::UnitSummary;

#[component]
pub fn App() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        UnitsProvider {
            Router::<Route> {}
        }
    }
}

#[component]
fn UnitsProvider(children: Element) -> Element {
    let units = use_resource(|| async move {
        Request::get("/api/units")
            .send()
            .await
            .ok()?
            .json::<Vec<UnitSummary>>()
            .await
            .ok()
    });
    use_context_provider(|| units);
    rsx! { {children} }
}
