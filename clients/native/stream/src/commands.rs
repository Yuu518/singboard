#[cfg(target_os = "android")]
#[path = "../../../../src-tauri/src/commands/android/config.rs"]
pub mod config;
#[path = "../../../../src-tauri/src/commands/network.rs"]
pub mod network;
#[path = "../../../../src-tauri/src/commands/ruleset.rs"]
mod ruleset;
#[cfg(target_os = "android")]
#[path = "../../../../src-tauri/src/commands/android/service.rs"]
pub mod service;
#[path = "../../../../src-tauri/src/commands/srs.rs"]
pub mod srs;
#[cfg(target_os = "android")]
#[path = "../../../../src-tauri/src/commands/update.rs"]
pub mod update;

#[cfg(windows)]
#[path = "../../../../src-tauri/src/commands/runtime_files.rs"]
pub mod runtime_files;
