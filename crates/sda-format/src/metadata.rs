use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub const MAX_METADATA: usize = 64 * 1024 * 1024;
pub fn exact(file: &mut File, offset: u64, size: usize) -> Result<Vec<u8>, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if offset
        .checked_add(size as u64)
        .is_none_or(|end| end > length)
    {
        return Err("SDAR chunk extends past file".into());
    }
    let mut bytes = vec![0; size];
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
pub fn metadata_bytes(file: &mut File, offset: u64, size: u64) -> Result<Vec<u8>, String> {
    if size > MAX_METADATA as u64 {
        return Err("SDAR metadata is too large".into());
    }
    exact(file, offset, size as usize)
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Clone, Debug)]
pub struct Asset {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub filename: String,
}
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub render: Value,
    pub source: Value,
    pub assets: BTreeMap<String, Asset>,
}
impl Metadata {
    pub fn parse(render: Value, id3: Option<&[u8]>) -> Result<Self, String> {
        let (mut source, mut tagged_render, mut assets) = (Value::Null, None, BTreeMap::new());
        if let Some(id3) = id3 {
            parse_id3(id3, &mut source, &mut tagged_render, &mut assets)?;
        }
        if tagged_render.as_ref().is_some_and(|other| *other != render) {
            return Err("Conflicting SDAR render metadata".into());
        }
        let metadata = Self {
            render,
            source,
            assets,
        };
        metadata.validate()?;
        Ok(metadata)
    }
    pub fn validate(&self) -> Result<(), String> {
        let r = &self.render;
        if r["schema"] != "sda.render-metadata" || r["version"] != 1 || r["scene"]["kind"] != "bed"
        {
            return Err("Unsupported SDAR scene version".into());
        }
        if !r["interchange"].is_null() {
            let manifest = &r["interchange"];
            if manifest["schema"] != "sda.interchange"
                || manifest["version"] != 1
                || manifest["unsupportedRequiredFeature"] != "error"
            {
                return Err("Unsupported SDA interchange contract".into());
            }
            if !manifest["rendererContract"].is_null()
                && (manifest["rendererContract"]["id"] != "sda.authored-scene"
                    || manifest["rendererContract"]["version"] != 1)
            {
                return Err("Unsupported required SDA renderer contract".into());
            }
            for feature in manifest["requiredFeatures"]
                .as_array()
                .ok_or("Missing SDA required features")?
            {
                if !feature.as_str().is_some_and(|v| {
                    [
                        "bed-scene-v1",
                        "file-processing-v1",
                        "embedded-render-assets-v1",
                    ]
                    .contains(&v)
                }) {
                    return Err(format!("Unsupported required SDA feature: {feature}"));
                }
            }
        }
        for field in ["objects", "automation"] {
            if r["scene"][field].as_array().is_none_or(|v| !v.is_empty()) {
                return Err(
                    "SDAR Bed Channel With Objects is not supported by this version".into(),
                );
            }
        }
        let channels = r["scene"]["channels"]
            .as_array()
            .ok_or("Missing SDAR channels")?;
        if ![6, 12, 16].contains(&channels.len()) {
            return Err("Unsupported SDAR bed layout".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for (i, ch) in channels.iter().enumerate() {
            if ch["pcmChannel"].as_u64() != Some(i as u64 + 1)
                || ch["kind"] != "bed"
                || !ch["id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && ids.insert(id.to_string()))
            {
                return Err("Invalid SDAR channel identity/order".into());
            }
            let name = ch["name"].as_str().ok_or("SDAR channel role missing")?;
            if ![
                "FrontLeft",
                "FrontRight",
                "Center",
                "LFE",
                "RearLeft",
                "RearRight",
                "SurroundLeft",
                "SurroundRight",
                "TopFrontLeft",
                "TopFrontRight",
                "TopRearLeft",
                "TopRearRight",
                "WideLeft",
                "WideRight",
                "TopMiddleLeft",
                "TopMiddleRight",
            ]
            .contains(&name)
                || !names.insert(name)
            {
                return Err("Unsupported or duplicate SDAR bed role".into());
            }
            for (key, limit) in [("azimuth", 180.), ("elevation", 90.), ("distance", 4.)] {
                let v = ch["position"][key]
                    .as_f64()
                    .ok_or("Missing SDAR channel geometry")?;
                if !v.is_finite() || v.abs() > limit || key == "distance" && v < 0. {
                    return Err("Invalid SDAR channel geometry".into());
                }
            }
        }
        for channel in channels {
            let coordinates = channel["cartesian"]
                .as_array()
                .filter(|v| v.len() == 3)
                .ok_or("Missing SDAR Cartesian geometry")?;
            let az = channel["position"]["azimuth"]
                .as_f64()
                .unwrap()
                .to_radians();
            let el = channel["position"]["elevation"]
                .as_f64()
                .unwrap()
                .to_radians();
            let distance = channel["position"]["distance"].as_f64().unwrap();
            let expected = [
                -distance * el.cos() * az.sin(),
                distance * el.cos() * az.cos(),
                distance * el.sin(),
            ];
            if coordinates.iter().zip(expected).any(|(v, expected)| {
                v.as_f64()
                    .is_none_or(|n| !n.is_finite() || (n - expected).abs() > 1e-4)
            }) {
                return Err("Conflicting SDAR channel geometry".into());
            }
            if channel["gainDb"]
                .as_f64()
                .is_none_or(|v| !v.is_finite() || !(-120.0..=24.0).contains(&v))
            {
                return Err("Invalid authored channel gain".into());
            }
        }
        for name in ["volumeBalance", "spatialBalance"] {
            let operation = &r["baked"][name];
            if operation["applied"].as_bool().is_none() || operation["enabled"].as_bool().is_none()
            {
                return Err("Missing authored processing decision".into());
            }
        }
        let recipe = r["runtimeRecipe"]["operations"]
            .as_array()
            .ok_or("Missing authored runtime recipe")?;
        for operation in recipe {
            if !operation.as_str().is_some_and(|name| {
                [
                    "HRTF",
                    "headphone EQ",
                    "room simulation",
                    "monitor hardware",
                    "head tracking",
                    "speaker routing",
                ]
                .contains(&name)
            }) {
                return Err("Unsupported authored runtime operation".into());
            }
        }
        if r["scene"]["sampleRate"] != 48000
            || r["scene"]["durationSamples"]
                .as_u64()
                .is_none_or(|n| n == 0)
        {
            return Err("Invalid SDAR timeline".into());
        }
        if r["playbackPolicy"]["authority"] != "file-authored"
            || r["playbackPolicy"]["applyBakedProcessingAgain"] != false
            || r["playbackPolicy"]["overrideUserEnhancementPresets"] != true
        {
            return Err("Missing authored SDAR playback policy".into());
        }
        Ok(())
    }
    pub fn channels(&self) -> usize {
        self.render["scene"]["channels"].as_array().unwrap().len()
    }
    pub fn labels(&self) -> Vec<String> {
        self.render["scene"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["label"].as_str().unwrap_or("").to_string())
            .collect()
    }
    pub fn renderer_labels(&self) -> Vec<String> {
        self.render["scene"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap().to_string())
            .collect()
    }
    pub fn master_frames(&self) -> u64 {
        self.render["scene"]["durationSamples"].as_u64().unwrap()
    }
    pub fn asset(&self, id: &str) -> Result<&Asset, String> {
        self.assets
            .get(id)
            .ok_or_else(|| format!("Missing authored SDAR asset {id}"))
    }
    pub fn summary(&self) -> Value {
        let common = &self.source["common"];
        serde_json::json!({"title":common["title"],"artist":common["artist"],"album":common["album"],"albumArtist":common["albumartist"],"date":common["date"],"year":common["year"],"track":common["track"],"disc":common["disk"],"lyrics":self.source["lyrics"],"profile":"SDAR Bed Channel","channels":self.channels(),"durationSamples":self.master_frames(),"sampleRate":48000})
    }
}
fn syncsafe(b: &[u8]) -> Result<usize, String> {
    if b.len() != 4 || b.iter().any(|n| *n > 127) {
        return Err("Invalid SDAR ID3 size".into());
    }
    Ok(((b[0] as usize) << 21) | ((b[1] as usize) << 14) | ((b[2] as usize) << 7) | b[3] as usize)
}
fn zero_string<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a str, String> {
    let end = bytes
        .get(*at..)
        .and_then(|v| v.iter().position(|b| *b == 0))
        .map(|n| *at + n)
        .ok_or("Truncated SDAR tag string")?;
    let value = std::str::from_utf8(&bytes[*at..end]).map_err(|_| "Invalid SDAR tag UTF-8")?;
    *at = end + 1;
    Ok(value)
}
fn parse_id3(
    bytes: &[u8],
    source: &mut Value,
    render: &mut Option<Value>,
    assets: &mut BTreeMap<String, Asset>,
) -> Result<(), String> {
    if bytes.len() < 10 || &bytes[..3] != b"ID3" || bytes[3] != 4 || bytes[5] != 0 {
        return Err("Unsupported SDAR tag encoding".into());
    }
    let end = 10 + syncsafe(&bytes[6..10])?;
    if end > bytes.len() {
        return Err("Truncated SDAR tags".into());
    }
    let mut at = 10;
    while at + 10 <= end {
        if bytes[at..at + 4].iter().all(|n| *n == 0) {
            break;
        }
        let id = &bytes[at..at + 4];
        let size = syncsafe(&bytes[at + 4..at + 8])?;
        if bytes[at + 8..at + 10] != [0, 0] || at + 10 + size > end {
            return Err("Invalid SDAR ID3 frame".into());
        }
        let body = &bytes[at + 10..at + 10 + size];
        if id == b"GEOB" || id == b"APIC" {
            if body.first() != Some(&3) {
                return Err("Unsupported SDAR binary tag encoding".into());
            }
            let mut cursor = 1;
            let mime = zero_string(body, &mut cursor)?.to_string();
            let filename;
            if id == b"APIC" {
                cursor += 1;
                filename = "cover".to_string();
                let _ = zero_string(body, &mut cursor)?;
            } else {
                filename = zero_string(body, &mut cursor)?.to_string();
                let _ = zero_string(body, &mut cursor)?;
            }
            let data = body.get(cursor..).ok_or("Invalid SDAR tag payload")?;
            if filename == "source-metadata.json" {
                *source = serde_json::from_slice(data)
                    .map_err(|e| format!("Invalid SDAR source tags: {e}"))?;
            } else if filename == "render-metadata.json" {
                *render = Some(
                    serde_json::from_slice(data)
                        .map_err(|e| format!("Invalid SDAR render tags: {e}"))?,
                );
            } else {
                assets.entry(hash(data)).or_insert_with(|| Asset {
                    data: data.to_vec(),
                    mime_type: mime,
                    filename,
                });
            }
        }
        at += 10 + size;
    }
    Ok(())
}
#[derive(Debug)]
pub struct PcmSource {
    pub offset: u64,
    pub bytes: u64,
    pub channels: usize,
    pub rate: u32,
    pub bits: u16,
}
pub fn read_master(path: &Path, allow_wave: bool) -> Result<(File, PcmSource, Metadata), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let header = exact(&mut file, 0, 12)?;
    let private = &header[..4] == b"SDAR" && &header[8..12] == b"BCH1";
    if !private
        && !(allow_wave
            && [b"RIFF".as_slice(), b"RF64".as_slice()].contains(&&header[..4])
            && &header[8..12] == b"WAVE")
    {
        return Err("Not an SDAR bed master".into());
    }
    let (mut format, mut data, mut render, mut id3, mut ds64) = (None, None, None, None, None);
    let mut at = 12;
    while at + 8 <= size {
        let h = exact(&mut file, at, 8)?;
        let id = &h[..4];
        let mut length = u32::from_le_bytes(h[4..8].try_into().unwrap()) as u64;
        if id == b"data" && length == 0xffffffff {
            length = ds64.ok_or("SDAR RF64 size missing")?;
        }
        if at
            .checked_add(8)
            .and_then(|v| v.checked_add(length))
            .is_none_or(|end| end > size)
        {
            return Err("Truncated SDAR master chunk".into());
        }
        match id {
            b"ds64" => {
                let b = metadata_bytes(&mut file, at + 8, length)?;
                if b.len() < 28 {
                    return Err("Invalid SDAR ds64".into());
                }
                ds64 = Some(u64::from_le_bytes(b[8..16].try_into().unwrap()));
            }
            b"fmt " => {
                let b = metadata_bytes(&mut file, at + 8, length)?;
                if b.len() < 16 {
                    return Err("Invalid SDAR PCM format".into());
                }
                let u16at = |i| u16::from_le_bytes(b[i..i + 2].try_into().unwrap());
                let encoding = u16at(0);
                let channels = u16at(2) as usize;
                let rate = u32::from_le_bytes(b[4..8].try_into().unwrap());
                let bits = u16at(14);
                if ![1, 0xfffe].contains(&encoding)
                    || ![6, 12, 16].contains(&channels)
                    || bits != 24
                    || rate != 48000
                    || u16at(12) as usize != channels * 3
                {
                    return Err("Unsupported SDAR PCM format".into());
                }
                format = Some((channels, rate, bits));
            }
            b"data" => {
                if data.replace((at + 8, length)).is_some() {
                    return Err("Duplicate SDAR PCM data".into());
                }
            }
            b"sdrm" => {
                render = Some(
                    serde_json::from_slice(&metadata_bytes(&mut file, at + 8, length)?)
                        .map_err(|e| e.to_string())?,
                );
            }
            b"id3 " => id3 = Some(metadata_bytes(&mut file, at + 8, length)?),
            _ => {}
        }
        at += 8 + length + (length & 1);
    }
    let (channels, rate, bits) = format.ok_or("Missing SDAR PCM format")?;
    let (offset, bytes) = data.ok_or("Missing SDAR PCM data")?;
    let metadata = Metadata::parse(
        render.ok_or("Missing SDAR render metadata")?,
        id3.as_deref(),
    )?;
    if metadata.channels() != channels
        || bytes % (channels as u64 * 3) != 0
        || bytes / (channels as u64 * 3) != metadata.master_frames()
    {
        return Err("SDAR PCM length/channel metadata mismatch".into());
    }
    Ok((
        file,
        PcmSource {
            offset,
            bytes,
            channels,
            rate,
            bits,
        },
        metadata,
    ))
}
pub fn track_metadata(path: impl AsRef<Path>) -> Result<serde_json::Value, String> {
    let file = crate::BedFile::open_interchange(path)?;
    let common = &file.metadata.source["common"];
    let mut result = serde_json::json!({"title":common["title"],"artist":common["artist"],"album":common["album"],"albumArtist":common["albumartist"],"year":common["year"].as_i64().map(|year|year.to_string()),"date":common["date"],"durationMs":file.frames()as f64/48.,"sourceCodec":"sdar","authoredPlayback":true,"format":if file.transport.as_ref().is_some_and(|v|v["container"]!="sdar"){"SDA M4A"}else{"SDAR Bed Channel"}});
    if let Some(id) = common["picture"]
        .as_array()
        .and_then(|v| v.first())
        .and_then(|p| p["data"]["asset"].as_str())
    {
        let asset = file.metadata.asset(id)?;
        if asset.mime_type.starts_with("image/") {
            const ALPHABET: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut encoded = String::with_capacity(asset.data.len().div_ceil(3) * 4);
            for b in asset.data.chunks(3) {
                let a = b[0] as usize;
                let c = b.get(1).copied().unwrap_or(0) as usize;
                let d = b.get(2).copied().unwrap_or(0) as usize;
                encoded.push(ALPHABET[a >> 2] as char);
                encoded.push(ALPHABET[((a & 3) << 4) | (c >> 4)] as char);
                encoded.push(if b.len() > 1 {
                    ALPHABET[((c & 15) << 2) | (d >> 6)] as char
                } else {
                    '='
                });
                encoded.push(if b.len() > 2 {
                    ALPHABET[d & 63] as char
                } else {
                    '='
                });
            }
            result["coverUri"] =
                serde_json::Value::String(format!("data:{};base64,{encoded}", asset.mime_type));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod interchange_tests {
    use super::*;
    #[test]
    fn unknown_required_processing_is_not_silently_substituted() {
        let metadata = Metadata {
            render: serde_json::json!({"schema":"sda.render-metadata","version":1,"scene":{"kind":"bed"},"interchange":{"schema":"sda.interchange","version":1,"requiredFeatures":["future-vst3-engine-v1"],"unsupportedRequiredFeature":"error"}}),
            ..Default::default()
        };
        assert!(metadata
            .validate()
            .unwrap_err()
            .contains("Unsupported required SDA feature"));
    }
}

/// Fast content probe for authored WAV. It skips PCM and does not decode assets.
pub fn is_authored_wave(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(h) = exact(&mut file, 0, 12) else {
        return false;
    };
    if ![b"RIFF".as_slice(), b"RF64".as_slice()].contains(&&h[..4]) || &h[8..12] != b"WAVE" {
        return false;
    }
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    let (mut at, mut ds64) = (12u64, None);
    let mut chunks = 0;
    while at + 8 <= size && chunks < 100000 {
        chunks += 1;
        let Ok(h) = exact(&mut file, at, 8) else {
            return false;
        };
        let mut n = u32::from_le_bytes(h[4..8].try_into().unwrap()) as u64;
        if &h[..4] == b"ds64" {
            let Ok(b) = exact(&mut file, at + 8, 28) else {
                return false;
            };
            ds64 = Some(u64::from_le_bytes(b[8..16].try_into().unwrap()));
        }
        if &h[..4] == b"data" && n == 0xffffffff {
            let Some(length) = ds64 else { return false };
            n = length;
        }
        if at
            .checked_add(8)
            .and_then(|v| v.checked_add(n))
            .is_none_or(|v| v > size)
        {
            return false;
        }
        if &h[..4] == b"sdrm" {
            if n > 8 * 1024 * 1024 {
                return false;
            }
            let Ok(b) = exact(&mut file, at + 8, n as usize) else {
                return false;
            };
            let Ok(v) = serde_json::from_slice::<Value>(&b) else {
                return false;
            };
            return v["schema"] == "sda.render-metadata"
                && v["version"] == 1
                && v["scene"]["kind"] == "bed";
        }
        at += 8 + n + (n & 1);
    }
    false
}
