use std::{env,fs,path::PathBuf};
fn main(){
 let root=PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../vendor/faad2");
 // ASIO exports a C get_sample_rate helper too; keep FAAD internal symbols namespaced.
 let mut build=cc::Build::new();build.include(root.join("include")).include(root.join("libfaad"))
  .file("src/aac_bridge.c").warnings(false).opt_level(2).flag_if_supported("-std=c99")
  .define("get_sample_rate","sda_faad_get_sample_rate")
  .define("HAVE_INTTYPES_H","1").define("HAVE_STDINT_H","1").define("HAVE_MEMCPY","1")
  .define("HAVE_STRING_H","1").define("HAVE_SYS_STAT_H","1").define("HAVE_SYS_TYPES_H","1")
  .define("HAVE_LRINTF","1").define("LC_ONLY_DECODER","1").define("DISABLE_SBR","1")
  .define("FAAD2_VERSION","\"2.11.4\"").define("PACKAGE_VERSION","\"2.11.4\"");
 if env::var("CARGO_CFG_TARGET_ENV").as_deref()!=Ok("msvc"){build.define("HAVE_STRINGS_H","1");}
 let mut files=fs::read_dir(root.join("libfaad")).unwrap().filter_map(Result::ok).map(|entry|entry.path()).filter(|path|path.extension().and_then(|value|value.to_str())==Some("c")).collect::<Vec<_>>();files.sort();
 for file in files{build.file(file);}
 build.compile("sda_faad_lc");
 if env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("windows"){println!("cargo:rustc-link-lib=m");}
 println!("cargo:rerun-if-changed=src/aac_bridge.c");println!("cargo:rerun-if-changed={}",root.display());
}
