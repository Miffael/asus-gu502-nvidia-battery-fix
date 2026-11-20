//! nv-service-tray — keeps NVIDIA background services off in quiet G-Helper modes.
//!
//! * watches G-Helper's performance mode (config.json) and stops/starts
//!   `NVDisplay.ContainerLocalSystem` and `NvContainerLocalSystem` per policy;
//! * tray menu with a toggle per service (manual override until the next mode change)
//!   and an automation switch.
//!
//! Needs admin rights to control the services: install.ps1 registers a logon task
//! that starts it elevated without a UAC prompt.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod ghelper;
mod policy;
mod services;
mod util;

use config::{Config, config_path, log_path};
use ghelper::ModeWatcher;
use policy::{Command, Mode, SvcState, command, desired};
use services::{Scm, describe, wide};
use std::ptr::{null, null_mut};
use std::time::SystemTime;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use util::{GREEN, GREY, ORANGE, icon_rgba, log, single_instance};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, SW_SHOWNORMAL, SetTimer, TranslateMessage, WM_TIMER,
};

/// (service name, short name for tooltip, menu label)
const SERVICES: [(&str, &str, &str); 2] = [
    ("NVDisplay.ContainerLocalSystem", "Display", "NVIDIA Display Container (панель NVIDIA)"),
    ("NvContainerLocalSystem", "App", "NVIDIA App Container (оверлей, запись, телеметрия)"),
];

struct Svc {
    name: &'static str,
    short: &'static str,
    label: &'static str,
    item: CheckMenuItem,
    /// Set from the tray toggle; wins over the policy until the G-Helper mode changes.
    manual: Option<bool>,
    state: SvcState,
    last_err: Option<String>,
    shown_text: String,
}

struct App {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    scm: Option<Scm>,
    scm_err: Option<String>,
    watcher: ModeWatcher,
    mode: Option<Mode>,
    svcs: Vec<Svc>,
    mode_item: MenuItem,
    auto_item: CheckMenuItem,
    log_item: MenuItem,
    cfg_item: MenuItem,
    quit_item: MenuItem,
    tray: TrayIcon,
    icon_color: [u8; 3],
    timer: usize,
}

fn mode_name(m: Option<Mode>) -> &'static str {
    m.map(Mode::name).unwrap_or("неизвестен")
}

fn file_mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn open_in_notepad(path: &std::path::Path) {
    let op = wide("open");
    let exe = wide("notepad.exe");
    let arg = wide(&format!("\"{}\"", path.display()));
    unsafe { ShellExecuteW(null_mut(), op.as_ptr(), exe.as_ptr(), arg.as_ptr(), null(), SW_SHOWNORMAL) };
}

fn make_icon(color: [u8; 3]) -> Icon {
    Icon::from_rgba(icon_rgba(color), 32, 32).expect("valid 32x32 RGBA icon")
}

impl App {
    fn auto_label(&self) -> String {
        let off = self.cfg.policy.off_modes();
        if off.is_empty() {
            "Автоматика (по политике в config.json)".to_string()
        } else {
            format!("Автоматика: выключать в {}", off.join(", "))
        }
    }

    fn set_timer(&mut self) {
        if self.timer != 0 {
            unsafe { KillTimer(null_mut(), self.timer) };
        }
        self.timer = unsafe { SetTimer(null_mut(), 0, self.cfg.poll_ms, None) };
    }

    /// Re-read config.json when it was edited by hand.
    fn reload_config_if_changed(&mut self) {
        let m = file_mtime(&config_path());
        if m.is_none() || m == self.cfg_mtime {
            return;
        }
        self.cfg_mtime = m;
        let (cfg, err) = Config::load();
        if let Some(e) = err {
            log(&e);
            return;
        }
        let poll_changed = cfg.poll_ms != self.cfg.poll_ms;
        if cfg.ghelper_path() != *self.watcher.path() {
            self.watcher = ModeWatcher::new(cfg.ghelper_path());
        }
        self.cfg = cfg;
        if poll_changed {
            self.set_timer();
        }
        log(&format!(
            "настройки перечитаны: auto={}, policy={:?}, poll_ms={}",
            self.cfg.auto, self.cfg.policy, self.cfg.poll_ms
        ));
    }

    fn tick(&mut self) {
        self.reload_config_if_changed();

        if self.scm.is_none() {
            match Scm::connect() {
                Ok(s) => {
                    self.scm = Some(s);
                    self.scm_err = None;
                }
                Err(e) => {
                    let m = format!("SCM недоступен: {}", describe(e));
                    if self.scm_err.as_ref() != Some(&m) {
                        log(&m);
                    }
                    self.scm_err = Some(m);
                }
            }
        }

        let mode = self.watcher.poll();
        if mode != self.mode {
            log(&format!("режим G-Helper: {} -> {}", mode_name(self.mode), mode_name(mode)));
            for s in &mut self.svcs {
                if s.manual.take().is_some() {
                    log(&format!("{}: ручной режим сброшен сменой режима G-Helper", s.name));
                }
            }
            self.mode = mode;
        }

        for i in 0..self.svcs.len() {
            self.reconcile(i, "автоматика");
        }
        self.refresh_ui();
    }

    /// Read the state of one service and issue a start/stop if it differs from the desired one.
    fn reconcile(&mut self, i: usize, why: &str) {
        let Some(scm) = self.scm.as_ref() else {
            self.svcs[i].state = SvcState::Unknown;
            return;
        };
        let s = &mut self.svcs[i];
        s.state = match scm.state(s.name) {
            Ok(st) => st,
            Err(e) => {
                let m = format!("{}: не удалось прочитать состояние — {}", s.name, describe(e));
                if s.last_err.as_ref() != Some(&m) {
                    log(&m);
                }
                s.last_err = Some(m);
                SvcState::Unknown
            }
        };
        let want = desired(s.manual, self.cfg.auto, self.mode, &self.cfg.policy);
        let Some(cmd) = command(want, s.state) else { return };
        let (res, verb) = match cmd {
            Command::Start => (scm.start(s.name), "запуск"),
            Command::Stop => (scm.stop(s.name), "остановка"),
        };
        match res {
            Ok(()) => {
                log(&format!("{}: {verb} ({why}, режим {})", s.name, mode_name(self.mode)));
                s.last_err = None;
                s.state = SvcState::Pending;
            }
            Err(e) => {
                let m = format!("{}: {verb} не удалась — {}", s.name, describe(e));
                if s.last_err.as_ref() != Some(&m) {
                    log(&m);
                }
                s.last_err = Some(m);
            }
        }
    }

    fn toggle(&mut self, i: usize) {
        let want = self.svcs[i].state != SvcState::Running;
        self.svcs[i].manual = Some(want);
        log(&format!(
            "{}: вручную {} (до смены режима G-Helper)",
            self.svcs[i].name,
            if want { "включить" } else { "выключить" }
        ));
        self.reconcile(i, "вручную");
        self.refresh_ui();
    }

    fn refresh_ui(&mut self) {
        for s in &mut self.svcs {
            // muda flips the check on click; always show the real state
            s.item.set_checked(s.state == SvcState::Running);
            let mut text = s.label.to_string();
            match s.state {
                SvcState::Pending => text.push_str(" — …"),
                SvcState::Unknown => text.push_str(" — ?"),
                _ => {}
            }
            if s.manual.is_some() {
                text.push_str(" [вручную]");
            }
            if s.last_err.is_some() {
                text.push_str(" ⚠");
            }
            if text != s.shown_text {
                s.item.set_text(&text);
                s.shown_text = text;
            }
        }

        self.mode_item.set_text(format!("G-Helper: {}", mode_name(self.mode)));
        self.auto_item.set_checked(self.cfg.auto);
        self.auto_item.set_text(self.auto_label());

        let any_unknown = self.scm.is_none() || self.svcs.iter().any(|s| s.state == SvcState::Unknown);
        let any_on = self.svcs.iter().any(|s| matches!(s.state, SvcState::Running | SvcState::Pending));
        let color = if any_unknown { GREY } else if any_on { ORANGE } else { GREEN };
        if color != self.icon_color {
            let _ = self.tray.set_icon(Some(make_icon(color)));
            self.icon_color = color;
        }

        let st = |s: &Svc| match s.state {
            SvcState::Running => "вкл",
            SvcState::Stopped => "выкл",
            SvcState::Pending => "…",
            SvcState::Unknown => "?",
        };
        let mut tip = format!("NVIDIA службы · G-Helper: {}\n", mode_name(self.mode));
        tip.push_str(&self.svcs.iter().map(|s| format!("{}: {}", s.short, st(s))).collect::<Vec<_>>().join(" · "));
        if let Some(e) = &self.scm_err {
            tip = format!("{tip}\n{e}");
        }
        tip.truncate(tip.char_indices().nth(120).map(|(i, _)| i).unwrap_or(tip.len()));
        let _ = self.tray.set_tooltip(Some(tip));
    }

    /// Returns false when the app should exit.
    fn on_menu(&mut self, ev: MenuEvent) -> bool {
        if ev.id == *self.quit_item.id() {
            log("выход по команде из меню");
            return false;
        }
        if ev.id == *self.auto_item.id() {
            self.cfg.auto = !self.cfg.auto;
            log(&format!("автоматика: {}", if self.cfg.auto { "вкл" } else { "выкл" }));
            if let Err(e) = self.cfg.save() {
                log(&format!("не удалось сохранить настройки: {e}"));
            }
            self.cfg_mtime = file_mtime(&config_path());
            self.tick();
        } else if ev.id == *self.log_item.id() {
            open_in_notepad(&log_path());
        } else if ev.id == *self.cfg_item.id() {
            open_in_notepad(&config_path());
        } else if let Some(i) = self.svcs.iter().position(|s| ev.id == *s.item.id()) {
            self.toggle(i);
        }
        true
    }
}

fn main() {
    if !single_instance("Local\\nv-service-tray") {
        return;
    }
    let (cfg, cfg_err) = Config::load();
    log(&format!("старт v{}, G-Helper config: {}", env!("CARGO_PKG_VERSION"), cfg.ghelper_path().display()));
    if let Some(e) = cfg_err {
        log(&e);
    }

    let mode_item = MenuItem::new("G-Helper: …", false, None);
    let svc_items: Vec<CheckMenuItem> =
        SERVICES.iter().map(|(_, _, label)| CheckMenuItem::new(label, true, false, None)).collect();
    let auto_item = CheckMenuItem::new("Автоматика", true, cfg.auto, None);
    let log_item = MenuItem::new("Открыть лог", true, None);
    let cfg_item = MenuItem::new("Открыть настройки (config.json)", true, None);
    let quit_item = MenuItem::new("Выход", true, None);

    let menu = Menu::new();
    let sep1 = PredefinedMenuItem::separator();
    let sep2 = PredefinedMenuItem::separator();
    let sep3 = PredefinedMenuItem::separator();
    menu.append_items(&[
        &mode_item,
        &sep1,
        &svc_items[0],
        &svc_items[1],
        &sep2,
        &auto_item,
        &log_item,
        &cfg_item,
        &sep3,
        &quit_item,
    ])
    .expect("menu");

    let tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_tooltip("NVIDIA службы")
        .with_icon(make_icon(GREY))
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            log(&format!("не удалось создать иконку в трее: {e}"));
            return;
        }
    };

    let svcs = SERVICES
        .iter()
        .zip(svc_items)
        .map(|(&(name, short, label), item)| Svc {
            name,
            short,
            label,
            item,
            manual: None,
            state: SvcState::Unknown,
            last_err: None,
            shown_text: String::new(),
        })
        .collect();

    let mut app = App {
        watcher: ModeWatcher::new(cfg.ghelper_path()),
        cfg,
        cfg_mtime: file_mtime(&config_path()),
        scm: None,
        scm_err: None,
        mode: None,
        svcs,
        mode_item,
        auto_item,
        log_item,
        cfg_item,
        quit_item,
        tray,
        icon_color: GREY,
        timer: 0,
    };
    app.set_timer();
    app.tick();

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let r = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if r == 0 || r == -1 {
            break;
        }
        if msg.message == WM_TIMER && msg.hwnd.is_null() {
            app.tick();
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if !app.on_menu(ev) {
                return;
            }
        }
    }
}
