fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/Brepro");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--no-insert-timestamp");
    }
}
