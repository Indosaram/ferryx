//! Links libghostty-vt. `FERRYX_GHOSTTY_VT_PREFIX` names a prefix that already
//! contains `lib/` and `include/` from `zig build -Demit-lib-vt`; otherwise the
//! library is built from the pinned submodule with the same procedure as the
//! desktop app (`src-tauri/native_terminal/build_ghostty.rs`).

#[path = "../../src-tauri/native_terminal/build_ghostty.rs"]
#[allow(dead_code)]
mod build_ghostty;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=FERRYX_GHOSTTY_VT_PREFIX");
    let target = env::var("TARGET").expect("TARGET");
    let lib_dir = match env::var("FERRYX_GHOSTTY_VT_PREFIX") {
        Ok(prefix) => {
            let lib_dir = PathBuf::from(prefix).join("lib");
            let (_, stem) = build_ghostty::resolve_static_lib(&lib_dir, &target).unwrap_or_else(|e| fail(&e));
            println!("cargo:rustc-link-lib=static={stem}");
            lib_dir
        }
        Err(_) => {
            let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
            let src_tauri = manifest.join("../../src-tauri");
            let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
            let cfg = build_ghostty::build_ghostty_vt_with_target_and_env(&src_tauri, &out, &target, None)
                .unwrap_or_else(|e| fail(&e));
            println!("cargo:rustc-link-lib=static={}", cfg.link_lib_stem);
            println!("cargo:rerun-if-changed=../../src-tauri/vendor/ghostty/build.zig");
            println!("cargo:rerun-if-changed=../../src-tauri/native_terminal/build_ghostty.rs");
            cfg.lib_dir
        }
    };
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
}

fn fail(e: &str) -> ! {
    eprintln!("\n[ferryx-vt] {e}\n");
    std::process::exit(1);
}
