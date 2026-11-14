fn main() {
    // Bei Änderungen am Starter-Vault neu kompilieren (er steckt per include_dir im Binary).
    println!("cargo:rerun-if-changed=starter-vault");
    tauri_build::build()
}
