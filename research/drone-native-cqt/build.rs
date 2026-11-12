//! The owner's experimental source stays outside Git; opt in to a frozen snapshot.
use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=CQT_OWNER_SNAPSHOT");
    println!("cargo:rustc-check-cfg=cfg(owner_snapshot)");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    let source = if let Some(directory) = env::var_os("CQT_OWNER_SNAPSHOT") {
        let root = PathBuf::from(directory)
            .canonicalize()
            .expect("snapshot exists");
        println!("cargo:rustc-cfg=owner_snapshot");
        println!("cargo:rerun-if-changed={}", root.join("cqt.rs").display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("halving.rs").display()
        );
        format!(
            "pub fn identity()->serde_json::Value {{ serde_json::json!({{\"cqt\":drone_native_cached::bytes_sha256(include_bytes!({:?})),\"halving\":drone_native_cached::bytes_sha256(include_bytes!({:?}))}}) }}\n#[path={:?}] mod halving;\n#[path={:?}] mod cqt;\npub fn features(x: &[f32]) -> (Vec<f32>,usize) {{ let f=cqt::features(x); (f.values,f.frame_count) }}",
            root.join("cqt.rs"),
            root.join("halving.rs"),
            root.join("halving.rs"),
            root.join("cqt.rs")
        )
    } else {
        "pub fn identity()->serde_json::Value { serde_json::Value::Null } pub fn features(_: &[f32]) -> (Vec<f32>,usize) { panic!(\"Build with CQT_OWNER_SNAPSHOT to test the external WIP\") }".into()
    };
    fs::write(output.join("owner.rs"), source).expect("write generated adapter");
}
