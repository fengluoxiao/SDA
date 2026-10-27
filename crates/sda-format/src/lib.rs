//! Authored SDAR beds, SDA Bed Lossless and legacy private AAC delivery.
pub mod aac;
pub mod metadata;
pub mod mp4;
pub mod reader;
pub use reader::BedFile;
pub mod resources;
pub use resources::PreparedResources;
pub mod cli;

pub mod lossless;
