//! Bounded PNG/JPEG decoding. Use `Context::decode_image` for untrusted/user files.
use crate::{Error, Image};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

pub const MAX_ENCODED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, Error> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(Error("image source exceeds 32 MiB".into()));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error(e.to_string()))?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err(Error("expected PNG or JPEG data".into()));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

/// Validate dimensions before allocating pixels (also used by the web worker).
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), Error> {
    let decoder = reader(bytes)?
        .into_decoder()
        .map_err(|e| Error(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(Error(
            "image exceeds 16 megapixels or has empty dimensions".into(),
        ));
    }
    Ok((w, h))
}

impl Image {
    /// Decode once into immutable straight-alpha RGBA8, applying EXIF orientation.
    /// Synchronous: use on a worker, or queue `Context::decode_image` from UI code.
    /// ICC colour-profile conversion and animation are not performed.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        dimensions(bytes)?;
        let mut decoder = reader(bytes)?
            .into_decoder()
            .map_err(|e| Error(e.to_string()))?;
        let orientation = decoder.orientation().map_err(|e| Error(e.to_string()))?;
        let mut decoded =
            image::DynamicImage::from_decoder(decoder).map_err(|e| Error(e.to_string()))?;
        decoded.apply_orientation(orientation);
        let rgba = decoded.into_rgba8();
        Self::rgba(rgba.width(), rgba.height(), rgba.into_raw())
    }
}

// A second WASM instance in a Web Worker runs the exact same bounded decoder.
// These exports are deliberately independent of Application/Host state.
#[cfg(target_arch = "wasm32")]
mod worker {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) }; }
    #[no_mangle]
    pub extern "C" fn rusterize_image_alloc(length: usize) -> *mut u8 {
        if length > MAX_ENCODED_BYTES {
            return std::ptr::null_mut();
        }
        Box::into_raw(vec![0u8; length].into_boxed_slice()) as *mut u8
    }
    /// # Safety
    /// Pointer/length must be the unique allocation returned by image_alloc.
    #[no_mangle]
    pub unsafe extern "C" fn rusterize_image_decode(pointer: *mut u8, length: usize) -> usize {
        if pointer.is_null() || length > MAX_ENCODED_BYTES {
            return 0;
        }
        let bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(pointer, length)) };
        let mut result = Vec::new();
        match Image::decode(&bytes) {
            Ok(image) => {
                result.extend_from_slice(&0u32.to_le_bytes());
                result.extend_from_slice(&image.width().to_le_bytes());
                result.extend_from_slice(&image.height().to_le_bytes());
                result.extend_from_slice(image.pixels());
            }
            Err(error) => {
                result.extend_from_slice(&1u32.to_le_bytes());
                result.extend_from_slice(error.0.as_bytes());
            }
        }
        let length = result.len();
        RESULT.with(|r| *r.borrow_mut() = result);
        length
    }
    #[no_mangle]
    pub extern "C" fn rusterize_image_result() -> *const u8 {
        RESULT.with(|r| r.borrow().as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    #[test]
    fn png_preserves_straight_alpha_and_clones_reuse_pixels() {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 7, 4, 0, 12, 34, 56, 128],
                2,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let image = Image::decode(&png).unwrap();
        assert_eq!(image.pixels(), &[255, 7, 4, 0, 12, 34, 56, 128]);
        let again = image.clone();
        assert_eq!(image.id(), again.id());
        assert_eq!(image.pixels().as_ptr(), again.pixels().as_ptr());
        png.truncate(png.len() / 2);
        assert!(Image::decode(&png).is_err());
        assert!(Image::decode(b"bad image").is_err());
    }
    #[test]
    fn jpeg_exif_orientation_is_applied() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(
                &[
                    255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
                ],
                2,
                3,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let original = Image::decode(&jpeg).unwrap();
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
        oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend_from_slice(exif);
        oriented.extend_from_slice(&jpeg[2..]);
        let rotated = Image::decode(&oriented).unwrap();
        assert_eq!((rotated.width(), rotated.height()), (3, 2));
        for y in 0..3 {
            for x in 0..2 {
                let from = (y * 2 + x) * 4;
                let to = (x * 3 + (2 - y)) * 4;
                assert_eq!(
                    &original.pixels()[from..from + 4],
                    &rotated.pixels()[to..to + 4]
                );
            }
        }
    }
    #[test]
    fn excessive_dimensions_fail_before_pixel_allocation() {
        let mut bytes = include_bytes!("../../../examples/media/assets/cover.png").to_vec();
        bytes[16..20].copy_from_slice(&16000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&16000u32.to_be_bytes());
        let mut crc = !0u32;
        for byte in &bytes[12..29] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320u32.wrapping_mul(crc & 1));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        assert!(dimensions(&bytes).unwrap_err().0.contains("16 megapixels"));
    }
}
