//! Raster and SVG decoding for remote images. Run this on a bounded background worker.
//!
//! The input, dimensions and output buffer have hard limits. `image::Limits`
//! additionally limits decoder allocations where the underlying codec supports
//! it; its allocation limit is explicitly best effort, unlike its size limits.

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

#[path = "decode/svg.rs"]
mod svg;

pub const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODER_ALLOC: u64 = 64 * 1024 * 1024;

/// One static frame in GPUI's BGRA channel order, with unmodified alpha.
#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Decode one raster image or SVG, using the first GIF/WebP animation frame.
/// Neither the filename nor HTTP type is trusted. SVG processing has separate
/// input/structure limits and cannot load nested images or external resources.
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err("Image download exceeds the 16 MiB limit".into());
    }
    if svg::looks_like_xml(bytes) {
        return svg::decode(bytes);
    }
    let format = image::guess_format(bytes).map_err(|_| unsupported())?;
    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Bmp
            | ImageFormat::Ico
            | ImageFormat::Tiff
            | ImageFormat::Qoi
    ) {
        return Err(unsupported());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    let output_len = output_len(width, height)?;
    // The source can be 16-bit RGBA, twice the size of the final BGRA8 buffer.
    if decoder.total_bytes() > MAX_DECODER_ALLOC {
        return Err("Image decoder buffer exceeds the 64 MiB limit".into());
    }
    // from_decoder reads a static image, rather than collecting animation frames.
    let mut bgra = DynamicImage::from_decoder(decoder)
        .map_err(decode_error)?
        .into_rgba8()
        .into_raw();
    debug_assert_eq!(bgra.len(), output_len);
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Decoded {
        width,
        height,
        bgra,
    })
}

fn output_len(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err("Image exceeds the 8192-pixel or 32 MiB decoded size limit".into());
    }
    let bytes = u64::from(width) * u64::from(height) * 4;
    if bytes > MAX_DECODED_BYTES as u64 {
        return Err("Image exceeds the 8192-pixel or 32 MiB decoded size limit".into());
    }
    Ok(bytes as usize)
}

fn unsupported() -> String {
    "Unsupported image format (PNG, JPEG, GIF, WebP, SVG, BMP, ICO, TIFF or QOI required)".into()
}

fn decode_error(error: image::ImageError) -> String {
    format!("Image could not be decoded: {error}")
}

#[cfg(test)]
#[path = "decode/tests.rs"]
mod tests;
