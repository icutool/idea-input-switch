use std::{iter, os::windows::ffi::OsStrExt, process::Command, sync::Mutex};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::warn;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLIP_DEFAULT_PRECIS, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DEFAULT_QUALITY, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FillRect,
    HBRUSH, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics,
    GetWindowTextLengthW, GetWindowTextW, IsWindow, MessageBoxW, PostMessageW, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, BS_OWNERDRAW,
    CW_USEDEFAULT, EN_SETFOCUS, ES_AUTOHSCROLL, ES_READONLY, HMENU, LBN_SELCHANGE,
    LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, MB_ICONWARNING, MB_OK,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOZORDER, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_NCDESTROY, WM_PAINT, WM_SETFONT,
    WNDCLASSW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const APP_KEY: &str = "Software\\IdeaInputSwitch";
const CHARACTER_ALIASES_VALUE: &str = "CharacterAliases";

const WINDOW_CLASS: PCWSTR = w!("IdeaInputSwitchCharacterAliasWindow");
const WINDOW_TITLE: PCWSTR = w!("字符别名");
const WINDOW_WIDTH: i32 = 760;
const WINDOW_HEIGHT: i32 = 560;
const LEFT_CARD: RECT = RECT {
    left: 28,
    top: 104,
    right: 358,
    bottom: 460,
};
const RIGHT_CARD: RECT = RECT {
    left: 382,
    top: 104,
    right: 716,
    bottom: 460,
};
const COLOR_BG: u32 = 0x00F7F7F7;
const COLOR_CARD: u32 = 0x00FFFFFF;
const COLOR_BORDER: u32 = 0x00E5E5E5;
const COLOR_FIELD: u32 = 0x00FBFBFD;
const COLOR_PRIMARY: u32 = 0x00FF7A00;
const COLOR_BUTTON: u32 = 0x00F4F4F6;
const COLOR_BUTTON_PRESSED: u32 = 0x00E7E7EA;
const COLOR_TEXT: u32 = 0x00231F20;
const COLOR_MUTED: u32 = 0x008A8582;

const ID_LIST: usize = 2001;
const ID_TRIGGER_EDIT: usize = 2002;
const ID_OUTPUT_EDIT: usize = 2003;
const ID_ADD: usize = 2004;
const ID_SAVE: usize = 2005;
const ID_DELETE: usize = 2006;
const ID_CHARMAP: usize = 2007;
const ID_STATUS: usize = 2008;
const ID_COMMON_START: usize = 2100;

const LB_ERR: isize = -1;
const COMMON_CHARS: &[&str] = &["、", "。", "「", "」", "『", "』", "·", "￥", "…", "—"];

pub const WM_ALIAS_CAPTURED: u32 = WM_APP + 30;

static WINDOW_STATE: Mutex<Option<AliasWindowState>> = Mutex::new(None);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CharacterAlias {
    pub trigger: KeyBinding,
    pub output: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub vk_code: u16,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

struct AliasWindowState {
    hwnd: HWND,
    list_hwnd: HWND,
    trigger_edit: HWND,
    output_edit: HWND,
    status_text: HWND,
    font: HGDIOBJ,
    title_font: HGDIOBJ,
    bg_brush: HBRUSH,
    card_brush: HBRUSH,
    field_brush: HBRUSH,
    selected_index: Option<usize>,
    pending_trigger: Option<KeyBinding>,
    aliases: Vec<CharacterAlias>,
}

unsafe impl Send for AliasWindowState {}

pub fn load_aliases() -> Vec<CharacterAlias> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(APP_KEY)
        .ok()
        .and_then(|key| key.get_value::<String, _>(CHARACTER_ALIASES_VALUE).ok())
        .and_then(|value| serde_json::from_str::<Vec<CharacterAlias>>(&value).ok())
        .unwrap_or_default()
}

pub fn save_aliases(aliases: &[CharacterAlias]) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(APP_KEY)?;
    key.set_value(
        CHARACTER_ALIASES_VALUE,
        &serde_json::to_string_pretty(aliases)?,
    )?;
    Ok(())
}

pub fn show_window(owner: HWND) -> Result<()> {
    if let Some(hwnd) = existing_window() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        return Ok(());
    }

    register_window_class()?;

    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            WINDOW_TITLE,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            owner,
            None,
            instance,
            None,
        )
    }
    .context("CreateWindowExW failed")?;

    center_window(hwnd);

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }

    Ok(())
}

pub fn binding_from_current_keyboard(vk_code: u16) -> Option<KeyBinding> {
    if is_modifier_vk(vk_code) {
        return None;
    }

    Some(KeyBinding {
        vk_code,
        modifiers: current_modifiers(),
    })
}

pub fn binding_matches(binding: KeyBinding, vk_code: u16) -> bool {
    binding.vk_code == vk_code && binding.modifiers == current_modifiers()
}

pub fn display_binding(binding: KeyBinding) -> String {
    let mut parts = Vec::new();
    if binding.modifiers.ctrl {
        parts.push("Ctrl".to_string());
    }
    if binding.modifiers.alt {
        parts.push("Alt".to_string());
    }
    if binding.modifiers.shift {
        parts.push("Shift".to_string());
    }
    parts.push(key_name(binding.vk_code));
    parts.join("+")
}

pub fn is_modifier_vk(vk_code: u16) -> bool {
    matches!(
        vk_code,
        0x10 | 0xA0 | 0xA1 | 0x11 | 0xA2 | 0xA3 | 0x12 | 0xA4 | 0xA5
    )
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            if let Err(error) = create_controls(hwnd) {
                warn!(?error, "failed to create character alias window controls");
                show_message(
                    hwnd,
                    "字符别名窗口创建失败",
                    &error.to_string(),
                    MB_ICONWARNING,
                );
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_window(hwnd);
            LRESULT(0)
        }
        WM_DRAWITEM => {
            let draw_item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            draw_button(draw_item);
            LRESULT(1)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            unsafe {
                let hdc = HDC(wparam.0 as _);
                let _ = SetBkMode(hdc, TRANSPARENT);
                let _ = SetTextColor(hdc, COLORREF(COLOR_TEXT));
            }
            let brush = WINDOW_STATE
                .lock()
                .ok()
                .and_then(|state| state.as_ref().map(|state| state.field_brush))
                .unwrap_or_default();
            LRESULT(brush.0 as isize)
        }
        WM_CTLCOLORBTN | WM_CTLCOLORSTATIC => {
            unsafe {
                let hdc = HDC(wparam.0 as _);
                let _ = SetBkMode(hdc, TRANSPARENT);
                let _ = SetTextColor(hdc, COLORREF(COLOR_MUTED));
            }
            let brush = WINDOW_STATE
                .lock()
                .ok()
                .and_then(|state| state.as_ref().map(|state| state.bg_brush))
                .unwrap_or_default();
            LRESULT(brush.0 as isize)
        }
        WM_ALIAS_CAPTURED => {
            let binding = KeyBinding {
                vk_code: wparam.0 as u16,
                modifiers: decode_modifiers(lparam.0 as u32),
            };
            handle_captured_binding(binding);
            LRESULT(0)
        }
        WM_CLOSE => {
            crate::hook::cancel_alias_capture();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY | WM_NCDESTROY => {
            crate::hook::cancel_alias_capture();
            if let Ok(mut state) = WINDOW_STATE.lock() {
                if state.as_ref().is_some_and(|state| state.hwnd == hwnd) {
                    if let Some(state) = state.as_ref() {
                        unsafe {
                            let _ = DeleteObject(state.font);
                            let _ = DeleteObject(state.title_font);
                            let _ = DeleteObject(state.bg_brush);
                            let _ = DeleteObject(state.card_brush);
                            let _ = DeleteObject(state.field_brush);
                        }
                    }
                    *state = None;
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn register_window_class() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error.0 != 1410 {
            return Err(anyhow!("RegisterClassW failed: {error:?}"));
        }
    }

    Ok(())
}

fn create_ui_font(height: i32, weight: i32) -> Result<HGDIOBJ> {
    let font = unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            DEFAULT_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Microsoft YaHei UI"),
        )
    };

    if font.is_invalid() {
        Err(anyhow!("CreateFontW failed"))
    } else {
        Ok(HGDIOBJ(font.0))
    }
}

fn set_control_font(hwnd: HWND, font: HGDIOBJ) {
    unsafe {
        let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
    }
}

fn paint_window(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let bg_brush = CreateSolidBrush(COLORREF(COLOR_BG));
        let card_brush = CreateSolidBrush(COLORREF(COLOR_CARD));
        let border_pen = CreatePen(PS_SOLID, 1, COLORREF(COLOR_BORDER));

        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let _ = FillRect(hdc, &client, bg_brush);

        let previous_pen = SelectObject(hdc, border_pen);
        let previous_brush = SelectObject(hdc, card_brush);
        let _ = RoundRect(
            hdc,
            LEFT_CARD.left,
            LEFT_CARD.top,
            LEFT_CARD.right,
            LEFT_CARD.bottom,
            18,
            18,
        );
        let _ = RoundRect(
            hdc,
            RIGHT_CARD.left,
            RIGHT_CARD.top,
            RIGHT_CARD.right,
            RIGHT_CARD.bottom,
            18,
            18,
        );

        let field_brush = CreateSolidBrush(COLORREF(COLOR_FIELD));
        let field_pen = CreatePen(PS_SOLID, 1, COLORREF(0x00D8D8DC));
        let _ = SelectObject(hdc, field_pen);
        let _ = SelectObject(hdc, field_brush);
        let _ = RoundRect(hdc, 46, 152, 338, 438, 12, 12);
        let _ = RoundRect(hdc, 410, 180, 686, 220, 12, 12);
        let _ = RoundRect(hdc, 410, 252, 686, 292, 12, 12);

        let _ = SelectObject(hdc, previous_brush);
        let _ = SelectObject(hdc, previous_pen);
        let _ = DeleteObject(field_brush);
        let _ = DeleteObject(field_pen);

        draw_text(hdc, "字符别名", 34, 28, 220, 30, true, COLOR_TEXT);
        draw_text(
            hdc,
            "把不顺手的按键映射成指定字符，适合中文标点和特殊符号。",
            34,
            64,
            620,
            24,
            false,
            COLOR_MUTED,
        );
        draw_text(hdc, "规则", 52, 124, 120, 26, true, COLOR_TEXT);
        draw_text(hdc, "编辑", 410, 124, 120, 26, true, COLOR_TEXT);
        draw_text(hdc, "触发键", 410, 160, 120, 22, false, COLOR_MUTED);
        draw_text(hdc, "输出字符", 410, 232, 120, 22, false, COLOR_MUTED);
        draw_text(hdc, "常用字符", 410, 304, 120, 22, false, COLOR_MUTED);

        let _ = DeleteObject(bg_brush);
        let _ = DeleteObject(card_brush);
        let _ = DeleteObject(border_pen);
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    title: bool,
    color: u32,
) {
    let font = if title {
        create_ui_font(20, 600).ok()
    } else {
        create_ui_font(15, 400).ok()
    };
    let previous_font = font.map(|font| SelectObject(hdc, font));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(color));

    let mut text = wide_null(text);
    let text_len = text.len().saturating_sub(1);
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    let _ = DrawTextW(
        hdc,
        &mut text[..text_len],
        &mut rect,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );

    if let Some(previous_font) = previous_font {
        let selected_font = SelectObject(hdc, previous_font);
        let _ = DeleteObject(selected_font);
    }
}

fn draw_button(item: &DRAWITEMSTRUCT) {
    let id = item.CtlID as usize;
    let Some(label) = button_label(id) else {
        return;
    };

    unsafe {
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let is_primary = id == ID_ADD;
        let is_chip = id >= ID_COMMON_START && id < ID_COMMON_START + COMMON_CHARS.len();

        let fill = if is_primary {
            if pressed {
                0x00D86A00
            } else {
                COLOR_PRIMARY
            }
        } else if pressed {
            COLOR_BUTTON_PRESSED
        } else {
            COLOR_BUTTON
        };
        let border = if is_primary { COLOR_PRIMARY } else { 0x00DADADF };
        let text_color = if disabled {
            0x00A9A9AF
        } else if is_primary {
            0x00FFFFFF
        } else {
            COLOR_TEXT
        };

        let brush = CreateSolidBrush(COLORREF(fill));
        let pen = CreatePen(PS_SOLID, 1, COLORREF(border));
        let previous_brush = SelectObject(item.hDC, brush);
        let previous_pen = SelectObject(item.hDC, pen);

        let mut rect = item.rcItem;
        if pressed {
            rect.left += 1;
            rect.top += 1;
            rect.right += 1;
            rect.bottom += 1;
        }
        let radius = if is_chip { 10 } else { 14 };
        let _ = RoundRect(
            item.hDC,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        );

        let font = create_ui_font(if is_chip { 16 } else { 15 }, 500).ok();
        let previous_font = font.map(|font| SelectObject(item.hDC, font));
        let _ = SetBkMode(item.hDC, TRANSPARENT);
        let _ = SetTextColor(item.hDC, COLORREF(text_color));

        let mut text = wide_null(label);
        let text_len = text.len().saturating_sub(1);
        let _ = DrawTextW(
            item.hDC,
            &mut text[..text_len],
            &mut rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );

        if let Some(previous_font) = previous_font {
            let selected_font = SelectObject(item.hDC, previous_font);
            let _ = DeleteObject(selected_font);
        }
        let _ = SelectObject(item.hDC, previous_brush);
        let _ = SelectObject(item.hDC, previous_pen);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
    }
}

fn button_label(id: usize) -> Option<&'static str> {
    match id {
        ID_ADD => Some("新增规则"),
        ID_SAVE => Some("保存"),
        ID_DELETE => Some("删除"),
        ID_CHARMAP => Some("字符表..."),
        id if id >= ID_COMMON_START && id < ID_COMMON_START + COMMON_CHARS.len() => {
            Some(COMMON_CHARS[id - ID_COMMON_START])
        }
        _ => None,
    }
}

fn create_controls(hwnd: HWND) -> Result<()> {
    let font = create_ui_font(18, 400)?;
    let title_font = create_ui_font(20, 600)?;
    let bg_brush = unsafe { CreateSolidBrush(COLORREF(COLOR_BG)) };
    let card_brush = unsafe { CreateSolidBrush(COLORREF(COLOR_CARD)) };
    let field_brush = unsafe { CreateSolidBrush(COLORREF(COLOR_FIELD)) };

    let list_hwnd = create_control(
        hwnd,
        w!("LISTBOX"),
        "",
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | style(LBS_NOTIFY),
        58,
        164,
        268,
        262,
        ID_LIST,
    )?;
    set_control_font(list_hwnd, font);

    let trigger_edit = create_control(
        hwnd,
        w!("EDIT"),
        "点击后按触发键",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(ES_AUTOHSCROLL) | style(ES_READONLY),
        418,
        190,
        260,
        22,
        ID_TRIGGER_EDIT,
    )?;
    set_control_font(trigger_edit, font);

    let output_edit = create_control(
        hwnd,
        w!("EDIT"),
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(ES_AUTOHSCROLL),
        418,
        262,
        260,
        22,
        ID_OUTPUT_EDIT,
    )?;
    set_control_font(output_edit, font);

    let status_text = create_control(
        hwnd,
        w!("STATIC"),
        "选择左侧规则，或录制一个新触发键。",
        WS_CHILD | WS_VISIBLE,
        34,
        488,
        670,
        24,
        ID_STATUS,
    )?;
    set_control_font(status_text, font);

    let add_button = create_control(
        hwnd,
        w!("BUTTON"),
        "新增规则",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(BS_OWNERDRAW),
        410,
        408,
        94,
        34,
        ID_ADD,
    )?;
    set_control_font(add_button, font);

    let save_button = create_control(
        hwnd,
        w!("BUTTON"),
        "保存",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(BS_OWNERDRAW),
        518,
        408,
        76,
        34,
        ID_SAVE,
    )?;
    set_control_font(save_button, font);

    let delete_button = create_control(
        hwnd,
        w!("BUTTON"),
        "删除",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(BS_OWNERDRAW),
        608,
        408,
        78,
        34,
        ID_DELETE,
    )?;
    set_control_font(delete_button, font);

    let charmap_button = create_control(
        hwnd,
        w!("BUTTON"),
        "字符表...",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(BS_OWNERDRAW),
        592,
        298,
        94,
        30,
        ID_CHARMAP,
    )?;
    set_control_font(charmap_button, font);

    for (index, ch) in COMMON_CHARS.iter().enumerate() {
        let x = 410 + (index as i32 % 5) * 42;
        let y = 334 + (index as i32 / 5) * 34;
        let common_button = create_control(
            hwnd,
            w!("BUTTON"),
            ch,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | style(BS_OWNERDRAW),
            x,
            y,
            34,
            28,
            ID_COMMON_START + index,
        )?;
        set_control_font(common_button, font);
    }

    let aliases = load_aliases();
    let mut state = WINDOW_STATE
        .lock()
        .map_err(|_| anyhow!("character alias window state lock poisoned"))?;
    *state = Some(AliasWindowState {
        hwnd,
        list_hwnd,
        trigger_edit,
        output_edit,
        status_text,
        font,
        title_font,
        bg_brush,
        card_brush,
        field_brush,
        selected_index: None,
        pending_trigger: None,
        aliases,
    });
    drop(state);

    refresh_list();
    Ok(())
}

fn create_control(
    parent: HWND,
    class_name: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let text = wide_null(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            PCWSTR(text.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            parent,
            HMENU(id as *mut _),
            instance,
            None,
        )
    }
    .context("CreateWindowExW control failed")
}

fn handle_command(hwnd: HWND, wparam: WPARAM, _lparam: LPARAM) {
    let command_id = low_word(wparam.0) as usize;
    let notification = high_word(wparam.0);

    if command_id == ID_TRIGGER_EDIT && notification == EN_SETFOCUS as u16 {
        let _ = crate::hook::begin_alias_capture(hwnd, WM_ALIAS_CAPTURED);
        if let Ok(mut state) = WINDOW_STATE.lock() {
            if let Some(state) = state.as_mut() {
                state.pending_trigger = None;
                set_window_text(state.trigger_edit, "");
            }
        }
        set_status("请按下要作为别名触发的键。");
        return;
    }

    if command_id == ID_LIST && notification == LBN_SELCHANGE as u16 {
        select_current_list_item();
        return;
    }

    match command_id {
        ID_ADD => add_alias(hwnd),
        ID_SAVE => save_selected_alias(hwnd),
        ID_DELETE => delete_selected_alias(),
        ID_CHARMAP => open_character_map(hwnd),
        id if id >= ID_COMMON_START && id < ID_COMMON_START + COMMON_CHARS.len() => {
            let ch = COMMON_CHARS[id - ID_COMMON_START];
            set_output_text(ch);
        }
        _ => {}
    }
}

fn handle_captured_binding(binding: KeyBinding) {
    if let Ok(mut state) = WINDOW_STATE.lock() {
        let Some(state) = state.as_mut() else {
            return;
        };
        state.pending_trigger = Some(binding);
        let display = display_binding(binding);
        set_window_text(state.trigger_edit, &display);

        match binding_conflict(&state.aliases, state.selected_index, binding) {
            Some(message) => set_window_text(state.status_text, &message),
            None => set_window_text(state.status_text, "触发键已录制，填写输出字符后保存。"),
        }
    }
}

fn add_alias(hwnd: HWND) {
    if let Err(error) = add_alias_inner() {
        show_message(hwnd, "无法新增字符别名", &error.to_string(), MB_ICONWARNING);
        set_status(&error.to_string());
    }
}

fn add_alias_inner() -> Result<()> {
    let mut guard = WINDOW_STATE
        .lock()
        .map_err(|_| anyhow!("character alias window state lock poisoned"))?;
    let Some(state) = guard.as_mut() else {
        return Ok(());
    };

    let trigger = state
        .pending_trigger
        .ok_or_else(|| anyhow!("请先点击触发键输入框并按下一个键。"))?;
    let output = get_window_text(state.output_edit);
    validate_alias(&state.aliases, None, trigger, &output)?;

    state.aliases.push(CharacterAlias { trigger, output });
    state.selected_index = Some(state.aliases.len() - 1);
    save_aliases(&state.aliases)?;
    crate::hook::reload_character_aliases(state.aliases.clone());
    set_window_text(state.status_text, "字符别名已新增。");
    drop(guard);
    refresh_list();
    select_list_index();
    Ok(())
}

fn save_selected_alias(hwnd: HWND) {
    if let Err(error) = save_selected_alias_inner() {
        show_message(hwnd, "无法保存字符别名", &error.to_string(), MB_ICONWARNING);
        set_status(&error.to_string());
    }
}

fn save_selected_alias_inner() -> Result<()> {
    let mut guard = WINDOW_STATE
        .lock()
        .map_err(|_| anyhow!("character alias window state lock poisoned"))?;
    let Some(state) = guard.as_mut() else {
        return Ok(());
    };

    let index = state
        .selected_index
        .ok_or_else(|| anyhow!("请先选择左侧规则，或点击新增。"))?;
    let trigger = state
        .pending_trigger
        .ok_or_else(|| anyhow!("请先点击触发键输入框并按下一个键。"))?;
    let output = get_window_text(state.output_edit);
    validate_alias(&state.aliases, Some(index), trigger, &output)?;

    state.aliases[index] = CharacterAlias { trigger, output };
    save_aliases(&state.aliases)?;
    crate::hook::reload_character_aliases(state.aliases.clone());
    set_window_text(state.status_text, "字符别名已保存。");
    drop(guard);
    refresh_list();
    select_list_index();
    Ok(())
}

fn delete_selected_alias() {
    let mut guard = match WINDOW_STATE.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let Some(state) = guard.as_mut() else {
        return;
    };
    let Some(index) = state.selected_index else {
        set_window_text(state.status_text, "请先选择要删除的规则。");
        return;
    };
    if index >= state.aliases.len() {
        return;
    }

    state.aliases.remove(index);
    state.selected_index = None;
    state.pending_trigger = None;
    let _ = save_aliases(&state.aliases);
    crate::hook::reload_character_aliases(state.aliases.clone());
    set_window_text(state.trigger_edit, "点击这里后按触发键");
    set_window_text(state.output_edit, "");
    set_window_text(state.status_text, "字符别名已删除。");
    drop(guard);
    refresh_list();
}

fn select_current_list_item() {
    let mut state = match WINDOW_STATE.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let Some(state) = state.as_mut() else {
        return;
    };

    let selected = unsafe { SendMessageW(state.list_hwnd, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
    if selected == LB_ERR {
        state.selected_index = None;
        return;
    }
    let index = selected as usize;
    if let Some(alias) = state.aliases.get(index) {
        state.selected_index = Some(index);
        state.pending_trigger = Some(alias.trigger);
        set_window_text(state.trigger_edit, &display_binding(alias.trigger));
        set_window_text(state.output_edit, &alias.output);
        set_window_text(state.status_text, "已载入规则，可修改后保存。");
    }
}

fn refresh_list() {
    let state = match WINDOW_STATE.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let Some(state) = state.as_ref() else {
        return;
    };

    unsafe {
        let _ = SendMessageW(state.list_hwnd, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    }

    for alias in &state.aliases {
        let row = format!("{}  ->  {}", display_binding(alias.trigger), alias.output);
        let row = wide_null(&row);
        unsafe {
            let _ = SendMessageW(
                state.list_hwnd,
                LB_ADDSTRING,
                WPARAM(0),
                LPARAM(row.as_ptr() as isize),
            );
        }
    }
}

fn select_list_index() {
    let state = match WINDOW_STATE.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let Some(state) = state.as_ref() else {
        return;
    };
    if let Some(index) = state.selected_index {
        unsafe {
            let _ = SendMessageW(state.list_hwnd, LB_SETCURSEL, WPARAM(index), LPARAM(0));
        }
    }
}

fn validate_alias(
    aliases: &[CharacterAlias],
    ignore_index: Option<usize>,
    trigger: KeyBinding,
    output: &str,
) -> Result<()> {
    if output.trim().is_empty() {
        return Err(anyhow!("请填写要输出的字符。"));
    }

    if let Some(message) = binding_conflict(aliases, ignore_index, trigger) {
        return Err(anyhow!(message));
    }

    Ok(())
}

fn binding_conflict(
    aliases: &[CharacterAlias],
    ignore_index: Option<usize>,
    trigger: KeyBinding,
) -> Option<String> {
    if trigger.vk_code == 0x0D {
        return Some("Enter 已被内置注释换行监听占用。".to_string());
    }

    if trigger.vk_code == 0xBF && trigger.modifiers == KeyModifiers::default() {
        return Some("/ 已被内置 // 监听占用。".to_string());
    }

    let plain_shift = KeyModifiers {
        shift: true,
        ..KeyModifiers::default()
    };
    if (trigger.vk_code == 0x38 && trigger.modifiers == plain_shift)
        || (trigger.vk_code == 0x6A && trigger.modifiers == KeyModifiers::default())
    {
        return Some("* 已被内置 /** 监听占用。".to_string());
    }

    aliases
        .iter()
        .enumerate()
        .find(|(index, alias)| Some(*index) != ignore_index && alias.trigger == trigger)
        .map(|(_, _)| format!("{} 已被另一个字符别名占用。", display_binding(trigger)))
}

fn open_character_map(hwnd: HWND) {
    match Command::new("charmap.exe").spawn() {
        Ok(_) => set_status("已打开 Windows 字符表，可复制字符后粘贴到输出字符。"),
        Err(error) => show_message(hwnd, "无法打开字符表", &error.to_string(), MB_ICONWARNING),
    }
}

fn set_output_text(text: &str) {
    if let Ok(state) = WINDOW_STATE.lock() {
        if let Some(state) = state.as_ref() {
            set_window_text(state.output_edit, text);
        }
    }
}

fn set_status(text: &str) {
    if let Ok(state) = WINDOW_STATE.lock() {
        if let Some(state) = state.as_ref() {
            set_window_text(state.status_text, text);
        }
    }
}

fn get_window_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied as usize])
}

fn set_window_text(hwnd: HWND, text: &str) {
    let text = wide_null(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }
}

fn show_message(
    hwnd: HWND,
    title: &str,
    message: &str,
    icon: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) {
    let title = wide_null(title);
    let message = wide_null(message);
    unsafe {
        let _ = MessageBoxW(
            hwnd,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | icon,
        );
    }
}

fn existing_window() -> Option<HWND> {
    let state = WINDOW_STATE.lock().ok()?;
    let hwnd = state.as_ref()?.hwnd;
    if unsafe { IsWindow(hwnd).as_bool() } {
        Some(hwnd)
    } else {
        None
    }
}

fn center_window(hwnd: HWND) {
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (screen_width - WINDOW_WIDTH).max(0) / 2;
    let y = (screen_height - WINDOW_HEIGHT).max(0) / 2;
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, WINDOW_WIDTH, WINDOW_HEIGHT, SWP_NOZORDER);
    }
}

fn current_modifiers() -> KeyModifiers {
    KeyModifiers {
        ctrl: key_down(VK_CONTROL.0 as i32)
            || key_down(VK_LCONTROL.0 as i32)
            || key_down(VK_RCONTROL.0 as i32),
        alt: key_down(VK_MENU.0 as i32)
            || key_down(VK_LMENU.0 as i32)
            || key_down(VK_RMENU.0 as i32),
        shift: key_down(VK_SHIFT.0 as i32)
            || key_down(VK_LSHIFT.0 as i32)
            || key_down(VK_RSHIFT.0 as i32),
    }
}

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

fn encode_modifiers(modifiers: KeyModifiers) -> u32 {
    (modifiers.ctrl as u32) | ((modifiers.alt as u32) << 1) | ((modifiers.shift as u32) << 2)
}

fn decode_modifiers(value: u32) -> KeyModifiers {
    KeyModifiers {
        ctrl: value & 1 != 0,
        alt: value & 2 != 0,
        shift: value & 4 != 0,
    }
}

pub fn post_captured_binding(hwnd: HWND, message_id: u32, binding: KeyBinding) {
    unsafe {
        let _ = PostMessageW(
            hwnd,
            message_id,
            WPARAM(binding.vk_code as usize),
            LPARAM(encode_modifiers(binding.modifiers) as isize),
        );
    }
}

fn key_name(vk_code: u16) -> String {
    match vk_code {
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x1B => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x23 => "End".to_string(),
        0x24 => "Home".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x2D => "Insert".to_string(),
        0x2E => "Delete".to_string(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk_code as u32)
            .map(|ch| ch.to_string())
            .unwrap_or_else(|| format!("VK {vk_code}")),
        0x70..=0x87 => format!("F{}", vk_code - 0x6F),
        0xBA => ";".to_string(),
        0xBB => "=".to_string(),
        0xBC => ",".to_string(),
        0xBD => "-".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xC0 => "`".to_string(),
        0xDB => "[".to_string(),
        0xDC => "\\".to_string(),
        0xDD => "]".to_string(),
        0xDE => "'".to_string(),
        _ => format!("VK {vk_code}"),
    }
}

fn low_word(value: usize) -> u16 {
    (value & 0xffff) as u16
}

fn high_word(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}

fn wide_null(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

fn style(value: i32) -> WINDOW_STYLE {
    WINDOW_STYLE(value as u32)
}
