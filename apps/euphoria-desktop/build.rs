use std::path::PathBuf;
use std::process::Command;

// The prebuilt ONNX Runtime (ort-sys, pyke.io) references the Clang
// compiler-rt builtin `___isPlatformVersionAtLeast` in its CoreML objects.
// rustc links with -nodefaultlibs and never pulls in libclang_rt, so we
// pass the SDK archive to the linker explicitly.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    println!("cargo:rerun-if-env-changed=SDKROOT");
    let sdk = std::env::var("SDKROOT").unwrap_or_else(|_| {
        Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .expect("xcrun not found")
            .stdout
            .into_owned()
    });
    let sdk = String::from_utf8_lossy(&sdk).trim().to_string();
    let mut root = PathBuf::from(sdk);
    let mut candidate = root.join("usr/lib/clang/lib/darwin/libclang_rt.osx.a");
    if !candidate.exists() {
        // Some SDK layouts nest the toolchain under the SDK directory.
        root.pop();
        candidate = root.join("usr/lib/clang/lib/darwin/libclang_rt.osx.a");
    }
    assert!(
        candidate.exists(),
        "libclang_rt.osx.a not found under {}",
        candidate.display()
    );
    println!("cargo:rustc-link-arg={}", candidate.display());
}
