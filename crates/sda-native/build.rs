use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("mpegh");
    for path in ["scripts/prepare-mpegh.mjs", "packages/core/mpegh/bridge.c", "vendor/libmpegh/decoder", "vendor/libmpegh/CMakeLists.txt", "vendor/libmpegh/.git/HEAD"] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    println!("cargo:rerun-if-env-changed=SDA_NODE");
    let status = Command::new(env::var_os("SDA_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("scripts/prepare-mpegh.mjs")).arg(&out).status()
        .expect("Node.js is required to prepare the same MPEG-H capture bridge as Windows (set SDA_NODE)");
    assert!(status.success(), "MPEG-H source preparation failed");
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(out.join("sources.json")).unwrap()).unwrap();
    let mut build = cc::Build::new();
    build.include(manifest["includes"].as_str().unwrap()).define("LC_LEVEL_4", None)
        .opt_level(2).warnings(false).flag_if_supported("-std=c99");
    for source in manifest["sources"].as_array().unwrap() { build.file(source.as_str().unwrap()); }
    // Deliberately no fast-math: match Windows WASM's floating-point decoder.
    build.compile("sda_mpegh");
}
