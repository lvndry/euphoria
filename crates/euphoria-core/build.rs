// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the C++ wrapper around libkeyfinder and links the library, when
//! the `libkeyfinder` feature is enabled.

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

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .includes(&keyfinder.include_paths)
        .file("src/detect/keyfinder_wrapper.cpp")
        .compile("euphoria_keyfinder");

    // Linkers that drop unreferenced libraries need libkeyfinder listed after
    // the wrapper that calls it, so its link flags are emitted only now.
    for search_path in &keyfinder.link_paths {
        println!("cargo:rustc-link-search=native={}", search_path.display());
    }
    for library in &keyfinder.libs {
        println!("cargo:rustc-link-lib={library}");
    }
}
