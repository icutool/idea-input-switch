use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    FillRect, GetStockObject, SelectObject, SetBkMode, SetTextColor,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DT_LEFT, DT_SINGLELINE,
    DT_VCENTER, FF_DONTCARE, HBRUSH, HGDIOBJ, NULL_BRUSH, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetSystemMetrics, KillTimer,
    LoadCursorW, RegisterClassW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HMENU, HWND_TOPMOST,
    IDC_ARROW, LWA_ALPHA, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    WM_CLOSE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::{ime::ImeMode, tray};

const POPUP_CLASS: PCWSTR = w!("IdeaInputSwitchPopupWindow");
const POPUP_TITLE: PCWSTR = w!("IdeaInputSwitch");
const POPUP_TIMER_ID: usize = 1;
const POPUP_DURATION_MS: u32 = 2000;
const POPUP_WIDTH: i32 = 300;
const POPUP_HEIGHT: i32 = 60;
const POPUP_MARGIN: i32 = 20;

// 配色方案
const COLOR_BG: u32 = 0x00FFFFFF;          // 白色背景
const COLOR_BORDER_LEFT_CN: u32 = 0x0033AA44; // 中文模式绿色左边框
const COLOR_BORDER_LEFT_EN: u32 = 0x00CC5500; // 英文模式橙色左边框
const COLOR_BORDER_LEFT_DEF: u32 = 0x005588CC; // 默认蓝色左边框
const COLOR_TEXT_MAIN: u32 = 0x00222222;    // 主文字颜色
const COLOR_TEXT_SUB: u32 = 0x00888888;     // 副文字颜色
const COLOR_BORDER: u32 = 0x00E0E0E0;      // 外框线颜色
const ACCENT_BAR_WIDTH: i32 = 4;           // 左侧彩色竖条宽度
const WINDOW_ALPHA: u8 = 245;              // 窗口透明度

static POPUP_STATE: OnceLock<Mutex<PopupState>> = OnceLock::new();

struct PopupState {
    class_registered: bool,
    popup_hwnd_raw: isize,
    // 当前显示的文本和图标类型，供 WM_PAINT 使用
    main_text: String,
    sub_text: String,
    accent_color: u32,
}

impl Default for PopupState {
    fn default() -> Self {
        Self {
            class_registered: false,
            popup_hwnd_raw: 0,
            main_text: String::new(),
            sub_text: String::new(),
            accent_color: COLOR_BORDER_LEFT_DEF,
        }
    }
}

pub fn show_mode_switch(hwnd: HWND, mode: ImeMode) -> Result<()> {
    match mode {
        ImeMode::Chinese => show_popup(
            hwnd,
            "已切换到中文输入",
            "// 触发 · 中文模式",
            COLOR_BORDER_LEFT_CN,
        ),
        ImeMode::English => show_popup(
            hwnd,
            "已切换到英文输入",
            "Enter 触发 · 英文模式",
            COLOR_BORDER_LEFT_EN,
        ),
        ImeMode::Unknown => show_popup(
            hwnd,
            "输入法状态未知",
            "无法识别当前输入法",
            COLOR_BORDER_LEFT_DEF,
        ),
    }
}

pub fn show_pause_status(hwnd: HWND, paused: bool) -> Result<()> {
    if paused {
        show_popup(hwnd, "已暂停监听", "IDEA 输入法切换已停用", COLOR_BORDER_LEFT_DEF)
    } else {
        show_popup(hwnd, "已恢复监听", "IDEA 输入法切换已启用", COLOR_BORDER_LEFT_CN)
    }
}

pub fn show_autostart_status(hwnd: HWND, enabled: bool) -> Result<()> {
    if enabled {
        show_popup(hwnd, "已开启开机自启", "下次开机将自动运行", COLOR_BORDER_LEFT_CN)
    } else {
        show_popup(hwnd, "已关闭开机自启", "下次开机不再自动运行", COLOR_BORDER_LEFT_DEF)
    }
}

pub fn show_status(hwnd: HWND, mode: ImeMode, paused: bool, autostart_enabled: bool) -> Result<()> {
    let mode_label = match mode {
        ImeMode::Chinese => "中文",
        ImeMode::English => "英文",
        ImeMode::Unknown => "未知",
    };
    let pause_label = if paused { "已暂停" } else { "监听中" };
    let autostart_label = if autostart_enabled { "开" } else { "关" };

    let sub = format!("输入法：{mode_label}  ·  自启：{autostart_label}");
    let accent = if paused {
        COLOR_BORDER_LEFT_DEF
    } else if mode == ImeMode::Chinese {
        COLOR_BORDER_LEFT_CN
    } else {
        COLOR_BORDER_LEFT_EN
    };
    show_popup(hwnd, pause_label, &sub, accent)
}

pub fn show_already_running(hwnd: HWND) -> Result<()> {
    show_popup(hwnd, "已在运行", "无需重复启动，请查看托盘区图标", COLOR_BORDER_LEFT_DEF)
}

pub fn show_started(hwnd: HWND) -> Result<()> {
    show_popup(hwnd, "已启动", "IdeaInputSwitch 正在后台运行", COLOR_BORDER_LEFT_CN)
}

#[allow(dead_code)]
pub fn show_system_balloon(hwnd: HWND, title: &str, message: &str) -> Result<()> {
    tray::show_balloon(hwnd, title, message)
}

fn show_popup(owner: HWND, main: &str, sub: &str, accent: u32) -> Result<()> {
    // 先更新文字内容到状态，再触发重绘
    {
        let state = POPUP_STATE.get_or_init(|| Mutex::new(PopupState::default()));
        let mut s = state.lock().map_err(|_| anyhow!("popup state poisoned"))?;
        s.main_text = main.to_string();
        s.sub_text = sub.to_string();
        s.accent_color = accent;
    }

    let popup = ensure_popup_window(owner)?;

    unsafe {
        let _ = KillTimer(popup, POPUP_TIMER_ID);

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let x = screen_w - POPUP_WIDTH - POPUP_MARGIN;
        let y = screen_h - POPUP_HEIGHT - POPUP_MARGIN - 48;

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

        // 强制重绘
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(popup, None, true);
        let _ = windows::Win32::Graphics::Gdi::UpdateWindow(popup);

        let _ = SetTimer(popup, POPUP_TIMER_ID, POPUP_DURATION_MS, None);
    }

    Ok(())
}

fn ensure_popup_window(owner: HWND) -> Result<HWND> {
    let state = POPUP_STATE.get_or_init(|| Mutex::new(PopupState::default()));
    let mut state = state.lock().map_err(|_| anyhow!("popup state poisoned"))?;

    if !state.class_registered {
        register_popup_class()?;
        state.class_registered = true;
    }

    if state.popup_hwnd_raw == 0 {
        let popup = create_popup_window(owner)?;
        state.popup_hwnd_raw = popup.0 as isize;
    }

    Ok(HWND(state.popup_hwnd_raw as _))
}

fn register_popup_class() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let null_brush = unsafe { HBRUSH(GetStockObject(NULL_BRUSH).0) };
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(popup_proc),
        hInstance: HINSTANCE(instance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).ok() }.unwrap_or_default(),
        hbrBackground: null_brush,
        lpszClassName: POPUP_CLASS,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        return Err(anyhow!("RegisterClassW for popup failed"));
    }

    Ok(())
}

fn create_popup_window(owner: HWND) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;

    let popup = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            POPUP_CLASS,
            POPUP_TITLE,
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            POPUP_WIDTH,
            POPUP_HEIGHT,
            owner,
            HMENU(0_isize as _),
            instance,
            None,
        )
    }
    .context("failed to create popup window")?;

    // 设置整体透明度
    unsafe {
        let _ = SetLayeredWindowAttributes(popup, COLORREF(0), WINDOW_ALPHA, LWA_ALPHA);
    }

    Ok(popup)
}

unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_popup(hwnd);
            LRESULT(0)
        }
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

unsafe fn paint_popup(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);

    // ── 背景 ──────────────────────────────────────────────
    let bg_brush = CreateSolidBrush(COLORREF(COLOR_BG));
    FillRect(hdc, &rc, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    // ── 外框线（浅灰） ─────────────────────────────────────
    let border_pen = CreatePen(PS_SOLID, 1, COLORREF(COLOR_BORDER));
    let old_pen = SelectObject(hdc, HGDIOBJ(border_pen.0));
    let null_brush = HBRUSH(GetStockObject(NULL_BRUSH).0);
    let old_brush = SelectObject(hdc, HGDIOBJ(null_brush.0));
    let _ = windows::Win32::Graphics::Gdi::Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(border_pen.0));

    // ── 读取当前状态中的颜色和文字 ────────────────────────
    let (main_text, sub_text, accent_color) = {
        if let Some(state) = POPUP_STATE.get() {
            if let Ok(s) = state.lock() {
                (s.main_text.clone(), s.sub_text.clone(), s.accent_color)
            } else {
                (String::new(), String::new(), COLOR_BORDER_LEFT_DEF)
            }
        } else {
            (String::new(), String::new(), COLOR_BORDER_LEFT_DEF)
        }
    };

    // ── 左侧彩色竖条 ───────────────────────────────────────
    let accent_brush = CreateSolidBrush(COLORREF(accent_color));
    let accent_rc = RECT {
        left: rc.left + 1,
        top: rc.top + 1,
        right: rc.left + 1 + ACCENT_BAR_WIDTH,
        bottom: rc.bottom - 1,
    };
    FillRect(hdc, &accent_rc, accent_brush);
    let _ = DeleteObject(HGDIOBJ(accent_brush.0));

    let text_left = rc.left + ACCENT_BAR_WIDTH + 14;

    // ── 主文字（粗体，较大） ───────────────────────────────
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(COLOR_TEXT_MAIN));

    let font_main = CreateFontW(
        20,                     // 字体高度
        0, 0, 0,
        700,                    // 粗体 FW_BOLD
        0, 0, 0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (FF_DONTCARE.0 | DEFAULT_PITCH.0) as u32,
        w!("Microsoft YaHei UI"),
    );
    let old_font = SelectObject(hdc, HGDIOBJ(font_main.0));

    let mut main_rc = RECT {
        left: text_left,
        top: rc.top + 8,
        right: rc.right - 10,
        bottom: rc.top + 8 + 22,
    };
    let mut main_wide = wide_null(&main_text);
    DrawTextW(
        hdc,
        &mut main_wide,
        &mut main_rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );

    SelectObject(hdc, old_font);
    let _ = DeleteObject(HGDIOBJ(font_main.0));

    // ── 副文字（细体，较小，灰色） ─────────────────────────
    SetTextColor(hdc, COLORREF(COLOR_TEXT_SUB));

    let font_sub = CreateFontW(
        13,
        0, 0, 0,
        400,                    // 正常 FW_NORMAL
        0, 0, 0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (FF_DONTCARE.0 | DEFAULT_PITCH.0) as u32,
        w!("Microsoft YaHei UI"),
    );
    let old_font2 = SelectObject(hdc, HGDIOBJ(font_sub.0));

    let mut sub_rc = RECT {
        left: text_left,
        top: rc.top + 32,
        right: rc.right - 10,
        bottom: rc.bottom - 6,
    };
    let mut sub_wide = wide_null(&sub_text);
    DrawTextW(
        hdc,
        &mut sub_wide,
        &mut sub_rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );

    SelectObject(hdc, old_font2);
    let _ = DeleteObject(HGDIOBJ(font_sub.0));

    let _ = EndPaint(hwnd, &ps);
}

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
