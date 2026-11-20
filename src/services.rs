//! Thin wrapper over the Windows Service Control Manager.
//! Start/stop calls are fire-and-forget: the caller re-reads the state on the next tick,
//! so the UI thread never blocks waiting for a service.

use crate::policy::SvcState;
use std::ptr::null;
use windows_sys::Win32::Foundation::{ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_NOT_ACTIVE, GetLastError};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS,
    SERVICE_STOP, SERVICE_STOPPED, StartServiceW,
};

/// Win32 error code.
pub type WinErr = u32;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseServiceHandle(self.0) };
        }
    }
}

pub struct Scm(Handle);

impl Scm {
    pub fn connect() -> Result<Scm, WinErr> {
        let h = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
        if h.is_null() { Err(unsafe { GetLastError() }) } else { Ok(Scm(Handle(h))) }
    }

    fn open(&self, name: &str, access: u32) -> Result<Handle, WinErr> {
        let w = wide(name);
        let h = unsafe { OpenServiceW((self.0).0, w.as_ptr(), access) };
        if h.is_null() { Err(unsafe { GetLastError() }) } else { Ok(Handle(h)) }
    }

    pub fn state(&self, name: &str) -> Result<SvcState, WinErr> {
        let h = self.open(name, SERVICE_QUERY_STATUS)?;
        let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { QueryServiceStatus(h.0, &mut st) } == 0 {
            return Err(unsafe { GetLastError() });
        }
        Ok(match st.dwCurrentState {
            SERVICE_RUNNING => SvcState::Running,
            SERVICE_STOPPED => SvcState::Stopped,
            _ => SvcState::Pending,
        })
    }

    pub fn start(&self, name: &str) -> Result<(), WinErr> {
        let h = self.open(name, SERVICE_START)?;
        if unsafe { StartServiceW(h.0, 0, null()) } == 0 {
            let e = unsafe { GetLastError() };
            if e != ERROR_SERVICE_ALREADY_RUNNING {
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn stop(&self, name: &str) -> Result<(), WinErr> {
        let h = self.open(name, SERVICE_STOP)?;
        let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { ControlService(h.0, SERVICE_CONTROL_STOP, &mut st) } == 0 {
            let e = unsafe { GetLastError() };
            if e != ERROR_SERVICE_NOT_ACTIVE {
                return Err(e);
            }
        }
        Ok(())
    }
}

pub fn describe(e: WinErr) -> String {
    match e {
        5 => "нет прав (нужен запуск от администратора)".into(),
        1060 => "служба не найдена".into(),
        1061 => "служба сейчас не принимает команды".into(),
        _ => format!("ошибка Win32 {e}"),
    }
}
