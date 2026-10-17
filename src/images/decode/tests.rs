use super::{MAX_DECODED_BYTES, MAX_DIMENSION, MAX_ENCODED_BYTES, decode, output_len};
use image::{AnimationDecoder, ExtendedColorType, Frame, ImageEncoder, Rgba, RgbaImage};
use std::io::Cursor;

fn png(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(pixels, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    encoded
}

#[test]
fn raster_pixels_use_bgra_order_and_preserve_alpha() {
    let decoded = decode(&png(&[10, 20, 30, 40, 255, 100, 50, 255], 2, 1)).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.bgra, [30, 20, 10, 40, 50, 100, 255, 255]);

    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
        .write_image(&[10, 20, 30, 255], 1, 1, ExtendedColorType::Rgba8)
        .unwrap();
    assert_eq!(decode(&webp).unwrap().bgra, [30, 20, 10, 255]);
}

#[test]
fn malformed_unknown_and_oversized_inputs_fail_without_a_pixel_buffer() {
    for bytes in [
        b"".as_slice(),
        b"not an image",
        b"<svg>",
        b"\x89PNG\r\n\x1a\n",
    ] {
        assert!(decode(bytes).is_err());
    }
    assert!(
        decode(&vec![0; MAX_ENCODED_BYTES + 1])
            .unwrap_err()
            .contains("16 MiB")
    );

    let mut gif = gif(&[[1, 2, 3, 255]]);
    // A valid header with a huge logical screen is rejected before decoding the
    // first tiny frame, including screens within the individual side limits.
    for (width, height) in [(MAX_DIMENSION + 1, 1), (4096, 4096)] {
        gif[6..8].copy_from_slice(&(width as u16).to_le_bytes());
        gif[8..10].copy_from_slice(&(height as u16).to_le_bytes());
        assert!(decode(&gif).unwrap_err().contains("limit"));
    }
    assert!(output_len(0, 1).is_err());
    assert!(output_len(u32::MAX, u32::MAX).is_err());
    assert_eq!(output_len(8192, 1024).unwrap(), MAX_DECODED_BYTES);
}

fn gif(colors: &[[u8; 4]]) -> Vec<u8> {
    let mut encoded = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
        for color in colors {
            encoder
                .encode_frame(Frame::new(RgbaImage::from_pixel(1, 1, Rgba(*color))))
                .unwrap();
        }
    }
    encoded
}

#[test]
fn animated_gif_decodes_only_first_frame_even_with_a_broken_later_frame() {
    let first = gif(&[[255, 0, 0, 255]]);
    let animated = gif(&[[255, 0, 0, 255], [0, 0, 255, 255]]);
    assert_eq!(decode(&animated).unwrap().bgra, [0, 0, 255, 255]);
    // Keep a complete first frame and only part of the second descriptor.
    let truncated = &animated[..first.len() + 4];
    assert_eq!(decode(truncated).unwrap().bgra, [0, 0, 255, 255]);
    assert!(
        image::codecs::gif::GifDecoder::new(Cursor::new(truncated))
            .unwrap()
            .into_frames()
            .collect_frames()
            .is_err()
    );
}

fn riff_chunk(target: &mut Vec<u8>, name: &[u8; 4], payload: &[u8]) {
    target.extend_from_slice(name);
    target.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    target.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        target.push(0);
    }
}

fn animated_webp() -> Vec<u8> {
    let mut payload = b"WEBP".to_vec();
    // VP8X: animation flag, reserved bytes, 1×1 canvas minus one.
    riff_chunk(&mut payload, b"VP8X", &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    riff_chunk(&mut payload, b"ANIM", &[0, 0, 0, 0, 0, 0]);
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let mut frame = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut frame)
            .write_image(&color, 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        // ANMF: x/y/width/height minus one, 100ms duration, no blend.
        let mut descriptor = vec![0; 16];
        descriptor[12] = 100;
        descriptor[15] = 2;
        descriptor.extend_from_slice(&frame[12..]);
        riff_chunk(&mut payload, b"ANMF", &descriptor);
    }
    let mut encoded = b"RIFF".to_vec();
    encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    encoded.extend_from_slice(&payload);
    encoded
}

#[test]
fn animated_webp_returns_first_frame_without_collecting_animation() {
    let encoded = animated_webp();
    let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&encoded)).unwrap();
    let frames = decoder.into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 2, "the fixture really is animated");
    assert_ne!(frames[0].buffer(), frames[1].buffer());
    assert_eq!(decode(&encoded).unwrap().bgra, [0, 0, 255, 255]);
}
