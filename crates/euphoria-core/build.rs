fn main() {
    let keyfinder = pkg_config::Config::new()
        .probe("libkeyfinder")
        .expect("libkeyfinder was not found through pkg-config");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .includes(&keyfinder.include_paths)
        .file("src/detect/keyfinder_wrapper.cpp")
        .compile("euphoria_keyfinder");

    println!("cargo:rerun-if-changed=src/detect/keyfinder_wrapper.cpp");
}
