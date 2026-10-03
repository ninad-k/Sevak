fn main() {
    // Recompile native icon resources when the generated branding changes.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
