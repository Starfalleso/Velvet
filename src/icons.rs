//! Win32 icon extraction (`IShellItemImageFactory` -> GPUI `RenderImage`)
//! and antialiased Velvet SVG icon rasterization (`assets/velvet.svg`) for tray + EXE.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, ImageBuffer, Rgba};
use smallvec::SmallVec;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDIBits,
    GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
    SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// Extracts a square icon from an `IShellItem` into a GPUI `RenderImage` (BGRA format).
pub fn extract_shell_item_icon(item: &IShellItem, size: i32) -> Option<Arc<RenderImage>> {
    let factory: IShellItemImageFactory = item.cast().ok()?;

    let sz = SIZE { cx: size, cy: size };
    let hbitmap = unsafe {
        factory
            .GetImage(sz, SIIGBF_BIGGERSIZEOK | SIIGBF_ICONONLY)
            .or_else(|_| factory.GetImage(sz, SIIGBF_BIGGERSIZEOK))
            .ok()?
    };

    let image = hbitmap_to_render_image(hbitmap);
    unsafe {
        let _ = DeleteObject(hbitmap);
    }
    image
}

/// Extracts an icon from a filesystem path (`.lnk` or `.exe`).
pub fn extract_path_icon(path: &Path, size: i32) -> Option<Arc<RenderImage>> {
    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()? };
    extract_shell_item_icon(&item, size)
}

fn hbitmap_to_render_image(hbitmap: HBITMAP) -> Option<Arc<RenderImage>> {
    if hbitmap.is_invalid() || hbitmap.0.is_null() {
        return None;
    }

    let mut bmp = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            hbitmap,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut BITMAP as *mut _),
        )
    };
    if got == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
        return None;
    }

    let width = bmp.bmWidth as u32;
    let height = bmp.bmHeight as u32;

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bgra = vec![0u8; (width * height * 4) as usize];

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() || hdc.0.is_null() {
        return None;
    }

    let scanlines = unsafe {
        GetDIBits(
            hdc,
            hbitmap,
            0,
            height,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
    }

    if scanlines == 0 {
        return None;
    }

    // Legacy 32bpp icons sometimes leave alpha=0 everywhere even for non-black pixels.
    let all_alpha_zero = bgra.chunks_exact(4).all(|px| px[3] == 0);
    if all_alpha_zero {
        let any_rgb = bgra
            .chunks_exact(4)
            .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0);
        if !any_rgb {
            return None;
        }
        for px in bgra.chunks_exact_mut(4) {
            if px[0] != 0 || px[1] != 0 || px[2] != 0 {
                px[3] = 255;
            }
        }
    }

    // GPUI RenderImage expects BGRA pixels inside `ImageBuffer<Rgba<u8>, Vec<u8>>`.
    let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bgra)?;
    let frame = Frame::new(buffer);
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1))))
}

fn in_bg_rect(x: f32, y: f32) -> bool {
    let dx = (x - 128.0).abs();
    let dy = (y - 128.0).abs();
    if dx > 112.0 || dy > 112.0 {
        return false;
    }
    let inner = 112.0 - 56.0;
    if dx > inner && dy > inner {
        (dx - inner).powi(2) + (dy - inner).powi(2) <= 56.0 * 56.0
    } else {
        true
    }
}

fn in_left_petal(x: f32, y: f32) -> bool {
    if !(64.0..=101.0).contains(&x) || !(72.0..=140.0).contains(&y) {
        return false;
    }
    if x > 72.0 && y < 101.0 && (x - 72.0).powi(2) + (y - 101.0).powi(2) > 29.0 * 29.0 {
        return false;
    }
    if x < 86.0 && y > 118.0 && (x - 86.0).powi(2) + (y - 118.0).powi(2) > 22.0 * 22.0 {
        return false;
    }
    true
}

fn in_center_pill(x: f32, y: f32) -> bool {
    if !(109.0..=147.0).contains(&x) || !(57.0..=140.0).contains(&y) {
        return false;
    }
    if y < 76.0 && (x - 128.0).powi(2) + (y - 76.0).powi(2) > 19.0 * 19.0 {
        return false;
    }
    true
}

fn in_right_petal(x: f32, y: f32) -> bool {
    in_left_petal(256.0 - x, y)
}

fn in_chevron(x: f32, y: f32) -> bool {
    if y < 148.0 {
        return false;
    }
    let sum = (x - 128.0).abs() + y;
    (176.0..=200.0).contains(&sum)
}

fn sample_velvet_svg(x: f32, y: f32) -> Option<(u8, u8, u8)> {
    if in_chevron(x, y) {
        Some((0x1E, 0x22, 0x2B))
    } else if in_left_petal(x, y) || in_center_pill(x, y) || in_right_petal(x, y) {
        Some((0xFF, 0xFF, 0xFF))
    } else if in_bg_rect(x, y) {
        Some((0x58, 0x65, 0xF2))
    } else {
        None
    }
}

/// Renders `assets/velvet.svg` at `size x size` straight-alpha RGBA using stratified supersampling.
pub fn render_velvet_icon_rgba(size: u32) -> Vec<u8> {
    let samples: u32 = if size <= 64 { 8 } else { 4 };
    let total_samples = (samples * samples) as f32;
    let scale = 256.0 / size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];

    for py in 0..size {
        for px in 0..size {
            let mut r_sum = 0.0f32;
            let mut g_sum = 0.0f32;
            let mut b_sum = 0.0f32;
            let mut hits = 0.0f32;

            for sy in 0..samples {
                let v = ((py as f32) + (sy as f32 + 0.5) / samples as f32) * scale;
                for sx in 0..samples {
                    let u = ((px as f32) + (sx as f32 + 0.5) / samples as f32) * scale;
                    if let Some((r, g, b)) = sample_velvet_svg(u, v) {
                        r_sum += r as f32;
                        g_sum += g as f32;
                        b_sum += b as f32;
                        hits += 1.0;
                    }
                }
            }

            if hits > 0.0 {
                let idx = ((py * size + px) * 4) as usize;
                out[idx] = (r_sum / hits).round().clamp(0.0, 255.0) as u8;
                out[idx + 1] = (g_sum / hits).round().clamp(0.0, 255.0) as u8;
                out[idx + 2] = (b_sum / hits).round().clamp(0.0, 255.0) as u8;
                out[idx + 3] = ((hits / total_samples) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    out
}

/// Renders Velvet's SVG logo as a crisp 64x64 `HICON` for the Windows system tray.
pub fn create_tray_hicon() -> Option<HICON> {
    const SIZE: u32 = 64;
    let rgba = render_velvet_icon_rgba(SIZE);
    let mut pixels = vec![0u8; rgba.len()];

    // Convert straight-alpha RGBA to BGRA for Win32 GDI DIB sections.
    for (src, dst) in rgba.chunks_exact(4).zip(pixels.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE as i32,
            biHeight: -(SIZE as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    unsafe {
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm_color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(hbm_color);
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());

        let hbm_mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, None);
        if hbm_mask.is_invalid() || hbm_mask.0.is_null() {
            let _ = DeleteObject(hbm_color);
            return None;
        }

        let icon_info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };

        let hicon = CreateIconIndirect(&icon_info).ok();
        let _ = DeleteObject(hbm_color);
        let _ = DeleteObject(hbm_mask);
        hicon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ColorType, ImageEncoder};
    use std::path::PathBuf;

    fn encode_velvet_ico() -> Vec<u8> {
        let sizes = [16u32, 24, 32, 48, 64, 256];
        let mut frames: Vec<(u32, Vec<u8>)> = Vec::with_capacity(sizes.len());

        for size in sizes {
            let rgba = render_velvet_icon_rgba(size);
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(&rgba, size, size, ColorType::Rgba8.into())
                .expect("PNG encoding for ICO frame");
            frames.push((size, png));
        }

        let count = frames.len() as u16;
        let mut ico = Vec::new();
        ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // ICON type
        ico.extend_from_slice(&count.to_le_bytes());

        let mut offset = 6u32 + (frames.len() as u32) * 16;
        for (size, png) in &frames {
            let dim = if *size >= 256 { 0u8 } else { *size as u8 };
            ico.push(dim); // width
            ico.push(dim); // height
            ico.push(0); // palette count
            ico.push(0); // reserved
            ico.extend_from_slice(&1u16.to_le_bytes()); // planes
            ico.extend_from_slice(&32u16.to_le_bytes()); // bpp
            ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
            ico.extend_from_slice(&offset.to_le_bytes());
            offset += png.len() as u32;
        }

        for (_, png) in frames {
            ico.extend_from_slice(&png);
        }

        ico
    }

    #[test]
    fn velvet_svg_renderer_hits_expected_colors() {
        let rgba = render_velvet_icon_rgba(64);
        assert_eq!(rgba.len(), 64 * 64 * 4);

        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 64 + x) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };

        // Top-left outer margin should be transparent
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        // Background blurple (#5865F2) around (48px, 48px) -> (12, 12) in 64x64
        assert_eq!(pixel(12, 12), [0x58, 0x65, 0xF2, 0xFF]);
        // Center white pill (#FFFFFF) at (128, 100) -> (32, 25)
        assert_eq!(pixel(32, 25), [0xFF, 0xFF, 0xFF, 0xFF]);
        // Dark chevron (#1E222B) at (128, 188) -> (32, 47)
        assert_eq!(pixel(32, 47), [0x1E, 0x22, 0x2B, 0xFF]);

        let ico = encode_velvet_ico();
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 6, 0]);
    }

    #[test]
    #[ignore]
    fn export_velvet_ico() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("velvet.ico");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, encode_velvet_ico()).unwrap();
    }

    #[test]
    #[ignore]
    fn export_velvet_png() {
        let size = 1024u32;
        let rgba = render_velvet_icon_rgba(size);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, size, size, ColorType::Rgba8.into())
            .expect("PNG encoding for velvet.png");
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("velvet.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, png).unwrap();
    }
}
