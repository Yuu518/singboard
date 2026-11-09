#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    singboard_lib::desktop::main();
    #[cfg(target_os = "android")]
    singboard_lib::run();
}
