use std::{
    sync::{mpsc::Sender, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_OEM_2, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

const SLASH_TRIGGER_WINDOW: Duration = Duration::from_millis(300);
const DOC_COMMENT_TRIGGER_WINDOW: Duration = Duration::from_secs(2);
const VK_8: u16 = b'8' as u16;
const VK_MULTIPLY: u16 = 0x6A;
const VK_SHIFT_CODE: u16 = 0x10;
const VK_LSHIFT: u16 = 0xA0;
const VK_RSHIFT: u16 = 0xA1;
const VK_CONTROL: u16 = 0x11;
const VK_LCONTROL: u16 = 0xA2;
const VK_RCONTROL: u16 = 0xA3;
const VK_MENU: u16 = 0x12;
const VK_LMENU: u16 = 0xA4;
const VK_RMENU: u16 = 0xA5;

static SENDER: OnceLock<Sender<HookEvent>> = OnceLock::new();
static NOTIFY_HWND_RAW: OnceLock<isize> = OnceLock::new();
static NOTIFY_MESSAGE: OnceLock<u32> = OnceLock::new();
static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();
static CHARACTER_ALIASES: OnceLock<Mutex<Vec<crate::char_alias::CharacterAlias>>> = OnceLock::new();
static ALIAS_CAPTURE_TARGET: OnceLock<Mutex<Option<AliasCaptureTarget>>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum HookEvent {
    SlashSequence,
    DocCommentEnter,
    EnterPressed,
}

#[derive(Default)]
struct HookState {
    last_slash_at: Option<Instant>,
    doc_comment_stage: u8,
    last_doc_comment_at: Option<Instant>,
}

#[derive(Clone, Copy)]
struct AliasCaptureTarget {
    hwnd_raw: isize,
    message_id: u32,
}

pub struct HookThread {
    thread_id: u32,
    join_handle: Option<JoinHandle<()>>,
}

impl HookThread {
    pub fn stop(mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }

        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn start(
    sender: Sender<HookEvent>,
    hwnd: HWND,
    message_id: u32,
    character_aliases: Vec<crate::char_alias::CharacterAlias>,
) -> Result<HookThread> {
    let (thread_tx, thread_rx) = std::sync::mpsc::channel();
    let hwnd_raw = hwnd.0 as isize;

    let join_handle = thread::spawn(move || {
        let _ = SENDER.set(sender);
        let _ = NOTIFY_HWND_RAW.set(hwnd_raw);
        let _ = NOTIFY_MESSAGE.set(message_id);
        let _ = HOOK_STATE.set(Mutex::new(HookState::default()));
        let _ = CHARACTER_ALIASES.set(Mutex::new(character_aliases));
        let _ = ALIAS_CAPTURE_TARGET.set(Mutex::new(None));

        let thread_id = unsafe { GetCurrentThreadId() };
        let _ = thread_tx.send(thread_id);

        let module = match unsafe { GetModuleHandleW(None) } {
            Ok(module) => module,
            Err(_) => return,
        };

        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };

        let Ok(hook) = hook else {
            return;
        };

        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            match result.0 {
                -1 | 0 => break,
                _ => unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                },
            }
        }

        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
    });

    let thread_id = thread_rx
        .recv()
        .context("failed to receive hook thread id")?;

    Ok(HookThread {
        thread_id,
        join_handle: Some(join_handle),
    })
}

unsafe extern "system" fn keyboard_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 && matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let keyboard = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = keyboard.vkCode as u16;

        if capture_alias_binding(vk_code) {
            return LRESULT(1);
        }

        if trigger_character_alias(vk_code) {
            return LRESULT(1);
        }

        match vk_code {
            code if code == VK_OEM_2.0 as u16 => handle_slash_event(),
            code if is_star_key(code) => handle_star_event(),
            code if code == VK_RETURN.0 as u16 => handle_enter_event(),
            code => handle_other_key_event(code),
        }
    }

    CallNextHookEx(None, ncode, wparam, lparam)
}

pub fn reload_character_aliases(aliases: Vec<crate::char_alias::CharacterAlias>) {
    let aliases_state = CHARACTER_ALIASES.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut current) = aliases_state.lock() {
        *current = aliases;
    }
}

pub fn begin_alias_capture(hwnd: HWND, message_id: u32) -> Result<()> {
    let target = ALIAS_CAPTURE_TARGET.get_or_init(|| Mutex::new(None));
    let mut target = target
        .lock()
        .map_err(|_| anyhow::anyhow!("alias capture target lock poisoned"))?;
    *target = Some(AliasCaptureTarget {
        hwnd_raw: hwnd.0 as isize,
        message_id,
    });
    Ok(())
}

pub fn cancel_alias_capture() {
    let Some(target) = ALIAS_CAPTURE_TARGET.get() else {
        return;
    };
    if let Ok(mut target) = target.lock() {
        *target = None;
    }
}

fn capture_alias_binding(vk_code: u16) -> bool {
    let Some(binding) = crate::char_alias::binding_from_current_keyboard(vk_code) else {
        return false;
    };

    let target = {
        let Some(target) = ALIAS_CAPTURE_TARGET.get() else {
            return false;
        };
        let Ok(mut target) = target.lock() else {
            return false;
        };
        target.take()
    };

    let Some(target) = target else {
        return false;
    };

    crate::char_alias::post_captured_binding(
        HWND(target.hwnd_raw as _),
        target.message_id,
        binding,
    );
    true
}

fn trigger_character_alias(vk_code: u16) -> bool {
    if crate::is_listening_paused() || crate::char_alias::is_modifier_vk(vk_code) {
        return false;
    }

    let alias = {
        let aliases = CHARACTER_ALIASES.get_or_init(|| Mutex::new(Vec::new()));
        let Ok(aliases) = aliases.lock() else {
            return false;
        };
        aliases
            .iter()
            .find(|alias| crate::char_alias::binding_matches(alias.trigger, vk_code))
            .cloned()
    };

    let Some(alias) = alias else {
        return false;
    };

    send_unicode_text(&alias.output);
    true
}

fn send_unicode_text(text: &str) {
    let mut inputs = Vec::new();
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, false));
        inputs.push(unicode_input(unit, true));
    }

    if !inputs.is_empty() {
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

fn unicode_input(unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn handle_slash_event() {
    let now = Instant::now();
    let Some(state) = HOOK_STATE.get() else {
        return;
    };

    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    let should_trigger = state
        .last_slash_at
        .map(|last| now.duration_since(last) <= SLASH_TRIGGER_WINDOW)
        .unwrap_or(false);

    state.last_slash_at = Some(now);
    state.doc_comment_stage = 1;
    state.last_doc_comment_at = Some(now);

    if should_trigger {
        send_event(HookEvent::SlashSequence);
    }
}

fn handle_star_event() {
    let now = Instant::now();
    let Some(state) = HOOK_STATE.get() else {
        return;
    };

    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    if is_doc_comment_active(&state, now) {
        if matches!(state.doc_comment_stage, 1 | 2) {
            state.doc_comment_stage += 1;
            state.last_doc_comment_at = Some(now);
            return;
        }
    }

    reset_doc_comment_state(&mut state);
}

fn handle_enter_event() {
    let now = Instant::now();
    let should_trigger_doc_comment = {
        let Some(state) = HOOK_STATE.get() else {
            return;
        };

        let mut state = match state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };

        let should_trigger = state.doc_comment_stage == 3 && is_doc_comment_active(&state, now);
        reset_doc_comment_state(&mut state);
        should_trigger
    };

    if should_trigger_doc_comment {
        send_event(HookEvent::DocCommentEnter);
    } else {
        send_event(HookEvent::EnterPressed);
    }
}

fn handle_other_key_event(code: u16) {
    if is_modifier_key(code) {
        return;
    }

    let Some(state) = HOOK_STATE.get() else {
        return;
    };

    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    reset_doc_comment_state(&mut state);
}

fn is_doc_comment_active(state: &HookState, now: Instant) -> bool {
    state
        .last_doc_comment_at
        .map(|last| now.duration_since(last) <= DOC_COMMENT_TRIGGER_WINDOW)
        .unwrap_or(false)
}

fn reset_doc_comment_state(state: &mut HookState) {
    state.doc_comment_stage = 0;
    state.last_doc_comment_at = None;
}

fn is_star_key(code: u16) -> bool {
    code == VK_MULTIPLY || (code == VK_8 && is_shift_pressed())
}

fn is_shift_pressed() -> bool {
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}

fn is_modifier_key(code: u16) -> bool {
    matches!(
        code,
        VK_SHIFT_CODE
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_CONTROL
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_MENU
            | VK_LMENU
            | VK_RMENU
    )
}

fn send_event(event: HookEvent) {
    let Some(sender) = SENDER.get() else {
        return;
    };

    if sender.send(event).is_err() {
        return;
    }

    let Some(hwnd_raw) = NOTIFY_HWND_RAW.get() else {
        return;
    };
    let Some(message_id) = NOTIFY_MESSAGE.get() else {
        return;
    };

    unsafe {
        let _ = PostMessageW(HWND(*hwnd_raw as _), *message_id, WPARAM(0), LPARAM(0));
    }
}
