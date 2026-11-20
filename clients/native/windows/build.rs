use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=panel.rc");
    println!("cargo:rerun-if-changed=../../../src-tauri/singboard.exe.manifest");
    println!("cargo:rerun-if-changed=../../../src-tauri/icons/icon.ico");
    embed_resource::compile_for("panel.rc", ["singboard"], embed_resource::NONE)
        .manifest_required()
        .expect("嵌入 Windows 图标");
    println!("cargo:rerun-if-env-changed=SINGBOARD_SERVICE_HOST");
    let host = PathBuf::from(
        env::var_os("SINGBOARD_SERVICE_HOST").expect("请通过 build.ps1 构建服务宿主"),
    );
    println!("cargo:rerun-if-changed={}", host.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::copy(host, out.join("singboard.service")).expect("嵌入 Windows 服务宿主");
}
