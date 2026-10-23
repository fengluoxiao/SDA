//! No sound is produced. Runs the production legacy Hexagon backend and precision gate.
#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
fn main() {
    let directory = std::env::args()
        .nth(1)
        .expect("app-private runtime directory");
    match sda_native_renderer::spectral_accelerator::probe_legacy_runtime(&directory) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"error":error,"playbackEndToEndQualified":false})
            );
            std::process::exit(2);
        }
    }
}
#[cfg(not(all(target_os = "android", feature = "pcm-diagnostic")))]
fn main() {
    eprintln!("Requires Android target and pcm-diagnostic feature");
    std::process::exit(2);
}
