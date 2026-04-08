use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, GetStockObject, DEFAULT_GUI_FONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, KillTimer, LoadCursorW, RegisterClassW,
    SendMessageW, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, HMENU, HWND_TOPMOST, IDC_ARROW, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, WINDOW_EX_STYLE, WM_CLOSE, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::{ime::ImeMode, tray};

const POPUP_CLASS: PCWSTR = w!("IdeaImePopupWindow");
const STATIC_CLASS: PCWSTR = w!("STATIC");
const POPUP_TITLE: PCWSTR = w!("IdeaIME");
const POPUP_TIMER_ID: usize = 1;
const POPUP_DURATION_MS: u32 = 1000;
const POPUP_WIDTH: i32 = 320;
const POPUP_HEIGHT: i32 = 54;
const POPUP_MARGIN: i32 = 16;

static POPUP_STATE: OnceLock<Mutex<PopupState>> = OnceLock::new();

#[derive(Default)]
struct PopupState {
    class_registered: bool,
    popup_hwnd_raw: isize,
    label_hwnd_raw: isize,
}

pub fn show_mode_switch(hwnd: HWND, mode: ImeMode) -> Result<()> {
    let message = match mode {
        ImeMode::Chinese => "\u{5df2}\u{5207}\u{6362}\u{5230}\u{4e2d}\u{6587}\u{8f93}\u{5165}",
        ImeMode::English => "\u{5df2}\u{5207}\u{6362}\u{5230}\u{82f1}\u{6587}\u{8f93}\u{5165}",
        ImeMode::Unknown => "\u{8f93}\u{5165}\u{6cd5}\u{72b6}\u{6001}\u{672a}\u{77e5}",
    };

    show_popup(hwnd, message)
}

pub fn show_pause_status(hwnd: HWND, paused: bool) -> Result<()> {
    let message = if paused {
        "\u{5df2}\u{6682}\u{505c} IDEA \u{8f93}\u{5165}\u{6cd5}\u{76d1}\u{542c}"
    } else {
        "\u{5df2}\u{6062}\u{590d} IDEA \u{8f93}\u{5165}\u{6cd5}\u{76d1}\u{542c}"
    };

    show_popup(hwnd, message)
}

pub fn show_autostart_status(hwnd: HWND, enabled: bool) -> Result<()> {
    let message = if enabled {
        "\u{5df2}\u{5f00}\u{542f}\u{5f00}\u{673a}\u{81ea}\u{542f}"
    } else {
        "\u{5df2}\u{5173}\u{95ed}\u{5f00}\u{673a}\u{81ea}\u{542f}"
    };

    show_popup(hwnd, message)
}

pub fn show_status(hwnd: HWND, mode: ImeMode, paused: bool, autostart_enabled: bool) -> Result<()> {
    let mode_label = match mode {
        ImeMode::Chinese => "\u{4e2d}\u{6587}",
        ImeMode::English => "\u{82f1}\u{6587}",
        ImeMode::Unknown => "\u{672a}\u{77e5}",
    };
    let pause_label = if paused {
        "\u{5df2}\u{6682}\u{505c}"
    } else {
        "\u{76d1}\u{542c}\u{4e2d}"
    };
    let autostart_label = if autostart_enabled {
        "\u{5f00}"
    } else {
        "\u{5173}"
    };

    let message = format!(
        "{pause_label} | \u{5f53}\u{524d}\u{8f93}\u{5165}\u{6cd5}\u{ff1a}{mode_label} | \u{5f00}\u{673a}\u{81ea}\u{542f}\u{ff1a}{autostart_label}"
    );
    show_popup(hwnd, &message)
}

#[allow(dead_code)]
pub fn show_system_balloon(hwnd: HWND, title: &str, message: &str) -> Result<()> {
    tray::show_balloon(hwnd, title, message)
}

fn show_popup(owner: HWND, message: &str) -> Result<()> {
    let (popup, label) = ensure_popup_window(owner)?;
    let text = wide_null(message);

    unsafe {
        SetWindowTextW(label, PCWSTR(text.as_ptr())).context("failed to set popup text")?;
        let _ = KillTimer(popup, POPUP_TIMER_ID);

        let x = GetSystemMetrics(SM_CXSCREEN) - POPUP_WIDTH - POPUP_MARGIN;
        let y = GetSystemMetrics(SM_CYSCREEN) - POPUP_HEIGHT - POPUP_MARGIN - 48;
        SetWindowPos(
            popup,
            HWND_TOPMOST,
            x,
            y,
            POPUP_WIDTH,
            POPUP_HEIGHT,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .context("failed to show popup")?;
        let _ = SetTimer(popup, POPUP_TIMER_ID, POPUP_DURATION_MS, None);
    }

    Ok(())
}

fn ensure_popup_window(owner: HWND) -> Result<(HWND, HWND)> {
    let state = POPUP_STATE.get_or_init(|| Mutex::new(PopupState::default()));
    let mut state = state.lock().map_err(|_| anyhow!("popup state poisoned"))?;

    if !state.class_registered {
        register_popup_class()?;
        state.class_registered = true;
    }

    if state.popup_hwnd_raw == 0 || state.label_hwnd_raw == 0 {
        let (popup, label) = create_popup_window(owner)?;
        state.popup_hwnd_raw = popup.0 as isize;
        state.label_hwnd_raw = label.0 as isize;
    }

    Ok((
        HWND(state.popup_hwnd_raw as _),
        HWND(state.label_hwnd_raw as _),
    ))
}

fn register_popup_class() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let background = unsafe { CreateSolidBrush(COLORREF(0x00F7F7F7)) };
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(popup_proc),
        hInstance: HINSTANCE(instance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).ok() }.unwrap_or_default(),
        hbrBackground: background,
        lpszClassName: POPUP_CLASS,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        return Err(anyhow!("RegisterClassW for popup failed"));
    }

    Ok(())
}

fn create_popup_window(owner: HWND) -> Result<(HWND, HWND)> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;

    let popup = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            POPUP_CLASS,
            POPUP_TITLE,
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            POPUP_WIDTH,
            POPUP_HEIGHT,
            owner,
            None,
            instance,
            None,
        )
    }
    .context("failed to create popup window")?;

    let label = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            STATIC_CLASS,
            w!(""),
            WS_CHILD | WS_VISIBLE,
            14,
            16,
            POPUP_WIDTH - 28,
            22,
            popup,
            HMENU(1_isize as _),
            instance,
            None,
        )
    }
    .context("failed to create popup label")?;

    unsafe {
        let gui_font = GetStockObject(DEFAULT_GUI_FONT);
        let _ = SendMessageW(label, WM_SETFONT, WPARAM(gui_font.0 as usize), LPARAM(1));
    }

    Ok((popup, label))
}

unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            let _ = KillTimer(hwnd, POPUP_TIMER_ID);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
