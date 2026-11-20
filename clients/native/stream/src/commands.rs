#[cfg(target_os = "android")]
#[path = "commands/android/config.rs"]
pub mod config;
pub mod network;
mod ruleset;
#[cfg(target_os = "android")]
#[path = "commands/android/service.rs"]
pub mod service;
pub mod srs;
#[cfg(target_os = "android")]
pub mod update;

#[cfg(windows)]
pub mod runtime_files;
