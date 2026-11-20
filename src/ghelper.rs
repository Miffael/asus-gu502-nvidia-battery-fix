//! Reads the current performance mode from G-Helper's config.json.
//! G-Helper rewrites the file on every mode change, so we re-parse only when the
//! modification time changes. A half-written file just keeps the previous mode.

use crate::policy::Mode;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct ModeWatcher {
    path: PathBuf,
    mtime: Option<SystemTime>,
    mode: Option<Mode>,
}

pub fn parse_mode(json: &str) -> Option<Mode> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Mode::from_ghelper(v.get("performance_mode")?.as_i64()?)
}

impl ModeWatcher {
    pub fn new(path: PathBuf) -> Self {
        ModeWatcher { path, mtime: None, mode: None }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Current mode, or None if G-Helper's config is missing/unreadable.
    pub fn poll(&mut self) -> Option<Mode> {
        let mtime = match std::fs::metadata(&self.path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => {
                self.mtime = None;
                self.mode = None;
                return None;
            }
        };
        if self.mtime != Some(mtime) {
            if let Ok(text) = std::fs::read_to_string(&self.path) {
                if let Some(m) = parse_mode(&text) {
                    self.mode = Some(m);
                    self.mtime = Some(mtime);
                }
                // parse failure (file mid-write): keep old mode and mtime, retry next tick
            }
        }
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_performance_mode() {
        assert_eq!(parse_mode(r#"{"performance_mode": 2, "gpu_mode": 1}"#), Some(Mode::Silent));
        assert_eq!(parse_mode(r#"{"performance_mode": 0}"#), Some(Mode::Balanced));
        assert_eq!(parse_mode(r#"{"gpu_mode": 1}"#), None);
        assert_eq!(parse_mode(r#"{"performance_mode": 2, "gpu"#), None);
    }

    #[test]
    fn watcher_handles_missing_and_changing_file() {
        let dir = std::env::temp_dir().join(format!("nvst-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.json");
        let _ = std::fs::remove_file(&f);
        let mut w = ModeWatcher::new(f.clone());
        assert_eq!(w.poll(), None);
        std::fs::write(&f, r#"{"performance_mode": 2}"#).unwrap();
        assert_eq!(w.poll(), Some(Mode::Silent));
        // make sure mtime differs on coarse file systems
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&f, r#"{"performance_mode": 1}"#).unwrap();
        let t = SystemTime::now() + std::time::Duration::from_secs(1);
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
        assert_eq!(w.poll(), Some(Mode::Turbo));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
