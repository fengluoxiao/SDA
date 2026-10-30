mod mp3 { pub use sda_alac_file_regression::Mp3FileDecoder; }
#[path = "../../../packages/core/src/loudness.rs"] mod loudness;
mod balance { pub use crate::loudness::{LoudnessMeter, ReferenceLoudness as Measurement}; }
#[path = "../../../crates/sda-native/src/stereo_reference.rs"] mod stereo_reference;
use stereo_reference::{FreshStereoReference, CompleteReference, POLICY};
use std::{path::{Path, PathBuf}, fs, io::Write};
fn fixture(name:&str)->PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/sda-native/tests/fixtures").join(name) }
fn finish(reference:&mut FreshStereoReference, chunk:usize)->CompleteReference {
    loop { let step=reference.step(chunk).unwrap(); if step.done {return step.measurement.unwrap();} assert!(step.measurement.is_none()); }
}
fn temp()->PathBuf { std::env::temp_dir().join(format!("sda-fresh-reference-{}-{}.m4a",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())) }
#[test]
fn quiet_intro_requires_complete_source_for_alac_and_mp3() {
    for (name,alac) in [("loudness/quiet-intro-96000.m4a",true),("loudness/quiet-intro-44100.mp3",false)] {
        let path=fixture(name);let mut reference=FreshStereoReference::open(&path,alac).unwrap();
        let first=reference.step(65536).unwrap();assert!(!first.done && first.measurement.is_none(),"intro cannot become a complete reference");
        let full=finish(&mut reference,65536);assert_eq!(full.policy,POLICY);assert!(full.complete);assert_eq!(full.sample_rate,48000);assert!(full.frames>7*48000);
        let level=full.measurement.integrated_lufs.unwrap();assert!((-15.0..-5.0).contains(&level),"complete loud section was not measured: {level}");
        let mut decoder=if alac {mp3::Mp3FileDecoder::open_alac(path).unwrap()} else {mp3::Mp3FileDecoder::open(path).unwrap()};
        let pcm=decoder.read_interleaved(65536).unwrap();let mut intro=loudness::LoudnessMeter::new(48000);
        intro.push(&vec![pcm.iter().step_by(2).copied().collect(),pcm.iter().skip(1).step_by(2).copied().collect()]);
        assert!(intro.integrated().integrated_lufs.unwrap() < -35.0,"fixture must begin quietly");
        let repeated=reference.step(65536).unwrap();assert!(repeated.done);assert_eq!(repeated.frames,full.frames);
    }
}
#[test]
fn cookie_rate_and_chunk_partition_preserve_complete_reference() {
    for (name,rate) in [("alac/tone-96000-2ch-entry1.m4a",96000),("alac/tone-44100-2ch.m4a",44100)] {
        let path=fixture(name);let mut small=FreshStereoReference::open(&path,true).unwrap();let mut large=FreshStereoReference::open(&path,true).unwrap();
        let a=finish(&mut small,4096);let b=finish(&mut large,65536);assert_eq!(a.input_sample_rate,rate);assert_eq!(a.frames,b.frames);assert_eq!(a.measurement.integrated_lufs,b.measurement.integrated_lufs);assert_eq!(a.measurement.true_peak_dbtp,b.measurement.true_peak_dbtp);
    }
}
#[test]
fn cancellation_releases_source_and_reopening_measures_new_contents() {
    let path=temp();fs::copy(fixture("loudness/quiet-intro-96000.m4a"),&path).unwrap();
    let mut cancelled=FreshStereoReference::open(&path,true).unwrap();assert!(!cancelled.step(4096).unwrap().done);drop(cancelled);
    let moved=path.with_extension("moved.m4a");fs::rename(&path,&moved).unwrap();
    fs::copy(fixture("alac/tone-44100-2ch.m4a"),&path).unwrap();let mut replacement=FreshStereoReference::open(&path,true).unwrap();let value=finish(&mut replacement,4096);assert_eq!(value.input_sample_rate,44100);drop(replacement);
    fs::remove_file(path).unwrap();fs::remove_file(moved).unwrap();
}
#[test]
fn source_changes_and_partial_payloads_are_rejected() {
    let path=temp();fs::copy(fixture("loudness/quiet-intro-96000.m4a"),&path).unwrap();let mut reference=FreshStereoReference::open(&path,true).unwrap();reference.step(4096).unwrap();
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&[0]).unwrap();
    loop {match reference.step(65536) {Err(error)=>{assert!(error.contains("changed"),"{error}");break;},Ok(step)=>assert!(!step.done)}}
    drop(reference);fs::remove_file(path).unwrap();
    let mut complete=FreshStereoReference::open(fixture("alac/tone-96000-2ch-entry1-short.m4a"),true).unwrap();let mut payload=finish(&mut complete,4096);payload.validate().unwrap();payload.complete=false;assert!(payload.validate().is_err());
}
