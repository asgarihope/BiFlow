use std::{fs, path::Path};

fn ensure_staged_helper(path: &str) {
    let path = Path::new(path);
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, []);
}

fn main() {
    println!("cargo:rerun-if-changed=../version");
    ensure_staged_helper("../packaging/staged/iran-split-helper");
    ensure_staged_helper("../packaging/staged/iran-split-helper.exe");
    println!("cargo:rerun-if-changed=../crates/iran-split-taskbar/windows.manifest");
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!(
            "../crates/iran-split-taskbar/windows.manifest"
        )),
    );
    tauri_build::try_build(attributes).expect("Tauri build configuration failed");
}
