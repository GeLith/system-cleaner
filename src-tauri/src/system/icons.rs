//! 图标提取 —— 对齐 Electron app.getFileIcon(path,{size:'small'}) -> dataURL|null
//! 契约 (ipc.js#13-24):
//!   - 返回字符串 dataURL 或 null (前端 pages.js#1105 直接把返回值当 src 用)
//!   - 按路径小写缓存, 失败结果也缓存 (iconCache.set(key,result))
//! 实现零依赖: shell32 SHGetFileInfoW 取 HICON(解析 .lnk/文件关联),
//! gdi32 GetDIBits 抓 32bpp BGRA, 手工封装成 ICO(BMP-in-ICO, 含 alpha)后 base64。

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

type HANDLE = *mut c_void;

const SHGFI_ICON: u32 = 0x0000_0100;
const SHGFI_LARGEICON: u32 = 0x0000_0000;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
const DIB_RGB_COLORS: u32 = 0;

#[repr(C)]
struct SHFILEINFOW {
    h_icon: HANDLE,
    i_icon: i32,
    dw_attributes: u32,
    sz_display_name: [u16; 260],
    sz_type_name: [u16; 80],
}

#[repr(C)]
struct ICONINFO {
    f_icon: i32,
    x_hotspot: u32,
    y_hotspot: u32,
    hbm_mask: HANDLE,
    hbm_color: HANDLE,
}

#[repr(C)]
struct BITMAP {
    bm_type: i32,
    bm_width: i32,
    bm_height: i32,
    bm_width_bytes: i32,
    bm_planes: u16,
    bm_bits_pixel: u16,
    bm_bits: *mut c_void,
}

#[derive(Default)]
#[repr(C)]
struct BITMAPINFOHEADER {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_ppm: i32,
    bi_y_ppm: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
struct BITMAPINFO {
    bmi_header: BITMAPINFOHEADER,
    bmi_colors: [u32; 1],
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetFileInfoW(
        psz_path: *const u16,
        dw_file_attributes: u32,
        psfi: *mut SHFILEINFOW,
        cb_file_info: u32,
        u_flags: u32,
    ) -> usize;
}

#[link(name = "user32")]
extern "system" {
    fn GetIconInfo(hicon: HANDLE, piconinfo: *mut ICONINFO) -> i32;
    fn DestroyIcon(hicon: HANDLE) -> i32;
    fn GetDC(hwnd: HANDLE) -> HANDLE;
    fn ReleaseDC(hwnd: HANDLE, hdc: HANDLE) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(hdc: HANDLE) -> HANDLE;
    fn DeleteDC(hdc: HANDLE) -> i32;
    fn DeleteObject(ho: HANDLE) -> i32;
    fn GetObjectW(h: HANDLE, c: i32, pv: *mut c_void) -> i32;
    fn GetDIBits(
        hdc: HANDLE,
        hbmp: HANDLE,
        start: u32,
        lines: u32,
        lpvbits: *mut c_void,
        lpbmi: *mut BITMAPINFO,
        usage: u32,
    ) -> i32;
}

/// 缓存: 小写路径 -> dataURL; 空串表示已知的失败(负缓存), 对齐 ipc.js iconCache
static ICON_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn file_icon_data_url(file_path: &str) -> String {
    if file_path.is_empty() {
        return String::new();
    }
    let key = file_path.to_lowercase();
    if let Some(hit) = ICON_CACHE.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let url = compute_data_url(file_path).unwrap_or_default();
    ICON_CACHE.lock().unwrap().insert(key, url.clone());
    url
}

fn compute_data_url(file_path: &str) -> Option<String> {
    unsafe { extract_hicon(file_path) }
        .and_then(|hicon| unsafe { hicon_to_ico(hicon) })
        .map(|bytes| format!("data:image/x-icon;base64,{}", base64_encode(&bytes)))
}

unsafe fn extract_hicon(path: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut sfi: SHFILEINFOW = std::mem::zeroed();
    let ok = SHGetFileInfoW(
        wide.as_ptr(),
        FILE_ATTRIBUTE_NORMAL,
        &mut sfi,
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if ok == 0 || sfi.h_icon.is_null() {
        None
    } else {
        Some(sfi.h_icon)
    }
}

/// HICON -> ICO 字节流 (ICONDIR + ICONDIRENTRY + BITMAPINFOHEADER + XOR(BGRA) + AND mask)
unsafe fn hicon_to_ico(hicon: HANDLE) -> Option<Vec<u8>> {
    let mut ii: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon, &mut ii) == 0 {
        DestroyIcon(hicon);
        return None;
    }
    let mut bm: BITMAP = std::mem::zeroed();
    let got = GetObjectW(
        ii.hbm_color,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bm as *mut BITMAP as *mut c_void,
    );
    let result = (|| {
        if got == 0 {
            return None;
        }
        let w = bm.bm_width;
        let h = bm.bm_height;
        if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
            return None;
        }
        // 底朝上(bottom-up)行序提取 —— 与 ICO 内嵌 BMP 的约定一致
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        let mut bmi = BITMAPINFO {
            bmi_header: BITMAPINFOHEADER {
                bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                bi_width: w,
                bi_height: h,
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0, // BI_RGB
                bi_size_image: pixels.len() as u32,
                ..Default::default()
            },
            bmi_colors: [0; 1],
        };
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let lines = GetDIBits(
            mem,
            ii.hbm_color,
            0,
            h as u32,
            pixels.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        );
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        if lines == 0 {
            return None;
        }
        // AND mask: 每行按 32bit 对齐; 有 alpha 通道时全 0 即可
        let mask_row = (((w as usize) + 31) / 32) * 4;
        let mask_len = mask_row * (h as usize);

        let mut out = Vec::with_capacity(6 + 16 + 40 + pixels.len() + mask_len);
        // ICONDIR
        out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        // ICONDIRENTRY (宽高 256 用 0 表示)
        let dim_byte = |v: i32| if v >= 256 { 0u8 } else { v as u8 };
        out.push(dim_byte(w));
        out.push(dim_byte(h));
        out.push(0); // 调色板色数
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bitcount
        let body_len = (40 + pixels.len() + mask_len) as u32;
        out.extend_from_slice(&body_len.to_le_bytes());
        out.extend_from_slice(&22u32.to_le_bytes()); // 数据偏移 6+16
        // BITMAPINFOHEADER (ICO 内 biHeight 必须为 2×h: XOR 像素段 + AND 掩码段合计)
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(w as i32).to_le_bytes());
        out.extend_from_slice(&((h as i32) * 2).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        out.extend_from_slice(&((pixels.len() + mask_len) as u32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        // XOR (BGRA, bottom-up)
        out.extend_from_slice(&pixels);
        // AND mask
        out.extend(std::iter::repeat(0u8).take(mask_len));
        Some(out)
    })();

    DeleteObject(ii.hbm_color);
    DeleteObject(ii.hbm_mask);
    DestroyIcon(hicon);
    result
}

/// 标准 base64 (RFC 4648, 带 padding)
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
