// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the C++ wrapper around libkeyfinder and links the library, and
//! the threads library of FFTW that the wrapper uses, when the
//! `libkeyfinder` feature is enabled.

fn main() {
    println!("cargo:rerun-if-changed=src/detect/keyfinder_wrapper.cpp");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    #[cfg(feature = "libkeyfinder")]
    build_keyfinder_wrapper();
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
