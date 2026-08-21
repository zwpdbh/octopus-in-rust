//! eframe GUI for non-technical players.
//!
//! 同步 tab (the default): pick the gamedata folder (or let auto-detect find
//! it), click one button, done. 上传 tab: for VPN-having uploaders to publish
//! a new patch set with the group token.

mod app;
mod fonts;
mod self_update;
mod strings;
mod workers;

pub use app::run;
