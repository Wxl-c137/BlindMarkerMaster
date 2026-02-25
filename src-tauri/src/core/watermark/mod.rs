// Watermarking algorithm modules
pub mod encoder;
pub mod dwt;
pub mod dct;
pub mod embedder;
pub mod extractor;
pub mod json_marker;

pub use json_marker::JsonWatermarker;
