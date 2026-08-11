//! Reusable Dioxus UI primitives.
//!
//! This crate contains presentation-only components plus a client-side agent
//! chat feature (`components::agent_chat`) that streams from any SSE endpoint
//! speaking the kind-tagged agent event protocol. It must not depend on
//! application-specific types or business logic from `faf-db-web` or any other
//! app.

pub mod components;

pub use components::*;
// Re-export the color type so callers can configure charts without adding
// `plotters` as a direct dependency.
pub use plotters::prelude::RGBColor;
