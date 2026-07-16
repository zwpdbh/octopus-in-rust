use dioxus::prelude::*;
use dioxus_router::Routable;

use crate::pages::{Home, Scheduler, SimulateBuild};

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[route("/")]
    Home {},
    #[route("/simulate")]
    SimulateBuild {},
    #[route("/scheduler")]
    Scheduler {},
}
