//! Moteurs intégrés de Chud Converter.
//!
//! - [`image_native`] : images courantes, en Rust pur (crate `image`).
//! - [`data_native`] : JSON, NDJSON, YAML, TOML, CSV, TSV, en Rust pur.
//! - [`ffmpeg`] : adaptateur FFmpeg/ffprobe (audio, vidéo, animations, sous-titres, images).
//! - [`sevenzip`] : adaptateur 7-Zip (archives), avec validation préalable des entrées.

pub mod data_native;
pub mod ffmpeg;
pub mod image_native;
pub mod sevenzip;
pub mod text;
mod util;

use chud_core::Engine;
use std::sync::Arc;

/// Moteurs natifs, toujours disponibles.
pub fn native_engines() -> Vec<Arc<dyn Engine>> {
    vec![Arc::new(image_native::ImageNative::new()), Arc::new(data_native::DataNative::new())]
}
