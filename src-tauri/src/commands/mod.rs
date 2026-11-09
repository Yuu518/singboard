#[cfg(target_os = "android")]
#[path = "android/app.rs"]
pub mod app;
pub mod binary;
#[cfg_attr(target_os = "android", path = "android/config.rs")]
pub mod config;
pub mod network;
mod ruleset;
#[cfg_attr(target_os = "android", path = "android/self_update.rs")]
pub mod self_update;
#[cfg_attr(target_os = "android", path = "android/service.rs")]
pub mod service;
pub mod srs;
pub mod update;
