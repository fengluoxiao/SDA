//! Floating-point Lo/Ro compatibility downmix. No HRTF, distance, room or EQ.
pub fn gains(label: &str) -> [f32; 2] {
    let side = std::f32::consts::FRAC_1_SQRT_2;
    match label {
        "L" | "Left" | "FrontLeft" | "FL" => [1.0, 0.0],
        "R" | "Right" | "FrontRight" | "FR" => [0.0, 1.0],
        "LFE" | "LFE1" | "LFE2" | "Lfe" | "Lfe2" => [0.0; 2],
        "C" | "Center" | "Centre" | "FrontCenter" | "CenterBack" | "BackCenter" | "Cb" | "Tc" | "Tfc" | "Tbc" => [side; 2],
        "Ls" | "Lb" | "Lc" | "Lsc" | "Lsd" | "Lw" | "Tfl" | "Tbl" | "Tsl" | "LeftSurround" | "SurroundLeft" | "SideLeft" | "BackLeft" | "RearLeft" | "TopFrontLeft" | "TopBackLeft" | "TopRearLeft" => [side, 0.0],
        "Rs" | "Rb" | "Rc" | "Rsc" | "Rsd" | "Rw" | "Tfr" | "Tbr" | "Tsr" | "RightSurround" | "SurroundRight" | "SideRight" | "BackRight" | "RearRight" | "TopFrontRight" | "TopBackRight" | "TopRearRight" => [0.0, side],
        _ => [side; 2],
    }
}

pub fn matrix(labels: impl IntoIterator<Item = String>) -> Vec<[f32; 2]> {
    // FFmpeg's floating-point -ac 2 path retains unity L/R coefficients.
    // The player applies its existing linked peak guard after summing.
    labels.into_iter().map(|label| gains(&label)).collect()
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn stereo_is_unity_and_lfe_is_excluded() {
        assert_eq!(matrix(["L".into(),"R".into(),"LFE".into()]),vec![[1.0,0.0],[0.0,1.0],[0.0,0.0]]);
    }
    #[test] fn five_one_matches_ffmpeg_float_impulse_matrix() {
        let m=matrix(["L","R","C","LFE","Ls","Rs"].map(str::to_string));
        assert_eq!(m[0],[1.0,0.0]);assert!((m[2][0]-0.70710677).abs()<1e-7);
        assert_eq!(gains("SurroundLeft"),gains("Ls"));assert_eq!(gains("SurroundRight"),gains("Rs"));
        assert_eq!(m[2][0],m[2][1]);assert_eq!(m[4][0],m[5][1]);
    }
}
