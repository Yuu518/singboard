fn main() {
    println!("cargo:rerun-if-changed=service.rc");
    println!("cargo:rerun-if-changed=icons/service.ico");

    embed_resource::compile_for("service.rc", ["singboard-service"], embed_resource::NONE)
        .manifest_required()
        .expect("failed to compile service icon resource");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/Brepro");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--no-insert-timestamp");
    }
}
