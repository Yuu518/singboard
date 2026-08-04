pub mod params;
pub mod wrapper;

/// 服务宿主的版本标记，面板据此判断已部署副本是否需要热换。
/// 宿主行为有变化时手动提升 service-host/Cargo.toml 的 version。
pub const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");
