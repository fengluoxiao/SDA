use crate::metadata::{hash, Metadata};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Debug)]
struct OwnedRoot {
    path: PathBuf,
    base: PathBuf,
}
impl OwnedRoot {
    fn new_in(base: &Path) -> Result<Self, String> {
        let base = base.canonicalize().map_err(|e| e.to_string())?;
        for _ in 0..100 {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("sda-format-{}-{n}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                            .map_err(|e| e.to_string())?;
                    }
                    return Ok(Self { path, base });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("Cannot create SDAR resource directory".into())
    }
}
impl Drop for OwnedRoot {
    fn drop(&mut self) {
        if let Ok(actual) = self.path.canonicalize() {
            if actual.parent() == Some(self.base.as_path())
                && actual
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("sda-format-"))
            {
                let _ = fs::remove_dir_all(&actual);
            }
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cover {
    pub path: PathBuf,
    pub mime_type: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Headphone {
    pub left: PathBuf,
    pub right: PathBuf,
    pub preamp: f32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedResources {
    #[serde(skip)]
    _owner: OwnedRoot,
    pub root: PathBuf,
    pub hrtf: PathBuf,
    pub hrtf_wet: f32,
    pub room: Option<PathBuf>,
    pub headphone: Option<Headphone>,
    pub covers: Vec<Cover>,
    pub lyrics: Vec<Value>,
}
fn str_field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("Missing authored SDAR {key}"))
}
fn safe_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|tail| tail.len() == 1 && matches!(tail.as_bytes()[0], b'1'..=b'9'));
    !device
        && !name.ends_with('.')
        && !name.is_empty()
        && name.len() < 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && name != "."
        && name != ".."
}
fn write_asset(metadata: &Metadata, id: &str, path: &Path) -> Result<(), String> {
    let asset = metadata.asset(id)?;
    if hash(&asset.data) != id {
        return Err("Authored SDAR asset checksum failed".into());
    }
    fs::write(path, &asset.data).map_err(|e| e.to_string())
}
fn write_hrtf(metadata: &Metadata, descriptor: &Value, root: &Path) -> Result<PathBuf, String> {
    let set = str_field(descriptor, "set")?;
    if !safe_name(set) || !set.starts_with("hrtf") {
        return Err("Invalid authored HRTF set name".into());
    }
    let directory = root.join(set);
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let manifest_id = str_field(descriptor, "manifest")?;
    let bytes = &metadata.asset(manifest_id)?.data;
    let manifest: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Authored HRTF metadata: {e}"))?;
    if manifest["sampleRate"] != 48000 {
        return Err("Authored HRTF must use 48 kHz".into());
    }
    let files = descriptor["files"]
        .as_object()
        .ok_or("Authored HRTF resources missing")?;
    for (name, id) in files {
        if !safe_name(name) || !(name.ends_with(".f32") || name.ends_with(".json")) {
            return Err("Invalid authored HRTF resource name".into());
        }
        write_asset(
            metadata,
            id.as_str().ok_or("Invalid HRTF asset ID")?,
            &directory.join(name),
        )?;
    }
    for position in manifest["positions"]
        .as_array()
        .ok_or("Authored HRTF positions missing")?
    {
        for key in ["dry", "wet"] {
            if let Some(name) = position[key].as_str() {
                if !safe_name(name) || !files.contains_key(name) {
                    return Err("Authored HRTF references an unavailable resource".into());
                }
            }
        }
    }
    if let Some(name) = manifest["triangleSupportFile"].as_str() {
        if !safe_name(name) || !files.contains_key(name) {
            return Err("Authored HRTF triangle support is unavailable".into());
        }
    }
    let path = directory.join("hrtf-set.json");
    write_asset(metadata, manifest_id, &path)?;
    Ok(path)
}
impl PreparedResources {
    pub fn new(metadata: &Metadata) -> Result<Self, String> {
        Self::new_in(metadata, &std::env::temp_dir())
    }
    pub fn new_in(metadata: &Metadata, base: &Path) -> Result<Self, String> {
        let assets = &metadata.render["assets"];
        if assets["schema"] != "sda.render-assets" || assets["version"] != 1 {
            return Err("This SDAR file does not contain its authored renderer resources".into());
        }
        let owner = OwnedRoot::new_in(base)?;
        let root = owner.path.clone();
        let hrtf = write_hrtf(metadata, &assets["hrtf"], &root)?;
        if assets["hrtf"]["companion"].is_object() {
            write_hrtf(metadata, &assets["hrtf"]["companion"], &root)?;
        }
        let hrtf_wet = assets["hrtf"]["wetWeight"]
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
            .ok_or("Invalid authored HRTF wet weight")? as f32;
        let cinema = &metadata.render["playbackSnapshot"]["cinema"];
        if cinema["settings"]["enabled"] == true {
            if let Some(id) = cinema["profileId"].as_str() {
                if assets["room"]["id"].as_str() != Some(id) {
                    return Err("Authored room response missing or mismatched".into());
                }
            }
        }
        let room = if assets["room"].is_object() {
            let id = str_field(&assets["room"], "profile")?;
            let path = root.join("room.json");
            write_asset(metadata, id, &path)?;
            Some(path)
        } else {
            None
        };
        let headphone = if assets["headphone"].is_object() {
            let h = &assets["headphone"];
            let preamp = h["preamp"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0. && *n <= 100.)
                .ok_or("Invalid authored headphone gain")? as f32;
            let (left, right) = (
                root.join("headphone-left.f32"),
                root.join("headphone-right.f32"),
            );
            for (key, path) in [("left", &left), ("right", &right)] {
                let id = str_field(h, key)?;
                let asset = metadata.asset(id)?;
                if asset.data.len() < 8
                    || asset.data.len() > 32768 * 4
                    || asset.data.len() % 4 != 0
                    || asset
                        .data
                        .chunks_exact(4)
                        .any(|v| !f32::from_le_bytes(v.try_into().unwrap()).is_finite())
                {
                    return Err("Invalid authored headphone FIR".into());
                }
                write_asset(metadata, id, path)?;
            }
            Some(Headphone {
                left,
                right,
                preamp,
            })
        } else {
            None
        };
        let mut covers = vec![];
        if let Some(pictures) = metadata.source["common"]["picture"].as_array() {
            for p in pictures {
                if let Some(id) = p["data"]["asset"].as_str() {
                    let asset = metadata.asset(id)?;
                    let path = root.join(format!("cover-{}.bin", id));
                    write_asset(metadata, id, &path)?;
                    covers.push(Cover {
                        path,
                        mime_type: asset.mime_type.clone(),
                        sha256: id.to_string(),
                    });
                }
            }
        }
        let mut lyrics = vec![];
        if let Some(items) = metadata.source["lyrics"].as_array() {
            for item in items {
                let id = str_field(item, "asset")?;
                let path = root.join(format!("lyrics-{id}.bin"));
                write_asset(metadata, id, &path)?;
                let mut copy = item.clone();
                copy["path"] = Value::String(path.to_string_lossy().into_owned());
                lyrics.push(copy);
            }
        }
        Ok(Self {
            _owner: owner,
            root,
            hrtf,
            hrtf_wet,
            room,
            headphone,
            covers,
            lyrics,
        })
    }
}
impl PreparedResources {
    pub fn renderer_scene(&self, metadata: &Metadata, layout: &str) -> Result<Value, String> {
        if ![
            "5.1", "5.1.2", "5.1.4", "7.1", "7.1.2", "7.1.4", "9.1", "9.1.2", "9.1.4", "9.1.6",
            "11.1.8", "22.2", "360RA-13",
        ]
        .contains(&layout)
        {
            return Err("Unsupported SDAR output layout".into());
        }
        let r = &metadata.render;
        let snapshot = &r["playbackSnapshot"];
        if snapshot["schema"] != "sda.playback-snapshot" || snapshot["version"] != 1 {
            return Err("The file lacks a complete authored playback snapshot".into());
        }
        let source_mutes = snapshot["routing"]["mutedSources"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let positions = r["scene"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, ch)| serde_json::json!({"id":format!("bed:{i}"),"position":ch["cartesian"],"gainDb":ch["gainDb"]}))
            .collect::<Vec<_>>();
        let muted = r["scene"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter_map(|(i, ch)| {
                source_mutes
                    .iter()
                    .any(|v| *v == ch["sourceId"])
                    .then(|| format!("bed:{i}"))
            })
            .collect::<Vec<_>>();
        let volume = &r["baked"]["volumeBalance"];
        let spatial = &r["baked"]["spatialBalance"];
        // Only the executable recipe crosses the 16 KiB native control protocol.
        // The complete provenance/asset manifest stays with the file and UI.
        let recipe = serde_json::json!({"schema":"sda.render-metadata","version":1,"playbackSnapshot":{
          "schema":"sda.playback-snapshot","version":1,"hrtf":{"directObjectHrtf":snapshot["hrtf"]["directObjectHrtf"]},
          "native":{"directionalHrtf":snapshot["native"]["directionalHrtf"],"masterPreampDb":snapshot["native"]["masterPreampDb"]},
          "cinema":{"settings":snapshot["cinema"]["settings"],"profileId":snapshot["cinema"]["profileId"]},"nearField":snapshot["nearField"],"sourceExtent":snapshot["sourceExtent"],
          "equalizer":{"bands":snapshot["equalizer"]["bands"],"lowFrequencyDiagnostic":snapshot["equalizer"]["lowFrequencyDiagnostic"]},
          "monitoring":{"comparisonGainDb":snapshot["monitoring"]["comparisonGainDb"]},
          "routing":{"mutedSpeakers":snapshot["routing"]["mutedSpeakers"],"activeFocus":snapshot["routing"]["activeFocus"]}}});
        let scene = serde_json::json!({"version":1,"layout":layout,"render":recipe,"resources":{"root":self.root,"hrtf":self.hrtf,"hrtfWet":self.hrtf_wet,"room":self.room,"headphone":self.headphone},
   "processing":{"enabled":true,"version":1,"programmeHandled":volume["applied"].as_bool().ok_or("Missing authored volume decision")? || !volume["enabled"].as_bool().ok_or("Missing authored volume enable flag")?,"sourceSpatialHandled":spatial["applied"].as_bool().ok_or("Missing authored spatial decision")? || !spatial["enabled"].as_bool().ok_or("Missing authored spatial enable flag")?,"spatialPresentation":spatial["enabled"].as_bool().ok_or("SDAR spatial processing decision missing")?,"stereoSource":true,"programmeGainDb":volume["programmeGainDb"],"makeupDb":volume["makeupDb"],"bedPositions":positions,"mutedSources":muted}});
        if serde_json::to_vec(&serde_json::json!({"type":"configureAuthoredScene","scene":scene}))
            .map_err(|e| e.to_string())?
            .len()
            > 16 * 1024
        {
            return Err(
                "Authored renderer control recipe exceeds this version's 16 KiB limit".into(),
            );
        }
        Ok(scene)
    }
}
