//! Real Vulkan cooperation with synthetic sources and simulated policy hints.
#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
fn main() {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);
    if let Err(e) = sda_native_renderer::spectral_accelerator::probe_cooperative_audio(
        count,
        std::env::args().nth(2).as_deref() == Some("limited"),
    ) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(not(all(target_os = "android", feature = "pcm-diagnostic")))]
fn main() {
    eprintln!("Requires Android and pcm-diagnostic");
    std::process::exit(2);
}
