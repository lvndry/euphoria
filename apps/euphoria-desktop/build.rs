//! Build script for the desktop app.
//!
//! macOS is the only target with native library wiring, and **macOS 15 is the
//! minimum supported version**: the prebuilt ONNX Runtime (ort-sys, pyke.io)
//! is built against the macOS 15 SDK.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // Keep the deployment target in sync with the prebuilt runtime's SDK so
    // the linker doesn't warn about version gaps.
    println!("cargo:rustc-link-arg=-mmacosx-version-min=15.0");

    // `xcrun` is a macOS host tool; cross-building the macOS target from a
    // non-macOS host is not supported.
    let host_is_macos = std::env::var("CARGO_CFG_HOST_OS").as_deref() == Ok("macos");
    let resource_dir = Command::new("xcrun")
        .args(["clang", "-print-resource-dir"])
        .output()
        .unwrap_or_else(|err| {
            let note = if host_is_macos {
                String::new()
            } else {
                format!(
                    "\n(xcrun is a macOS host tool; cross-building for macOS from a {} host is not supported)",
                    std::env::var("CARGO_CFG_HOST_OS").unwrap_or_else(|_| "non-macOS".into())
                )
            };
            panic!("failed to run `xcrun clang -print-resource-dir`: {err}{note}")
        });
    if !resource_dir.status.success() {
        let stderr = String::from_utf8_lossy(&resource_dir.stderr);
        panic!(
            "`xcrun clang -print-resource-dir` exited with {:?}: {stderr}",
            resource_dir.status.code()
        );
    }
    let resource_dir = String::from_utf8_lossy(&resource_dir.stdout)
        .trim()
        .to_string();

    // The prebuilt ONNX Runtime references the Clang compiler-rt builtin
    // `___isPlatformVersionAtLeast` in its CoreML objects. rustc links with
    // -nodefaultlibs and never pulls in libclang_rt, so we pass the
    // toolchain archive to the linker explicitly.
    let candidate = PathBuf::from(resource_dir).join("lib/darwin/libclang_rt.osx.a");
    assert!(
        candidate.exists(),
        "libclang_rt.osx.a not found at {}",
        candidate.display()
    );
    println!("cargo:rustc-link-arg={}", candidate.display());
}
