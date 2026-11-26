use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_poppler_glib)");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" {
        return;
    }

    let has_poppler = Command::new("pkg-config")
        .args(["--exists", "poppler-glib", "cairo"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if has_poppler {
        println!("cargo:rustc-cfg=has_poppler_glib");
        println!("cargo:rustc-link-lib=poppler-glib");
        println!("cargo:rustc-link-lib=cairo");
        println!("cargo:rustc-link-lib=gobject-2.0");
        println!("cargo:rustc-link-lib=glib-2.0");
    }
}

