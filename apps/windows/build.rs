use std::{env, fs, path::PathBuf};

fn version_parts() -> (u16, u16, u16) {
    let raw = env::var("SINGBOARD_RELEASE_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| env::var("CARGO_PKG_VERSION").ok())
        .unwrap_or_else(|| "0.0.0".into());
    let mut parts = raw.trim().trim_start_matches('v').split('.');
    let parse = |p: Option<&str>| {
        p.and_then(|s| s.split('-').next())
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0)
    };
    (parse(parts.next()), parse(parts.next()), parse(parts.next()))
}

fn main() {
    println!("cargo:rerun-if-changed=resources/panel.rc");
    println!("cargo:rerun-if-changed=resources/singboard.exe.manifest");
    println!("cargo:rerun-if-changed=resources/icons/icon.ico");
    println!("cargo:rerun-if-env-changed=SINGBOARD_RELEASE_VERSION");
    let (major, minor, patch) = version_parts();
    let macros = [
        format!("VER_MAJOR={major}"),
        format!("VER_MINOR={minor}"),
        format!("VER_PATCH={patch}"),
    ];
    embed_resource::compile_for("resources/panel.rc", ["singboard"], &macros)
        .manifest_required()
        .expect("嵌入 Windows 图标");
    println!("cargo:rerun-if-env-changed=SINGBOARD_SERVICE_HOST");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let dest = out.join("singboard.service");
    if let Some(host) = env::var_os("SINGBOARD_SERVICE_HOST").filter(|v| !v.is_empty()) {
        let host = PathBuf::from(host);
        println!("cargo:rerun-if-changed={}", host.display());
        fs::copy(host, dest).expect("嵌入 Windows 服务宿主");
        return;
    }
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile == "release" || profile == "windows-release" {
        panic!("请通过 scripts/build.ps1 构建服务宿主");
    }
    if let Some(candidate) = out
        .ancestors()
        .nth(3)
        .map(|dir| dir.join("singboard-service.exe"))
        .filter(|path| path.is_file())
    {
        println!("cargo:rerun-if-changed={}", candidate.display());
        fs::copy(candidate, dest).expect("嵌入 Windows 服务宿主");
    } else {
        fs::write(dest, []).expect("创建调试占位服务宿主");
    }
}
