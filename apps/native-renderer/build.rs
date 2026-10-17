fn main() {
    let revision = std::env::var("GITHUB_SHA").ok().or_else(|| std::process::Command::new("git").args(["rev-parse", "HEAD"])
        .output().ok().filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SDA_RENDERER_REVISION={revision}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    // asio-sys can reuse its prebuilt archive without re-emitting the C++ runtime
    // link directive. Keep GNU Windows builds portable (no libstdc++ DLL needed).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
        && std::env::var_os("CARGO_FEATURE_CPAL_OUTPUT").is_some()
    {
        println!("cargo:rustc-link-arg=-Wl,-Bstatic");
        println!("cargo:rustc-link-arg=-Wl,--start-group");
        println!("cargo:rustc-link-arg=-lstdc++");
        println!("cargo:rustc-link-arg=-lwinpthread");
        println!("cargo:rustc-link-arg=-lssp");
        println!("cargo:rustc-link-arg=-lmingwex");
        println!("cargo:rustc-link-arg=-lmsvcrt");
        println!("cargo:rustc-link-arg=-lkernel32");
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        println!("cargo:rustc-link-arg=-Wl,-Bdynamic");
    }
}
