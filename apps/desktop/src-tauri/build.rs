fn main() {
    // tauri-build embeds the icon as the executable's icon resource (which the windows' taskbar
    // icons are loaded from, src/win_icon.rs) but does not ask to rerun when it changes.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}
