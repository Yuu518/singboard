fn main() {
    // Prepare the embedded payload after the Tauri hook builds the release host.
    let manifest_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let release_dir = manifest_dir.join("target/release");
    let host = release_dir.join("singboard-service.exe");
    println!("cargo:rerun-if-changed={}", host.display());
    std::fs::copy(&host, release_dir.join("singboard.service"))
        .expect("build the release singboard-service helper before compiling the panel");

    if std::env::var("PROFILE").as_deref() == Ok("release") && tauri_build::is_dev() {
        println!(
            "cargo:warning=This optimized build still loads the development server. \
             For a distributable app use `pnpm tauri build` or enable `tauri/custom-protocol`."
        );
    }

    let mut attrs = tauri_build::Attributes::new();

    #[cfg(target_os = "windows")]
    {
        attrs = attrs.windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("singboard.exe.manifest")),
        );
    }

    tauri_build::try_build(attrs).expect("failed to build tauri app");
}
