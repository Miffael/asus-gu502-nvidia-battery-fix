//! Logging, tray icon drawing, single-instance guard.

use crate::config::log_path;
use crate::services::wide;
use std::io::Write;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::CreateMutexW;

const LOG_MAX_BYTES: u64 = 512 * 1024;

pub fn log(msg: &str) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > LOG_MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let t = unsafe {
        let mut st = std::mem::zeroed();
        GetLocalTime(&mut st);
        st
    };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {msg}\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Returns false if another instance already holds the mutex.
/// The handle is intentionally leaked: it must live as long as the process.
pub fn single_instance(name: &str) -> bool {
    let w = wide(name);
    let h: HANDLE = unsafe { CreateMutexW(std::ptr::null(), 0, w.as_ptr()) };
    !h.is_null() && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
}

pub const GREEN: [u8; 3] = [46, 160, 67];
pub const ORANGE: [u8; 3] = [230, 140, 30];
pub const GREY: [u8; 3] = [128, 128, 128];

/// 32x32 RGBA: filled anti-aliased circle with a white "chip" square in the middle.
pub fn icon_rgba(color: [u8; 3]) -> Vec<u8> {
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let c = (N as f32 - 1.0) / 2.0;
    let r = 14.5f32;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let alpha = (r - d + 0.5).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let chip = dx.abs() <= 6.0 && dy.abs() <= 6.0 && !(dx.abs() <= 3.0 && dy.abs() <= 3.0);
            let rgb = if chip { [255, 255, 255] } else { color };
            let i = (y * N + x) * 4;
            px[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], (alpha * 255.0) as u8]);
        }
    }
    px
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_expected_size_and_transparent_corners() {
        let px = super::icon_rgba(super::GREEN);
        assert_eq!(px.len(), 32 * 32 * 4);
        assert_eq!(px[3], 0, "top-left corner must be transparent");
        let mid = (16 * 32 + 16) * 4;
        assert_eq!(px[mid + 3], 255, "center must be opaque");
    }
}
