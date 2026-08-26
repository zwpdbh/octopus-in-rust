//! eframe GUI for non-technical players.
//!
//! 同步 tab (the default): one button syncs everything; a version panel shows
//! how the mirror compares to upstream. 上传 tabs: for VPN-having uploaders
//! to publish new files with the group token. 设置 tab: mirror address and
//! the FAForever / FAF Client folders.

mod app;
mod fonts;
mod self_update;
mod strings;
mod version_panel;
mod workers;

pub use app::run;
