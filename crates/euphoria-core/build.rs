// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the C++ wrapper around libkeyfinder and links the library, and
//! the threads library of FFTW that the wrapper uses, when the
//! `libkeyfinder` feature is enabled, and the C interface to the vendored
//! Signalsmith Stretch when the `pitch-shift` feature is, and clang's
//! runtime library for ONNX Runtime when the `stems` feature is, on macOS.

fn main() {
    println!("cargo:rerun-if-changed=src/detect/keyfinder_wrapper.cpp");
    println!("cargo:rerun-if-changed=src/signalsmith_shim.cpp");
    println!("cargo:rerun-if-changed=vendor");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    #[cfg(feature = "libkeyfinder")]
    build_keyfinder_wrapper();
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

#[cfg(feature = "libkeyfinder")]
fn build_keyfinder_wrapper() {
    let keyfinder = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("libkeyfinder")
        .expect("libkeyfinder was not found through pkg-config");
    let fftw = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("fftw3")
        .expect("FFTW, which libkeyfinder uses, was not found through pkg-config");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .includes(&keyfinder.include_paths)
        .file("src/detect/keyfinder_wrapper.cpp")
        .compile("euphoria_keyfinder");

    // Linkers that drop unreferenced libraries need libkeyfinder and FFTW
    // listed after the wrapper that calls them, so their link flags are
    // emitted only now. FFTW's threads library comes before FFTW itself,
    // which it calls.
    for search_path in keyfinder.link_paths.iter().chain(&fftw.link_paths) {
        println!("cargo:rustc-link-search=native={}", search_path.display());
    }
    for library in &keyfinder.libs {
        println!("cargo:rustc-link-lib={library}");
    }
    println!("cargo:rustc-link-lib=fftw3_threads");
    for library in &fftw.libs {
        println!("cargo:rustc-link-lib={library}");
    }
}
