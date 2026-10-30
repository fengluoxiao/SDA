//! Dolby OAMD trim metadata, independent of PCM/object gain and user balance.
//! ETSI TS 103 420 V1.2.1 clauses 5.5.12 / 5.6.5. Configuration order is
//! DAMF's 3x3 (surround tier + 3 * height tier), not nine named speaker layouts.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimConfiguration {
    /// 0 = renderer default, 1 = disabled, 2 = explicit custom controls.
    pub mode: u8,
    pub centre_db: Option<f32>,
    pub surround_db: Option<f32>,
    pub height_db: Option<f32>,
    pub top_bottom_y: Option<f32>,
    pub listener_y: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DolbyTrims {
    pub warp_mode: u8,
    pub global_mode: u8,
    pub configurations: Vec<TrimConfiguration>,
    /// Decoder OAMD indices resolved to actual renderer object IDs / bed labels.
    pub disabled_objects: Vec<u32>,
    pub disabled_beds: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimUpdate {
    pub sample_pos: u64,
    pub trims: DolbyTrims,
}

impl DolbyTrims {
    pub fn valid(&self) -> bool {
        self.warp_mode <= 1 && self.global_mode <= 2
            && (self.global_mode != 2 || self.configurations.len() == 9)
            && self.disabled_objects.len() <= 128 && self.disabled_beds.len() <= 128
            && self.configurations.len() <= 9
            && self.configurations.iter().all(|c| c.mode <= 2
                && [c.centre_db,c.surround_db,c.height_db].into_iter().flatten().all(|v| v.is_finite() && (-36.0..=6.0).contains(&v))
                && [c.top_bottom_y,c.listener_y].into_iter().flatten().all(|v| v.is_finite() && (-1.0..=1.0).contains(&v)))
    }
    pub fn configuration(&self, surround_tier: usize, height_tier: usize) -> Option<&TrimConfiguration> {
        if self.global_mode != 2 { return None; }
        self.configurations.get(surround_tier.min(2) + 3 * height_tier.min(2))
            .filter(|c| c.mode == 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_are_layout_tiers_not_channel_count_or_object_gain() {
        let t = DolbyTrims {global_mode:2, configurations:(0..9).map(|i|TrimConfiguration{mode:2,centre_db:Some(i as f32-4.),..Default::default()}).collect(),..Default::default()};
        assert!(t.valid());
        assert_eq!(t.configuration(0,0).unwrap().centre_db,Some(-4.));
        assert_eq!(t.configuration(1,0).unwrap().centre_db,Some(-3.));
        assert_eq!(t.configuration(2,0).unwrap().centre_db,Some(-2.));
        assert_eq!(t.configuration(2,2).unwrap().centre_db,Some(4.));
        let round:DolbyTrims=serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();assert_eq!(t,round);
    }
    #[test]
    fn invalid_metadata_cannot_invent_gain_or_configuration_slots() {
        let mut t=DolbyTrims{global_mode:2,..Default::default()};assert!(!t.valid());
        t.configurations=vec![TrimConfiguration::default();9];assert!(t.valid());
        t.configurations[0].height_db=Some(f32::NAN);assert!(!t.valid());
    }
}
