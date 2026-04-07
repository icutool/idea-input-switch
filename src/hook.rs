use std::{
    sync::{mpsc::Sender, Mutex, OnceLock},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_OEM_2, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

const SLASH_TRIGGER_WINDOW: Duration = Duration::from_millis(300);

static SENDER: OnceLock<Sender<HookEvent>> = OnceLock::new();
static NOTIFY_HWND_RAW: OnceLock<isize> = OnceLock::new();
static NOTIFY_MESSAGE: OnceLock<u32> = OnceLock::new();
static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum HookEvent {
    SlashSequence,
    EnterPressed,
}

#[derive(Default)]
struct HookState {
    last_slash_at: Option<Instant>,
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

pub fn start(sender: Sender<HookEvent>, hwnd: HWND, message_id: u32) -> Result<HookThread> {
    let (thread_tx, thread_rx) = std::sync::mpsc::channel();
    let hwnd_raw = hwnd.0 as isize;

    let join_handle = thread::spawn(move || {
        let _ = SENDER.set(sender);
        let _ = NOTIFY_HWND_RAW.set(hwnd_raw);
        let _ = NOTIFY_MESSAGE.set(message_id);
        let _ = HOOK_STATE.set(Mutex::new(HookState::default()));

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
        match keyboard.vkCode as u16 {
            code if code == VK_OEM_2.0 as u16 => handle_slash_event(),
            code if code == VK_RETURN.0 as u16 => send_event(HookEvent::EnterPressed),
            _ => {}
        }
    }

    CallNextHookEx(None, ncode, wparam, lparam)
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

    if should_trigger {
        send_event(HookEvent::SlashSequence);
    }
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
