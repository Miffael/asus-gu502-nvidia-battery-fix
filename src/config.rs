//! App settings in %APPDATA%\nv-service-tray\config.json (created with defaults on first run).

use crate::policy::Policy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Path to G-Helper's config.json; empty = %APPDATA%\GHelper\config.json.
    pub ghelper_config: String,
    /// How often to check the G-Helper mode and the service states.
    pub poll_ms: u32,
    /// Automation on/off (also toggled from the tray menu).
    pub auto: bool,
    pub policy: Policy,
}

impl Default for Config {
    fn default() -> Self {
        Config { ghelper_config: String::new(), poll_ms: 2000, auto: true, policy: Policy::default() }
    }
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    env_dir("APPDATA").join("nv-service-tray")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    env_dir("LOCALAPPDATA").join("nv-service-tray").join("nv-service-tray.log")
}

impl Config {
    pub fn ghelper_path(&self) -> PathBuf {
        if self.ghelper_config.trim().is_empty() {
            env_dir("APPDATA").join("GHelper").join("config.json")
        } else {
            PathBuf::from(&self.ghelper_config)
        }
    }

    /// Loads the config; writes defaults if the file is missing. A broken file is kept
    /// untouched and defaults are used (error is returned for logging).
    pub fn load() -> (Config, Option<String>) {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(mut c) => {
                    c.poll_ms = c.poll_ms.clamp(500, 60_000);
                    (c, None)
                }
                Err(e) => (Config::default(), Some(format!("config.json не прочитан ({e}), использую значения по умолчанию"))),
            },
            Err(_) => {
                let c = Config::default();
                let err = c.save().err();
                (c, err)
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        use std::io::Write;
        let path = config_path();
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::util::open_private(&path, false)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
