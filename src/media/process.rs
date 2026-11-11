//! Media image pipeline: bounded validation, decode, orientation and
//! re-encode of the three accepted formats.
//!
//! Every accepted upload is fully re-encoded before it is stored, so no
//! original bytes, metadata or color profile survives into delivery. The
//! source format is preserved (JPEG stays JPEG at a fixed quality, PNG and
//! WebP re-encode losslessly), and responsive variants are downscales of
//! the cleaned source. Codec choice is a measured cost decision, revisit it
//! together with variant widths when profiling real content.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};

use crate::error::{Error, InternalError};

/// Hard decode surface: Memory ceilings are enforced by image's own limits
/// (bounded RGBA allocation), side and pixel checks run after decode.
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 32_000_000;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;

/// Source JPEG quality: once, at the top of the file, a measured default.
const SOURCE_JPEG_QUALITY: u8 = 85;

/// Downscale targets; a variant is generated only when the cleaned source
/// is strictly wider, so every distinct size is served by exactly one row.
pub const VARIANT_WIDTHS: [u32; 3] = [640, 1280, 1920];

/// One re-encoded image buffer with its exact MIME type.
#[derive(Debug)]
pub struct Encoded {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

impl Encoded {
    /// File extension for object keys; one per accepted format.
    pub fn extension(&self) -> &'static str {
        match self.mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            _ => unreachable!("encoders only produce accepted mimes"),
        }
    }
}

/// A responsive downscale of the cleaned source.
#[derive(Debug)]
pub struct Variant {
    /// Object label: `w640` style.
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub encoded: Encoded,
}

impl Variant {
    /// Object label without extension.
    pub fn key_label(&self) -> &str {
        &self.label
    }
}

/// A fully processed upload.
#[derive(Debug)]
pub struct Processed {
    pub source: Encoded,
    pub variants: Vec<Variant>,
    /// Cleaned source dimensions, orientation applied.
    pub width: u32,
    pub height: u32,
}

impl Processed {
    /// Real quota footprint: source plus every variant.
    pub fn total_bytes(&self) -> i64 {
        let mut total = i64::try_from(self.source.bytes.len()).unwrap_or(i64::MAX);
        for variant in &self.variants {
            total = total.saturating_add(i64::try_from(variant.encoded.bytes.len()).unwrap_or(0));
        }
        total.min(i64::MAX - 1)
    }
}

/// Header-scan result: the exact committed MIME before a resource-heavy
/// decode begins.
#[derive(Debug, Clone, Copy)]
pub struct Sniff {
    pub mime: &'static str,
    format: ImageFormat,
}

impl Sniff {
    /// Object-key extension for sources of this format.
    pub fn extension(&self) -> &'static str {
        match self.mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            _ => unreachable!("sniff only maps accepted mimes"),
        }
    }
}

/// Only these formats pass; animated WebP is rejected outright rather than
/// flattened into a still frame, so a user never publishes something that
/// looks different from what they imported.
pub fn sniff(bytes: &[u8]) -> Result<Sniff, Error> {
    let guessed = image::guess_format(bytes);
    let mime = match guessed {
        Ok(ImageFormat::Jpeg) => "image/jpeg",
        Ok(ImageFormat::Png) => "image/png",
        Ok(ImageFormat::WebP) => "image/webp",
        _ => return Err(Error::UnsupportedMediaType),
    };
    let animated = match mime {
        "image/webp" => is_animated_webp(bytes),
        "image/png" => is_apng(bytes),
        _ => false,
    };
    if animated {
        return Err(Error::UnsupportedMediaType);
    }
    let format = match mime {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        "image/webp" => ImageFormat::WebP,
        _ => unreachable!("sniff only maps accepted mimes"),
    };
    Ok(Sniff { mime, format })
}

/// An animated WebP always opens with a `VP8X` chunk whose first flag
/// byte (offset 20, right after the 12-byte RIFF header, fourcc and chunk
/// size) carries the ANIMATION bit. A few lookups, never a full scan.
fn is_animated_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 21 || &bytes[12..16] != b"VP8X" {
        return false;
    }
    bytes[20] & 0b0000_0010 != 0
}

/// APNG is signalled by an `acTL` chunk appearing before the first top
/// level `IDAT`. Only real chunk boundaries count, so hostile payloads
/// hiding `IDAT`/`acTL` strings inside chunk data cannot fake animation —
/// and a genuine `acTL` is never missed, no matter where a decoder puts
/// it. The walk is bounded by the file's own chunk structure.
fn is_apng(bytes: &[u8]) -> bool {
    /// Eight bytes of PNG magic, then every chunk is
    /// `length(4) type(4)` + data + CRC(4): one walk step is
    /// `8 + length + 4` over the current chunk start.
    const CHUNK_STRIDE_FIXED: usize = 8;
    const CHUNK_CRC: usize = 4;
    if bytes.len() < 16 || bytes[..8] != b"\x89PNG\r\n\x1a\n"[..] {
        return false;
    }
    let mut offset = 8;
    while offset + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"));
        let chunk_type = &bytes[offset + 4..offset + 8];
        match chunk_type {
            b"acTL" => return true,
            b"IDAT" | b"IEND" => return false,
            _ => {}
        }
        let stride = CHUNK_STRIDE_FIXED + length as usize + CHUNK_CRC;
        // A corrupted length field would walk into garbage; detect clear
        // overflow and stop rather than looping over a huge stride.
        match offset.checked_add(stride) {
            Some(next) => offset = next,
            // Overflowing length gruff: treat as structurally broken.
            None => return false,
        }
    }
    false
}

/// Decodes, orients and re-encodes one image. The returned buffers are the
/// only bytes addressed for storing; originals are dropped here.
pub fn process(sniffed: Sniff, bytes: &[u8]) -> Result<Processed, Error> {
    if bytes.len() > (usize::MAX / 2) {
        return Err(Error::PayloadTooLarge);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);

    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(sniffed.format);
    reader.limits(limits);
    // Bounded decode: the built-in limits turn oversized headers into
    // ordinary errors instead of unconstrained allocations.
    let decoded = reader.decode().map_err(|e| decode_error(&e))?;
    let mut image = decoded.to_rgba8();
    // JPEG encodes reject alpha; RGBA is kept only for PNG/WebP fidelity.
    apply_orientation(bytes, &mut image);
    let width = image.width();
    let height = image.height();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::InvalidInput);
    }

    let dynamic = DynamicImage::ImageRgba8(image);
    let source = encode(&dynamic, sniffed.format)?;
    let mut variants = Vec::new();
    for variant_width in VARIANT_WIDTHS {
        if width <= variant_width {
            continue;
        }
        let scaled_height = scale_keep_ratio(height, width, variant_width);
        let resized = image::imageops::resize(
            &dynamic,
            variant_width,
            scaled_height,
            image::imageops::FilterType::Lanczos3,
        );
        let encoded = encode(&DynamicImage::from(resized), sniffed.format)?;
        variants.push(Variant {
            label: format!("w{variant_width}"),
            width: variant_width,
            height: scaled_height,
            encoded,
        });
    }
    Ok(Processed {
        source,
        variants,
        width,
        height,
    })
}

/// Keeps the aspect ratio: one integer multiplication and one division.
fn scale_keep_ratio(height: u32, width: u32, target_width: u32) -> u32 {
    (u64::from(height).saturating_mul(u64::from(target_width)) / u64::from(width).max(1))
        .try_into()
        .unwrap_or(1)
}

fn encode(image: &DynamicImage, format: ImageFormat) -> Result<Encoded, Error> {
    let mut buffer = Vec::new();
    match format {
        ImageFormat::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut buffer, SOURCE_JPEG_QUALITY);
            encoder.encode_image(image).map_err(|e| {
                Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
            })?;
        }
        _ => {
            image
                .write_to(&mut Cursor::new(&mut buffer), format)
                .map_err(|e| {
                    Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
                })?;
        }
    }
    let mime = mime_of(format);
    Ok(Encoded {
        mime,
        bytes: buffer,
    })
}

fn mime_of(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::WebP => "image/webp",
        _ => unreachable!("only accepted formats reach encode"),
    }
}

fn decode_error(error: &image::ImageError) -> Error {
    match error {
        image::ImageError::Limits(_) => Error::InvalidInput,
        image::ImageError::Decoding(_)
        | image::ImageError::Encoding(_)
        | image::ImageError::Unsupported(_)
        | image::ImageError::IoError(_) => Error::UnsupportedMediaType,
        _ => Error::UnsupportedMediaType,
    }
}

/// 1 = unrotated; values 5..8 additionally swap width and height. Decode
/// already can't trust user data, so EXIF failures degrade to "unrotated"
/// instead of rejecting the upload.
fn apply_orientation(bytes: &[u8], image: &mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>) {
    let Some(exif_reader) = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
        .and_then(|exif| {
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .map(|field| field.value.get_uint(0))
        })
    else {
        return;
    };
    let orientation = exif_reader.unwrap_or(1);
    match orientation {
        1 => {}
        2 => *image = image::imageops::flip_horizontal(image),
        3 => *image = image::imageops::rotate180(image),
        4 => *image = image::imageops::flip_vertical(image),
        5 => {
            *image = image::imageops::flip_horizontal(image);
            *image = image::imageops::rotate90(image);
        }
        6 => *image = image::imageops::rotate90(image),
        7 => {
            *image = image::imageops::flip_horizontal(image);
            *image = image::imageops::rotate270(image);
        }
        8 => *image = image::imageops::rotate270(image),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    /// Solid 8x6 blue PNG with a deterministic checksum — small fixture for
    /// the decode/encode circle.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = Vec::new();
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([10u8, 40, 200, 255]));
        let dynamic = DynamicImage::ImageRgba8(image);
        dynamic
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .expect("encode fixture");
        buffer
    }

    #[test]
    fn sniff_accepts_only_the_three_formats() {
        let png = png_bytes(4, 4);
        assert_eq!(sniff(&png).unwrap().mime, "image/png");
        assert!(sniff(b"not an image at all").is_err());
        assert!(sniff(&[]).is_err());
    }

    #[test]
    fn webp_animation_flag_is_rejected() {
        // VP8X layout: after the fixed 12-byte RIFF header and the chunk
        // header, offset 20 holds the flag byte, then 3 reserved bytes and
        // the 3-byte canvas size.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(26u32.to_le_bytes().as_slice());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(10u32.to_le_bytes().as_slice());
        bytes.push(0x02); // flags: ANIMATION bit set
        bytes.extend_from_slice(&[0; 3]); // reserved
        bytes.extend_from_slice(&[0, 1, 0]); // canvas width - 1
        bytes.extend_from_slice(&[0, 1, 0]); // canvas height - 1
        assert!(sniff(&bytes).is_err());
        bytes[20] = 0x10; // alpha bit only: not animated
        assert!(sniff(&bytes).is_ok(), "non-animated stays accepted");

        // A real chunk pipeline: a proper IHDR ahead of acTL means the walk
        // must resume at the right chunk after those 13 IHDR data bytes.
        let with_ihdr = {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            append_chunk(&mut bytes, b"IHDR", &[0; 13]);
            append_chunk(&mut bytes, b"acTL", &[0, 0, 0, 0]);
            append_chunk(&mut bytes, b"IDAT", b"pixel data");
            bytes
        };
        assert!(sniff(&with_ihdr).is_err(), "acTL after IHDR is animation");

        // Without acTL the walk flips through IHDR straight into IDAT.
        let still = {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            append_chunk(&mut bytes, b"IHDR", &[0; 13]);
            append_chunk(&mut bytes, b"IDAT", b"pixel data");
            bytes
        };
        assert!(sniff(&still).is_ok(), "real chunk walk with IHDR lands");

        // A tEXt chunk whose DATA happens to read acTL or IDAT changes
        // nothing: only real chunk types trigger the rejection.
        let text_payload = b"acTL\x00IDAT hidden inside data";
        let pixel = {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            append_chunk(&mut bytes, b"tEXt", text_payload);
            append_chunk(&mut bytes, b"IDAT", b"pixel data");
            bytes
        };
        assert!(
            sniff(&pixel).is_ok(),
            "chunk payload strings are not chunk types"
        );
    }

    fn append_chunk(target: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
        target.extend_from_slice((data.len() as u32).to_be_bytes().as_slice());
        target.extend_from_slice(chunk_type);
        target.extend_from_slice(data);
        target.extend_from_slice(&[0u8; 4]); // CRC, never validated here
    }

    #[test]
    fn processed_source_and_variants_have_exact_widths() {
        let bytes = png_bytes(800, 400);
        let sniffed = sniff(&bytes).unwrap();
        let processed = process(sniffed, &bytes).unwrap();
        assert_eq!(processed.source.mime, "image/png");
        assert_eq!(processed.width, 800);
        assert_eq!(processed.height, 400);
        // A 800-wide source generates only the smaller sizes.
        let labels = processed
            .variants
            .iter()
            .map(|variant| (variant.label.to_owned(), variant.width, variant.height))
            .collect::<Vec<_>>();
        assert_eq!(labels, vec![("w640".to_owned(), 640, 320)]);
    }

    #[test]
    fn tiny_sources_skip_variants() {
        let bytes = png_bytes(200, 100);
        let sniffed = sniff(&bytes).unwrap();
        let processed = process(sniffed, &bytes).unwrap();
        assert_eq!(processed.variants.len(), 0);
        assert_eq!(processed.width, 200);
    }

    #[test]
    fn corrupt_bytes_are_rejected_as_unsupported() {
        // Valid sniffing signature, garbage body.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice("A".repeat(2_000).as_bytes());
        assert!(process(sniff(&bytes).unwrap(), &bytes).is_err());
    }
}
