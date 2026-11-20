//! Logging, tray icon drawing, single-instance guard.

use crate::config::log_path;
use crate::services::wide;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::CreateMutexW;

const LOG_MAX_BYTES: u64 = 512 * 1024;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

fn is_reparse_point(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0).unwrap_or(false)
}

/// The app runs elevated but keeps its files in the user's profile, which any non-admin
/// process of that user can modify. Refuse paths that go through a symlink/junction, so the
/// elevated writes can't be redirected into protected locations.
fn path_is_private(file: &Path) -> bool {
    file.ancestors().skip(1).filter(|a| a.parent().is_some()).all(|a| !is_reparse_point(a))
}

/// Opens `file` for writing (append or truncate) only if neither it nor any parent directory
/// is a reparse point and it is not a hard link to some other file.
pub fn open_private(file: &Path, append: bool) -> std::io::Result<File> {
    let denied = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "путь через ссылку (symlink/junction/hardlink) — запись запрещена");
    if !path_is_private(file) {
        return Err(denied());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
        if !path_is_private(file) {
            return Err(denied());
        }
    }
    let mut o = OpenOptions::new();
    o.create(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if append {
        o.append(true);
    } else {
        o.write(true); // no truncate yet: check the handle first
    }
    let f = o.open(file)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle() as HANDLE, &mut info) } != 0;
    if !ok || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || info.nNumberOfLinks > 1 {
        return Err(denied());
    }
    if !append {
        f.set_len(0)?;
    }
    Ok(f)
}

pub fn log(msg: &str) {
    let path = log_path();
    if !path_is_private(&path) {
        return;
    }
    if std::fs::symlink_metadata(&path).map(|m| m.len() > LOG_MAX_BYTES).unwrap_or(false) {
        // rename moves directory entries, it doesn't follow links; the folder itself was checked above
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
    if let Ok(mut f) = open_private(&path, true) {
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nv-service-tray-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn open_private_writes_normal_files() {
        use std::io::Write;
        let d = scratch("plain");
        let f = d.join("sub").join("a.log");
        super::open_private(&f, true).unwrap().write_all(b"x").unwrap();
        super::open_private(&f, false).unwrap().write_all(b"yz").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"yz");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_private_refuses_hard_links() {
        let d = scratch("hardlink");
        let target = d.join("target.txt");
        std::fs::write(&target, b"keep").unwrap();
        let link = d.join("a.log");
        std::fs::hard_link(&target, &link).unwrap();
        assert!(super::open_private(&link, false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep", "target must stay untouched");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_private_refuses_junctions() {
        let d = scratch("junction");
        let real = d.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let junc = d.join("junc");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junc)
            .arg(&real)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return; // can't create a junction here — nothing to test
        }
        assert!(super::open_private(&junc.join("a.log"), true).is_err());
        assert!(!real.join("a.log").exists(), "write must not go through the junction");
        let _ = std::fs::remove_dir_all(&d);
    }
}
