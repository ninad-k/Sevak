//! Shell icon extraction, encoded as PNG for the web view.

use std::ffi::c_void;

use sevak_core::IconData;
use windows::core::{Interface, HRESULT, HSTRING};
use windows::Win32::Foundation::{HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_BIGGERSIZEOK,
    SIIGBF_ICONONLY, SIIGBF_THUMBNAILONLY,
};

use crate::error::{PlatformError, Result};

use super::com::ComGuard;

const MIN_SIZE: u32 = 16;
const MAX_SIZE: u32 = 256;
/// `E_PENDING`: the requested data is not available yet.
const E_PENDING: HRESULT = HRESULT(0x8000_000Au32 as i32);
const PENDING_RETRIES: u32 = 10;
const PENDING_DELAY: std::time::Duration = std::time::Duration::from_millis(30);

fn os_error(message: impl std::fmt::Display) -> PlatformError {
    PlatformError::Os {
        operation: "load_icon",
        message: message.to_string(),
    }
}

/// Renders the shell icon for `parsing_name` (a file path or a
/// `shell:AppsFolder\<aumid>` name) as a PNG of roughly `size` pixels.
pub(crate) fn load_shell_icon(parsing_name: &str, size: u32) -> Result<IconData> {
    let size = size.clamp(MIN_SIZE, MAX_SIZE);
    let bytes = shell_image_png(parsing_name, size, SIIGBF_BIGGERSIZEOK | SIIGBF_ICONONLY)?;
    Ok(IconData {
        mime: "image/png",
        bytes,
    })
}

/// Largest thumbnail asked of the shell.
pub(crate) const MAX_THUMBNAIL: u32 = 1024;

/// The shell's thumbnail of a file (an Office document, a video) as a PNG of
/// roughly `size` pixels, as Explorer would show it. An error when the file has
/// no thumbnail: unlike [`load_shell_icon`], the file type's generic icon is
/// never substituted.
pub(crate) fn load_shell_thumbnail(path: &str, size: u32) -> Result<Vec<u8>> {
    let size = size.clamp(MIN_SIZE, MAX_THUMBNAIL);
    shell_image_png(path, size, SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY)
}

fn shell_image_png(parsing_name: &str, size: u32, flags: SIIGBF) -> Result<Vec<u8>> {
    let _com = ComGuard::new();

    // SAFETY: the parsing name is a NUL-terminated HSTRING and no bind context
    // is passed.
    let item: IShellItem = unsafe {
        SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None::<&_>)
            .map_err(|e| os_error(format!("{parsing_name}: {e}")))?
    };
    let factory: IShellItemImageFactory = item.cast().map_err(os_error)?;

    // The shell may answer E_PENDING while it warms its icon cache; retry briefly.
    let mut attempt = 0;
    let bitmap = loop {
        // SAFETY: `factory` is a live COM object; the flags are valid.
        let result = unsafe {
            factory.GetImage(
                SIZE {
                    cx: size as i32,
                    cy: size as i32,
                },
                flags,
            )
        };
        match result {
            Ok(bitmap) => break bitmap,
            Err(err) if err.code() == E_PENDING && attempt < PENDING_RETRIES => {
                attempt += 1;
                std::thread::sleep(PENDING_DELAY);
            }
            Err(err) => return Err(os_error(format!("{parsing_name}: {err}"))),
        }
    };
    let bitmap = OwnedBitmap(bitmap);

    let (width, height, mut pixels) = bitmap.read_bgra()?;
    bgra_premultiplied_to_rgba(&mut pixels);
    encode_png(width, height, &pixels)
}

/// An `HBITMAP` that is deleted when dropped.
struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        // SAFETY: the handle came from `GetImage`, is owned exclusively by this
        // wrapper, and is deleted exactly once.
        let _ = unsafe { DeleteObject(HGDIOBJ(self.0 .0)) };
    }
}

/// A screen DC that is released when dropped.
struct ScreenDc(HDC);

impl ScreenDc {
    fn acquire() -> Result<Self> {
        // SAFETY: a null window handle requests the screen DC.
        let hdc = unsafe { GetDC(None) };
        if hdc.is_invalid() {
            Err(os_error("GetDC failed"))
        } else {
            Ok(Self(hdc))
        }
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the DC came from `GetDC(None)` and is released once.
        unsafe { ReleaseDC(Some(HWND::default()), self.0) };
    }
}

impl OwnedBitmap {
    /// Copies the pixels out as top-down 32-bpp BGRA.
    fn read_bgra(&self) -> Result<(u32, u32, Vec<u8>)> {
        let mut bitmap = BITMAP::default();
        // SAFETY: `bitmap` is a BITMAP-sized writable buffer matching the size
        // passed in.
        let copied = unsafe {
            GetObjectW(
                HGDIOBJ(self.0 .0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut BITMAP as *mut c_void),
            )
        };
        if copied == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
            return Err(os_error("could not query the icon bitmap"));
        }
        let (width, height) = (bitmap.bmWidth as u32, bitmap.bmHeight as u32);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // negative: top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];

        let dc = ScreenDc::acquire()?;
        // SAFETY: `pixels` holds exactly `width * height * 4` bytes, the amount
        // a 32-bpp copy of `height` scan lines writes; `info` describes that
        // layout; the bitmap is not selected into any other DC.
        let lines = unsafe {
            GetDIBits(
                dc.0,
                self.0,
                0,
                height,
                Some(pixels.as_mut_ptr() as *mut c_void),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            return Err(os_error("GetDIBits returned no scan lines"));
        }
        Ok((width, height, pixels))
    }
}

/// Converts premultiplied BGRA to straight RGBA in place. Bitmaps from
/// legacy icons carry no alpha channel at all (every alpha byte is zero while
/// the colors are not); those are treated as fully opaque.
pub(crate) fn bgra_premultiplied_to_rgba(pixels: &mut [u8]) {
    let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    let has_color = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[0] | p[1] | p[2] != 0);
    for px in pixels.as_chunks_mut::<4>().0 {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        let (r, g, b, a) = if has_alpha {
            match a {
                0 => (0, 0, 0, 0),
                255 => (r, g, b, 255),
                _ => (
                    unpremultiply(r, a),
                    unpremultiply(g, a),
                    unpremultiply(b, a),
                    a,
                ),
            }
        } else if has_color {
            (r, g, b, 255)
        } else {
            (0, 0, 0, 0)
        };
        px.copy_from_slice(&[r, g, b, a]);
    }
}

fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    let value = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
    value.min(255) as u8
}

pub(crate) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(os_error)?;
    writer.write_image_data(rgba).map_err(os_error)?;
    writer.finish().map_err(os_error)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn unpremultiplies_and_swaps_channels() {
        // BGRA: half-transparent premultiplied red (128, 0, 0 at alpha 128).
        let mut px = [0, 0, 128, 128, 10, 20, 30, 255, 5, 5, 5, 0];
        bgra_premultiplied_to_rgba(&mut px);
        assert_eq!(&px[0..4], &[255, 0, 0, 128]);
        assert_eq!(&px[4..8], &[30, 20, 10, 255]);
        assert_eq!(&px[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn alpha_less_bitmaps_become_opaque() {
        let mut px = [1, 2, 3, 0, 4, 5, 6, 0];
        bgra_premultiplied_to_rgba(&mut px);
        assert_eq!(px, [3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn png_encoding_round_trips() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 1, 2, 3, 4];
        let bytes = encode_png(2, 2, &rgba).unwrap();
        assert!(bytes.starts_with(PNG_SIGNATURE));
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..info.buffer_size()], &rgba);
    }

    #[test]
    fn notepad_icon_is_a_png() {
        let icon = load_shell_icon(r"C:\Windows\System32\notepad.exe", 32).unwrap();
        assert_eq!(icon.mime, "image/png");
        assert!(icon.bytes.starts_with(PNG_SIGNATURE));
        if let Some(dir) = std::env::var_os("SEVAK_ICON_DUMP_DIR") {
            let path = std::path::Path::new(&dir).join("notepad.png");
            std::fs::write(path, &icon.bytes).unwrap();
        }
    }

    #[test]
    fn missing_item_is_an_error_not_a_panic() {
        assert!(load_shell_icon(r"C:\definitely\not\here.exe", 32).is_err());
    }
}
