use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // The prebuilt ONNX Runtime (ort-sys, pyke.io) targets the macOS 15
    // SDK; keep the deployment target in sync so the linker doesn't warn
    // about version gaps.
    println!("cargo:rustc-link-arg=-mmacosx-version-min=15.0");

    // The prebuilt ONNX Runtime references the Clang compiler-rt builtin
    // `___isPlatformVersionAtLeast` in its CoreML objects. rustc links with
    // -nodefaultlibs and never pulls in libclang_rt, so we pass the
    // toolchain archive to the linker explicitly.
    let resource_dir = Command::new("xcrun")
        .args(["clang", "-print-resource-dir"])
        .output()
        .expect("xcrun clang not found")
        .stdout;
    let resource_dir = String::from_utf8_lossy(&resource_dir).trim().to_string();
    let candidate = PathBuf::from(resource_dir).join("lib/darwin/libclang_rt.osx.a");
    assert!(
        candidate.exists(),
        "libclang_rt.osx.a not found at {}",
        candidate.display()
    );
    println!("cargo:rustc-link-arg={}", candidate.display());
}
