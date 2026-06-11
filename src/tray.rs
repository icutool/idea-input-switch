use std::{iter, os::windows::ffi::OsStrExt};

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
    TrackPopupMenu, HICON, IDI_APPLICATION, IDI_ERROR, IDI_INFORMATION, IDI_WARNING, MF_CHECKED,
    MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_RBUTTONUP,
};

use crate::ime::{ImeMode, InputMethod};

pub const WM_TRAYICON: u32 = WM_APP + 10;
pub const TRAY_UID: u32 = 1;

pub const ID_STATUS: usize = 1001;
pub const ID_TOGGLE_PAUSE: usize = 1002;
pub const ID_TOGGLE_AUTOSTART: usize = 1003;
pub const ID_QUIT: usize = 1004;
pub const ID_SELECT_SOGOU: usize = 1005;
pub const ID_SELECT_MICROSOFT: usize = 1006;

pub fn add_icon(hwnd: HWND, mode: ImeMode, paused: bool) -> Result<()> {
    let mut data = base_icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAYICON;
    data.hIcon = load_status_icon(mode, paused);
    fill_utf16(&mut data.szTip, &tooltip(mode, paused));

    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &mut data) }.as_bool();
    if !added {
        return Err(anyhow!("Shell_NotifyIconW(NIM_ADD) failed: {:?}", unsafe {
            GetLastError()
        }));
    }

    Ok(())
}

pub fn update_icon(hwnd: HWND, mode: ImeMode, paused: bool) -> Result<()> {
    let mut data = base_icon_data(hwnd);
    data.uFlags = NIF_ICON | NIF_TIP;
    data.hIcon = load_status_icon(mode, paused);
    fill_utf16(&mut data.szTip, &tooltip(mode, paused));

    let modified = unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut data) }.as_bool();
    if !modified {
        return Err(anyhow!("Shell_NotifyIconW(NIM_MODIFY) failed"));
    }

    Ok(())
}

pub fn remove_icon(hwnd: HWND) {
    let mut data = base_icon_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
    }
}

#[allow(dead_code)]
pub fn show_balloon(hwnd: HWND, title: &str, message: &str) -> Result<()> {
    let mut data = base_icon_data(hwnd);
    data.uFlags = NIF_INFO;
    fill_utf16(&mut data.szInfoTitle, title);
    fill_utf16(&mut data.szInfo, message);
    data.dwInfoFlags = if message.contains("中文") {
        NIIF_WARNING
    } else {
        NIIF_INFO
    };

    let modified = unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut data) }.as_bool();
    if !modified {
        return Err(anyhow!("Shell_NotifyIconW(NIM_MODIFY/NIF_INFO) failed"));
    }

    Ok(())
}

pub fn handle_callback(hwnd: HWND, lparam: LPARAM) {
    match lparam.0 as u32 {
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            let _ = show_context_menu(hwnd);
        }
        WM_LBUTTONUP => {
            if let Some((mode, paused, autostart_enabled, input_method)) = current_state() {
                let _ = crate::notify::show_status(
                    hwnd,
                    mode,
                    paused,
                    autostart_enabled,
                    input_method,
                );
            }
        }
        _ => {}
    }
}

pub fn show_context_menu(hwnd: HWND) -> Result<()> {
    let Some((mode, paused, autostart_enabled, input_method)) = current_state() else {
        return Err(anyhow!("application context unavailable"));
    };

    let state_label = format!(
        "状态: {} / {} / {}",
        if paused {
            "已暂停"
        } else {
            "运行中"
        },
        match mode {
            ImeMode::Chinese => "中文",
            ImeMode::English => "英文",
            ImeMode::Unknown => "未知",
        },
        input_method.label()
    );
    let pause_label = if paused {
        "恢复监听"
    } else {
        "暂停监听"
    };
    let autostart_label = if autostart_enabled {
        "关闭开机自启"
    } else {
        "开启开机自启"
    };

    let state_text = wide_null(&state_label);
    let pause_text = wide_null(pause_label);
    let autostart_text = wide_null(autostart_label);
    let input_method_text = wide_null("输入法设置");
    let sogou_text = wide_null("搜狗输入法");
    let microsoft_text = wide_null("微软拼音");
    let quit_text = wide_null("退出");

    let menu = unsafe { CreatePopupMenu() }?;
    let input_method_menu = unsafe { CreatePopupMenu() }?;
    let sogou_flags = MF_STRING
        | if input_method == InputMethod::Sogou {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
    let microsoft_flags = MF_STRING
        | if input_method == InputMethod::Microsoft {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };

    unsafe {
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            ID_STATUS,
            PCWSTR(state_text.as_ptr()),
        );
        let _ = AppendMenuW(
            input_method_menu,
            sogou_flags,
            ID_SELECT_SOGOU,
            PCWSTR(sogou_text.as_ptr()),
        );
        let _ = AppendMenuW(
            input_method_menu,
            microsoft_flags,
            ID_SELECT_MICROSOFT,
            PCWSTR(microsoft_text.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_POPUP,
            input_method_menu.0 as usize,
            PCWSTR(input_method_text.as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_TOGGLE_PAUSE,
            PCWSTR(pause_text.as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_TOGGLE_AUTOSTART,
            PCWSTR(autostart_text.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, PCWSTR(quit_text.as_ptr()));
    }

    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }

    Ok(())
}

fn current_state() -> Option<(ImeMode, bool, bool, InputMethod)> {
    let context = crate::APP_CONTEXT.get()?;
    let context = context.lock().ok()?;
    Some((
        context.current_mode,
        context.paused,
        context.autostart_enabled,
        context.input_method,
    ))
}

fn base_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW::default();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    data
}

fn tooltip(mode: ImeMode, paused: bool) -> String {
    if paused {
        "IdeaInputSwitch: 已暂停".to_string()
    } else {
        match mode {
            ImeMode::Chinese => "IdeaInputSwitch: 中文输入".to_string(),
            ImeMode::English => "IdeaInputSwitch: 英文输入".to_string(),
            ImeMode::Unknown => "IdeaInputSwitch: 状态未知".to_string(),
        }
    }
}

fn load_status_icon(mode: ImeMode, paused: bool) -> HICON {
    if let Some(icon) = load_embedded_app_icon() {
        return icon;
    }

    unsafe {
        if paused {
            return LoadIconW(None, IDI_ERROR).unwrap_or_default();
        }

        match mode {
            ImeMode::Chinese => LoadIconW(None, IDI_WARNING).unwrap_or_default(),
            ImeMode::English => LoadIconW(None, IDI_INFORMATION).unwrap_or_default(),
            ImeMode::Unknown => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        }
    }
}

fn load_embedded_app_icon() -> Option<HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        LoadIconW(HINSTANCE(module.0), PCWSTR(1 as *const u16)).ok()
    }
}

fn fill_utf16(buffer: &mut [u16], text: &str) {
    buffer.fill(0);
    for (slot, codepoint) in buffer.iter_mut().zip(wide_null(text)) {
        *slot = codepoint;
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}
