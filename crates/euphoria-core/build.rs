// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the C interface to the vendored Signalsmith Stretch when the
//! `pitch-shift` feature is enabled, and links clang's runtime library for
//! ONNX Runtime when the `stems` feature is, on macOS.

fn main() {
    println!("cargo:rerun-if-changed=src/signalsmith_shim.cpp");
    println!("cargo:rerun-if-changed=vendor");
    #[cfg(feature = "pitch-shift")]
    build_signalsmith_shim();
    #[cfg(feature = "stems")]
    link_compiler_runtime_on_macos();
}

/// ONNX Runtime's Core ML code checks the macOS version with `@available`,
/// which calls `__isPlatformVersionAtLeast` from clang's runtime library.
/// rustc links without the compiler's default libraries, and some Rust
/// toolchains, such as Homebrew's, do not provide that function, so the
/// library is linked here when clang can say where it is.
#[cfg(feature = "stems")]
fn link_compiler_runtime_on_macos() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let Ok(output) = std::process::Command::new("clang")
        .arg("--print-runtime-dir")
        .output()
    else {
        return;
    };
    let directory = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success()
        && std::path::Path::new(&directory)
            .join("libclang_rt.osx.a")
            .is_file()
    {
        println!("cargo:rustc-link-search=native={directory}");
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
}

#[cfg(feature = "pitch-shift")]
fn build_signalsmith_shim() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include("vendor")
        // Upstream code: its warnings are not ours to fix.
        .warnings(false)
        // Unoptimised, the stretcher takes minutes over a track, which debug
        // builds and tests would otherwise wait for.
        .opt_level(2)
        .file("src/signalsmith_shim.cpp")
        .compile("euphoria_signalsmith");
}
