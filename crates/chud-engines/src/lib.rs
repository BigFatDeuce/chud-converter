//! Moteurs intégrés de Chud Converter.
//!
//! - [`image_native`] : images courantes, en Rust pur (crate `image`).
//! - [`data_native`] : JSON, NDJSON, YAML, TOML, CSV, TSV, en Rust pur.
//! - [`ffmpeg`] : adaptateur FFmpeg/ffprobe (audio, vidéo, animations, sous-titres, images).
//! - [`sevenzip`] : adaptateur 7-Zip (archives), avec validation préalable des entrées.
//! - Documents : [`pandoc`], [`libreoffice`], [`calibre`] (livres numériques).
//! - PDF : [`qpdf`], [`ghostscript`], [`tesseract`] (OCR).
//! - Images : [`imagemagick`], [`libvips`].
//! - Vidéo et analyse : [`mkvtoolnix`], [`mediainfo`], [`exiftool`].

pub mod calibre;
pub mod data_native;
pub mod exiftool;
pub mod ffmpeg;
pub mod ghostscript;
pub mod image_native;
pub mod imagemagick;
pub mod libreoffice;
pub mod libvips;
pub mod mediainfo;
pub mod mkvtoolnix;
pub mod pandoc;
pub mod qpdf;
mod qpdf_ops;
pub mod sevenzip;
pub mod tesseract;
pub mod text;
mod tool;
mod util;

use chud_core::Engine;
use std::sync::Arc;

/// Moteurs natifs, toujours disponibles.
pub fn native_engines() -> Vec<Arc<dyn Engine>> {
    vec![Arc::new(image_native::ImageNative::new()), Arc::new(data_native::DataNative::new())]
}
