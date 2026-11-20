//! Pure decision logic: G-Helper mode + settings -> what to do with a service.
//! No Windows calls here, so it is fully unit-tested.

use serde::{Deserialize, Serialize};

/// G-Helper performance mode (`performance_mode` in its config.json).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Balanced,
    Turbo,
    Silent,
}

impl Mode {
    /// G-Helper encoding: 0 = Balanced, 1 = Turbo, 2 = Silent.
    pub fn from_ghelper(v: i64) -> Option<Mode> {
        match v {
            0 => Some(Mode::Balanced),
            1 => Some(Mode::Turbo),
            2 => Some(Mode::Silent),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Balanced => "Balanced",
            Mode::Turbo => "Turbo",
            Mode::Silent => "Silent",
        }
    }
}

/// What to do with the NVIDIA services in a given mode.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Stop the services (and keep them stopped).
    Off,
    /// Start the services (and keep them running).
    On,
    /// Do not touch them.
    Leave,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Policy {
    pub silent: Action,
    pub balanced: Action,
    pub turbo: Action,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            silent: Action::Off,
            balanced: Action::Off,
            turbo: Action::Leave,
        }
    }
}

impl Policy {
    pub fn action(&self, mode: Mode) -> Action {
        match mode {
            Mode::Silent => self.silent,
            Mode::Balanced => self.balanced,
            Mode::Turbo => self.turbo,
        }
    }

    /// Modes in which services are switched off — for the menu label.
    pub fn off_modes(&self) -> Vec<&'static str> {
        [Mode::Silent, Mode::Balanced, Mode::Turbo]
            .into_iter()
            .filter(|m| self.action(*m) == Action::Off)
            .map(Mode::name)
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvcState {
    Running,
    Stopped,
    /// Start/stop/pause in progress — never act on it, wait for the next tick.
    Pending,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Start,
    Stop,
}

/// Desired state of a service: `Some(true)` running, `Some(false)` stopped, `None` don't care.
/// A manual override from the tray wins over automation until the next mode change.
pub fn desired(manual: Option<bool>, auto: bool, mode: Option<Mode>, policy: &Policy) -> Option<bool> {
    if manual.is_some() {
        return manual;
    }
    if !auto {
        return None;
    }
    match mode.map(|m| policy.action(m)) {
        Some(Action::On) => Some(true),
        Some(Action::Off) => Some(false),
        Some(Action::Leave) | None => None,
    }
}

/// Command needed to reach the desired state, if any.
pub fn command(desired: Option<bool>, state: SvcState) -> Option<Command> {
    match (desired, state) {
        (Some(true), SvcState::Stopped) => Some(Command::Start),
        (Some(false), SvcState::Running) => Some(Command::Stop),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghelper_mode_mapping() {
        assert_eq!(Mode::from_ghelper(0), Some(Mode::Balanced));
        assert_eq!(Mode::from_ghelper(1), Some(Mode::Turbo));
        assert_eq!(Mode::from_ghelper(2), Some(Mode::Silent));
        assert_eq!(Mode::from_ghelper(3), None);
        assert_eq!(Mode::from_ghelper(-1), None);
    }

    #[test]
    fn default_policy_off_in_silent_and_balanced() {
        let p = Policy::default();
        assert_eq!(desired(None, true, Some(Mode::Silent), &p), Some(false));
        assert_eq!(desired(None, true, Some(Mode::Balanced), &p), Some(false));
        assert_eq!(desired(None, true, Some(Mode::Turbo), &p), None);
        assert_eq!(p.off_modes(), vec!["Silent", "Balanced"]);
    }

    #[test]
    fn manual_override_wins() {
        let p = Policy::default();
        assert_eq!(desired(Some(true), true, Some(Mode::Silent), &p), Some(true));
        assert_eq!(desired(Some(false), false, Some(Mode::Turbo), &p), Some(false));
    }

    #[test]
    fn automation_disabled_or_mode_unknown_means_leave() {
        let p = Policy::default();
        assert_eq!(desired(None, false, Some(Mode::Silent), &p), None);
        assert_eq!(desired(None, true, None, &p), None);
    }

    #[test]
    fn commands_only_on_stable_mismatch() {
        assert_eq!(command(Some(false), SvcState::Running), Some(Command::Stop));
        assert_eq!(command(Some(true), SvcState::Stopped), Some(Command::Start));
        assert_eq!(command(Some(false), SvcState::Stopped), None);
        assert_eq!(command(Some(true), SvcState::Running), None);
        assert_eq!(command(Some(false), SvcState::Pending), None);
        assert_eq!(command(Some(true), SvcState::Unknown), None);
        assert_eq!(command(None, SvcState::Running), None);
    }

    #[test]
    fn policy_parses_from_json() {
        let p: Policy = serde_json::from_str(r#"{"silent":"off","balanced":"leave","turbo":"on"}"#).unwrap();
        assert_eq!(p.action(Mode::Balanced), Action::Leave);
        assert_eq!(p.action(Mode::Turbo), Action::On);
        let partial: Policy = serde_json::from_str(r#"{"turbo":"off"}"#).unwrap();
        assert_eq!(partial.silent, Action::Off);
        assert_eq!(partial.turbo, Action::Off);
    }
}
