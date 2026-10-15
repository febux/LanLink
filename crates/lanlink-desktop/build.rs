fn main() {
    // Tauri embeds ./assets at build time. Without this rerun directive, cargo
    // may reuse a cached build that still contains an old version of app.js/HTML.
    println!("cargo:rerun-if-changed=./assets");
    tauri_build::build()
}
