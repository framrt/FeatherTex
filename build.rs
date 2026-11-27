use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/feathertex.rc");
    println!("cargo:rerun-if-changed=assets/feathertex.ico");
    println!("cargo:rerun-if-changed=assets/icon_64.rgba");
    println!("cargo:rerun-if-changed=assets/feathertex-256.png");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("feathertex.res");

    let rc_ok = Command::new("rc.exe")
        .args(["/nologo", "/fo"])
        .arg(&res_path)
        .arg("assets/feathertex.rc")
        .status()
        .is_ok_and(|s| s.success());

    let windres_ok = if rc_ok {
        true
    } else {
        let obj_path = out_dir.join("feathertex_res.o");
        let ok = Command::new("windres")
            .args(["assets/feathertex.rc", "-O", "coff", "-o"])
            .arg(&obj_path)
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj_path.display());
        }
        ok
    };

    if rc_ok {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
    } else if !windres_ok {
        println!("cargo:warning=Could not embed Windows .ico resource (rc.exe/windres not found)");
    }
}
