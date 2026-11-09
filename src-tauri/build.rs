fn main() {
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows {
        // Prepare the embedded payload after the Tauri hook builds the release host.
        let manifest_dir =
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .map(|dir| if dir.is_absolute() { dir } else { manifest_dir.join(dir) })
            .unwrap_or_else(|| manifest_dir.join("target"));
        let release_dir = target_dir.join("release");
        let host = release_dir.join("singboard-service.exe");
        println!("cargo:rerun-if-changed={}", host.display());
        println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
        let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        std::fs::copy(&host, out_dir.join("singboard.service"))
            .expect("build the release singboard-service helper before compiling the panel");
    }

    if std::env::var("PROFILE").as_deref() == Ok("release") && tauri_build::is_dev() {
        println!(
            "cargo:warning=This optimized build still loads the development server. \
             For a distributable app use `pnpm tauri build` or enable `tauri/custom-protocol`."
        );
    }

    let mut attrs = tauri_build::Attributes::new();

    if target_windows {
        attrs = attrs.windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("singboard.exe.manifest")),
        );
    }

    tauri_build::try_build(attrs).expect("failed to build tauri app");
}
