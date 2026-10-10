fn main() {
    // Re-embed the Vite output whenever the frontend bundle changes.
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
