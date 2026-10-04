//! Clipboard images and file lists: the pixels as Sevak holds them, PNG files,
//! thumbnails and the content hashes the clipboard history dedupes by.
//!
//! Reading and writing the clipboard itself is [`crate::clipboard`]'s job
//! (through `arboard`, which converts between the OS formats: `CF_DIB` /
//! `CF_DIBV5` / `PNG` on Windows, `public.png` / `public.tiff` on macOS and
//! `image/png` on Linux). Everything here is plain computation.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::error::{PlatformError, Result};

/// The most RGBA bytes of one image Sevak is willing to hold (128 MiB, about
/// 32 megapixels). Anything bigger is ignored rather than risking memory.
pub const MAX_IMAGE_RAW_BYTES: usize = 128 * 1024 * 1024;

/// An image as 8-bit RGBA pixels, row by row, top row first.
#[derive(Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

// The pixels are private data and huge; never print them.
impl std::fmt::Debug for ClipboardImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClipboardImage({}x{})", self.width, self.height)
    }
}

/// The pixel count (width x height) of a DIB, from the first bytes of the
/// `CF_DIB` / `CF_DIBV5` data (a `BITMAPINFOHEADER`, `BITMAPV5HEADER` or the old
/// `BITMAPCOREHEADER`). `None` if the header is too short or malformed.
pub(crate) fn dib_header_pixels(header: &[u8]) -> Option<u64> {
    let word = |at: usize| -> Option<[u8; 4]> { header.get(at..at + 4)?.try_into().ok() };
    let size = u32::from_le_bytes(word(0)?);
    let (width, height) = if size == 12 {
        // BITMAPCOREHEADER: 16-bit width and height.
        let pair = |at: usize| {
            header
                .get(at..at + 2)
                .map(|b| u64::from(u16::from_le_bytes([b[0], b[1]])))
        };
        (pair(4)?, pair(6)?)
    } else if size >= 40 {
        // The height is negative for a top-down bitmap.
        let width = i32::from_le_bytes(word(4)?);
        let height = i32::from_le_bytes(word(8)?);
        (
            u64::from(width.unsigned_abs()),
            u64::from(height.unsigned_abs()),
        )
    } else {
        return None;
    };
    Some(width * height)
}

/// The pixel count of a PNG, from the first 24 bytes (signature and the start
/// of the `IHDR` chunk). `None` if it does not start like a PNG.
pub(crate) fn png_header_pixels(header: &[u8]) -> Option<u64> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if header.get(..8)? != SIGNATURE || header.get(12..16)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| -> Option<u64> {
        Some(u64::from(u32::from_be_bytes(
            header.get(at..at + 4)?.try_into().ok()?,
        )))
    };
    Some(word(16)? * word(20)?)
}

/// Whether the image on the system clipboard is bigger than
/// [`MAX_IMAGE_RAW_BYTES`] once decoded, judged from its header **before** the
/// clipboard library converts it (which allocates for every pixel). Only
/// Windows can be asked without converting; elsewhere this is `false` and the
/// size is checked on the converted image.
pub fn clipboard_image_is_oversized() -> bool {
    #[cfg(windows)]
    {
        crate::windows::clipboard_image_pixels()
            .is_some_and(|pixels| pixels.saturating_mul(4) > MAX_IMAGE_RAW_BYTES as u64)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn os_error(operation: &'static str, err: impl std::fmt::Display) -> PlatformError {
    PlatformError::Os {
        operation,
        message: err.to_string(),
    }
}

impl ClipboardImage {
    /// `None` unless `rgba` is exactly `width` x `height` pixels, there is at
    /// least one, and the image is within [`MAX_IMAGE_RAW_BYTES`].
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = u64::from(width) * u64::from(height) * 4;
        (expected > 0 && expected <= MAX_IMAGE_RAW_BYTES as u64 && rgba.len() as u64 == expected)
            .then_some(Self {
                width,
                height,
                rgba,
            })
    }

    /// A hash of the size and the pixels. Stable across runs and versions (it
    /// is stored in the history and names the image's files), so the algorithm
    /// must never change. Not cryptographic: it only recognises a copy of the
    /// same picture.
    pub fn content_hash(&self) -> u64 {
        const PRIME: u64 = 0x9E37_79B9_7F4A_7C15;
        let mix = |hash: u64, word: u64| {
            let hash = (hash ^ word).wrapping_mul(PRIME);
            hash ^ (hash >> 32)
        };
        let mut hash = mix(
            mix(0xcbf2_9ce4_8422_2325, u64::from(self.width)),
            u64::from(self.height),
        );
        let (chunks, rest) = self.rgba.as_chunks::<8>();
        for chunk in chunks {
            hash = mix(hash, u64::from_le_bytes(*chunk));
        }
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        mix(hash, u64::from_le_bytes(tail))
    }

    /// True if no pixel is transparent (a screenshot): such an image is stored
    /// without an alpha channel, which is smaller and faster to compress.
    fn is_opaque(&self) -> bool {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    }

    /// Encodes the image as a PNG (fast compression: this runs while the user
    /// is copying).
    pub fn encode_png(&self) -> Result<Vec<u8>> {
        let opaque = self.is_opaque();
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.set_color(if opaque {
            png::ColorType::Rgb
        } else {
            png::ColorType::Rgba
        });
        let mut writer = encoder
            .write_header()
            .map_err(|err| os_error("encode_png", err))?;
        let written = if opaque {
            let rgb: Vec<u8> = self
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect();
            writer.write_image_data(&rgb)
        } else {
            writer.write_image_data(&self.rgba)
        };
        written.map_err(|err| os_error("encode_png", err))?;
        writer.finish().map_err(|err| os_error("encode_png", err))?;
        Ok(bytes)
    }

    /// Decodes a PNG of any colour type. Refuses an image larger than
    /// [`MAX_IMAGE_RAW_BYTES`] before allocating for it.
    pub fn decode_png(bytes: &[u8]) -> Result<Self> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_limits(png::Limits {
            bytes: MAX_IMAGE_RAW_BYTES + 1024 * 1024,
        });
        // Palettes and low bit depths become 8-bit channels; 16-bit is cut to 8.
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder
            .read_info()
            .map_err(|err| os_error("decode_png", err))?;
        let (width, height) = reader.info().size();
        if u64::from(width) * u64::from(height) * 4 > MAX_IMAGE_RAW_BYTES as u64 {
            return Err(os_error("decode_png", "the image is too large"));
        }
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| os_error("decode_png", "the image is too large"))?;
        let mut buffer = vec![0; size];
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|err| os_error("decode_png", err))?;
        let pixels = &buffer[..info.buffer_size()];

        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => pixels.to_vec(),
            png::ColorType::Rgb => pixels
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => pixels
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Indexed => {
                return Err(os_error("decode_png", "unexpected palette image"))
            }
        };
        Self::new(width, height, rgba).ok_or_else(|| os_error("decode_png", "invalid image size"))
    }

    /// The image shrunk to fit a `max_edge` x `max_edge` box (area averaging,
    /// colours weighted by alpha so transparent pixels do not darken the
    /// edges). An image that already fits is returned as it is.
    #[must_use]
    pub fn thumbnail(&self, max_edge: u32) -> Self {
        let longest = self.width.max(self.height);
        if max_edge == 0 || longest <= max_edge {
            return self.clone();
        }
        let scale = |side: u32| {
            ((u64::from(side) * u64::from(max_edge)) / u64::from(longest)).max(1) as u32
        };
        let (out_w, out_h) = (scale(self.width), scale(self.height));
        let mut rgba = Vec::with_capacity(out_w as usize * out_h as usize * 4);

        for out_y in 0..out_h {
            let y0 = (u64::from(out_y) * u64::from(self.height) / u64::from(out_h)) as u32;
            let y1 = ((u64::from(out_y + 1) * u64::from(self.height) / u64::from(out_h)) as u32)
                .max(y0 + 1);
            for out_x in 0..out_w {
                let x0 = (u64::from(out_x) * u64::from(self.width) / u64::from(out_w)) as u32;
                let x1 = ((u64::from(out_x + 1) * u64::from(self.width) / u64::from(out_w)) as u32)
                    .max(x0 + 1);

                let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
                for y in y0..y1 {
                    let row = y as usize * self.width as usize;
                    for x in x0..x1 {
                        let i = (row + x as usize) * 4;
                        let alpha = u64::from(self.rgba[i + 3]);
                        r += u64::from(self.rgba[i]) * alpha;
                        g += u64::from(self.rgba[i + 1]) * alpha;
                        b += u64::from(self.rgba[i + 2]) * alpha;
                        a += alpha;
                        n += 1;
                    }
                }
                // Fully transparent: no colour to average.
                let pixel = match r.checked_div(a) {
                    Some(red) => [red as u8, (g / a) as u8, (b / a) as u8, (a / n) as u8],
                    None => [0, 0, 0, 0],
                };
                rgba.extend_from_slice(&pixel);
            }
        }
        Self {
            width: out_w,
            height: out_h,
            rgba,
        }
    }
}

/// A hash of a list of paths (stable, like [`ClipboardImage::content_hash`]),
/// to recognise a copy of the same files.
pub fn files_hash(paths: &[PathBuf]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for path in paths {
        for byte in path.to_string_lossy().bytes().chain([0]) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// Where "Save image as" puts files: the Desktop, else Downloads, else the
/// home folder (only folders that exist).
pub fn save_directory() -> Option<PathBuf> {
    [dirs::desktop_dir(), dirs::download_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
}

/// The first of `stem.ext`, `stem (2).ext`, `stem (3).ext`, ... that does not
/// exist in `dir`, so saving never overwrites a file.
pub fn unique_file_name(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{extension}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{extension}")))
        .find(|path| !path.exists())
        .expect("an unbounded range has a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, fill: impl Fn(u32, u32) -> [u8; 4]) -> ClipboardImage {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&fill(x, y));
            }
        }
        ClipboardImage::new(width, height, rgba).unwrap()
    }

    #[test]
    fn sizes_are_validated() {
        assert!(ClipboardImage::new(2, 1, vec![0; 8]).is_some());
        assert!(ClipboardImage::new(2, 1, vec![0; 7]).is_none());
        assert!(ClipboardImage::new(0, 1, vec![]).is_none());
        assert!(ClipboardImage::new(1, 0, vec![]).is_none());
        // Far bigger than the cap; rejected from the numbers alone.
        assert!(ClipboardImage::new(100_000, 100_000, vec![0; 8]).is_none());
    }

    #[test]
    fn the_hash_sees_pixels_and_shape_and_never_changes() {
        let a = image(3, 2, |x, y| [x as u8, y as u8, 7, 255]);
        let same = image(3, 2, |x, y| [x as u8, y as u8, 7, 255]);
        let mut other = same.clone();
        other.rgba[23] = 254; // the last pixel's alpha
        let reshaped = ClipboardImage::new(2, 3, a.rgba.clone()).unwrap();
        assert_eq!(a.content_hash(), same.content_hash());
        assert_ne!(a.content_hash(), other.content_hash());
        assert_ne!(a.content_hash(), reshaped.content_hash());
        // The hash names files on disk and is stored in the history file: this
        // value must stay put, or every saved image would look new.
        assert_eq!(
            ClipboardImage::new(1, 1, vec![1, 2, 3, 4])
                .unwrap()
                .content_hash(),
            0xf31f_d36d_81e6_92a5
        );
    }

    #[test]
    fn opaque_images_are_stored_as_rgb_and_round_trip() {
        let opaque = image(5, 4, |x, y| [x as u8 * 40, y as u8 * 50, 9, 255]);
        let bytes = opaque.encode_png().unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        // Colour type 2 (RGB) sits at byte 25 of a PNG.
        assert_eq!(bytes[25], 2);
        assert_eq!(ClipboardImage::decode_png(&bytes).unwrap(), opaque);
    }

    #[test]
    fn transparent_images_keep_their_alpha() {
        let translucent = image(4, 4, |x, _| [10, 20, 30, if x == 0 { 0 } else { 128 }]);
        let bytes = translucent.encode_png().unwrap();
        assert_eq!(bytes[25], 6); // RGBA
        assert_eq!(ClipboardImage::decode_png(&bytes).unwrap(), translucent);
    }

    #[test]
    fn other_png_colour_types_decode() {
        let mut gray = Vec::new();
        let mut encoder = png::Encoder::new(&mut gray, 2, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[10, 200]).unwrap();
        writer.finish().unwrap();
        let decoded = ClipboardImage::decode_png(&gray).unwrap();
        assert_eq!(decoded.rgba, [10, 10, 10, 255, 200, 200, 200, 255]);
    }

    #[test]
    fn dib_headers_give_their_pixel_counts_without_the_pixels() {
        let info = |width: i32, height: i32| {
            let mut header = 40u32.to_le_bytes().to_vec();
            header.extend(width.to_le_bytes());
            header.extend(height.to_le_bytes());
            header
        };
        assert_eq!(dib_header_pixels(&info(1920, 1080)), Some(1920 * 1080));
        // A negative height means top-down.
        assert_eq!(dib_header_pixels(&info(10, -20)), Some(200));
        assert_eq!(
            dib_header_pixels(&info(i32::MAX, i32::MAX)),
            Some(u64::from(i32::MAX as u32) * u64::from(i32::MAX as u32))
        );
        let mut core = 12u32.to_le_bytes().to_vec();
        core.extend(7u16.to_le_bytes());
        core.extend(9u16.to_le_bytes());
        assert_eq!(dib_header_pixels(&core), Some(63));
        // Too short, or not a header at all.
        assert_eq!(dib_header_pixels(&[]), None);
        assert_eq!(dib_header_pixels(&info(1, 1)[..11]), None);
        assert_eq!(dib_header_pixels(&7u32.to_le_bytes()), None);
    }

    #[test]
    fn png_headers_give_their_pixel_counts_without_decoding() {
        let image = image(5, 3, |_, _| [1, 2, 3, 255]);
        let png = image.encode_png().unwrap();
        assert_eq!(png_header_pixels(&png[..24]), Some(15));
        assert_eq!(png_header_pixels(&png[..23]), None);
        assert_eq!(png_header_pixels(b"not a png at all, no"), None);
        // A header that claims an enormous image is read as one.
        let mut huge = png[..24].to_vec();
        huge[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        huge[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(png_header_pixels(&huge).unwrap().saturating_mul(4) > MAX_IMAGE_RAW_BYTES as u64);
    }

    #[test]
    fn garbage_is_not_an_image() {
        assert!(ClipboardImage::decode_png(b"not a png").is_err());
        assert!(ClipboardImage::decode_png(&[]).is_err());
    }

    #[test]
    fn thumbnails_fit_the_box_and_average_colours() {
        let big = image(200, 100, |x, _| {
            if x < 100 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let small = big.thumbnail(20);
        assert_eq!((small.width, small.height), (20, 10));
        assert_eq!(small.rgba.len(), 20 * 10 * 4);
        assert_eq!(&small.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&small.rgba[19 * 4..20 * 4], &[0, 0, 255, 255]);
        // A tall image shrinks by its height, and never to nothing.
        let tall = image(3, 300, |_, _| [1, 2, 3, 255]).thumbnail(30);
        assert_eq!((tall.width, tall.height), (1, 30));
        // An image that fits is left alone.
        assert_eq!(big.thumbnail(500), big);
        assert_eq!(big.thumbnail(200), big);
    }

    #[test]
    fn transparent_pixels_do_not_darken_a_thumbnail() {
        let img = image(4, 2, |x, _| {
            if x % 2 == 0 {
                [200, 100, 50, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let small = img.thumbnail(2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(&small.rgba[0..4], &[200, 100, 50, 127]);
    }

    #[test]
    fn file_lists_hash_by_their_paths_and_order() {
        let a = vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")];
        let b = vec![PathBuf::from("/a/y"), PathBuf::from("/a/x")];
        assert_eq!(files_hash(&a), files_hash(&a.clone()));
        assert_ne!(files_hash(&a), files_hash(&b));
        // "ab" + "c" is not "a" + "bc".
        assert_ne!(
            files_hash(&[PathBuf::from("ab"), PathBuf::from("c")]),
            files_hash(&[PathBuf::from("a"), PathBuf::from("bc")])
        );
    }

    #[test]
    fn saved_files_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_file_name(dir.path(), "Clipboard", "png");
        assert_eq!(first, dir.path().join("Clipboard.png"));
        std::fs::write(&first, b"x").unwrap();
        let second = unique_file_name(dir.path(), "Clipboard", "png");
        assert_eq!(second, dir.path().join("Clipboard (2).png"));
        std::fs::write(&second, b"x").unwrap();
        assert_eq!(
            unique_file_name(dir.path(), "Clipboard", "png"),
            dir.path().join("Clipboard (3).png")
        );
    }
}
