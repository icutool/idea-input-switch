#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod hook;
mod ime;
mod notify;
mod tray;
mod watcher;

use std::sync::{
    mpsc::{channel, Receiver, TryRecvError},
    Mutex, OnceLock,
};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, WINDOW_EX_STYLE,
    WM_APP, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const WINDOW_CLASS: PCWSTR = w!("IdeaInputSwitchHiddenWindow");
const WINDOW_TITLE: PCWSTR = w!("IdeaInputSwitch");
const WM_APP_PROCESS_EVENTS: u32 = WM_APP + 1;

static APP_CONTEXT: OnceLock<Mutex<AppContext>> = OnceLock::new();

struct AppContext {
    receiver: Receiver<hook::HookEvent>,
    paused: bool,
    autostart_enabled: bool,
    current_mode: ime::ImeMode,
    hwnd_raw: isize,
}

fn main() -> Result<()> {
    init_logging();

    let (sender, receiver) = channel();
    let autostart_enabled = autostart::is_enabled().unwrap_or(false);

    APP_CONTEXT
        .set(Mutex::new(AppContext {
            receiver,
            paused: false,
            autostart_enabled,
            current_mode: ime::ImeMode::English,
            hwnd_raw: 0,
        }))
        .map_err(|_| anyhow!("application context already initialized"))?;

    let hwnd = create_message_window().context("failed to create hidden window")?;
    {
        let mut context = context_lock();
        context.hwnd_raw = hwnd.0 as isize;
    }

    tray::add_icon(hwnd, ime::ImeMode::English, false).context("failed to add tray icon")?;

    let hook_thread = hook::start(sender, hwnd, WM_APP_PROCESS_EVENTS)
        .context("failed to start keyboard hook thread")?;

    info!("IdeaInputSwitch started");
    run_message_loop()?;

    hook_thread.stop();
    tray::remove_icon(hwnd);
    Ok(())
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .compact()
        .try_init();
}

fn context_lock() -> std::sync::MutexGuard<'static, AppContext> {
    APP_CONTEXT
        .get()
        .expect("application context must exist")
        .lock()
        .expect("application context lock poisoned")
}

fn hwnd_from_raw(raw: isize) -> HWND {
    HWND(raw as _)
}

fn create_message_window() -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        return Err(anyhow!("RegisterClassW failed: {:?}", unsafe {
            GetLastError()
        }));
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            WINDOW_TITLE,
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            None,
        )
    }
    .context("CreateWindowExW failed")?;

    Ok(hwnd)
}

fn run_message_loop() -> Result<()> {
    let mut message = MSG::default();
    loop {
        let has_message = unsafe { GetMessageW(&mut message, None, 0, 0) };
        match has_message.0 {
            -1 => return Err(anyhow!("GetMessageW failed")),
            0 => break,
            _ => unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            },
        }
    }

    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            handle_menu_command(hwnd, (wparam.0 & 0xffff) as usize);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        tray::WM_TRAYICON => {
            tray::handle_callback(hwnd, lparam);
            LRESULT(0)
        }
        WM_APP_PROCESS_EVENTS => {
            if let Err(error) = drain_hook_events() {
                warn!(?error, "failed to process keyboard event");
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn handle_menu_command(hwnd: HWND, command_id: usize) {
    let result = match command_id {
        tray::ID_STATUS => show_current_status(hwnd),
        tray::ID_TOGGLE_PAUSE => toggle_pause(hwnd),
        tray::ID_TOGGLE_AUTOSTART => toggle_autostart(hwnd),
        tray::ID_QUIT => unsafe { DestroyWindow(hwnd).context("failed to destroy hidden window") },
        _ => Ok(()),
    };

    if let Err(error) = result {
        warn!(?error, "menu command failed");
    }
}

fn show_current_status(hwnd: HWND) -> Result<()> {
    let context = context_lock();
    notify::show_status(
        hwnd,
        context.current_mode,
        context.paused,
        context.autostart_enabled,
    )
}

fn toggle_pause(hwnd: HWND) -> Result<()> {
    let (paused, current_mode) = {
        let mut context = context_lock();
        context.paused = !context.paused;
        (context.paused, context.current_mode)
    };

    tray::update_icon(hwnd, current_mode, paused)?;
    notify::show_pause_status(hwnd, paused)
}

fn toggle_autostart(hwnd: HWND) -> Result<()> {
    let enabled = {
        let mut context = context_lock();
        let next = !context.autostart_enabled;
        autostart::set_enabled(next)?;
        context.autostart_enabled = next;
        next
    };

    notify::show_autostart_status(hwnd, enabled)
}

fn drain_hook_events() -> Result<()> {
    loop {
        let event = {
            let context = context_lock();
            match context.receiver.try_recv() {
                Ok(event) => Some(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
            }
        };

        match event {
            Some(event) => process_hook_event(event)?,
            None => break,
        }
    }

    Ok(())
}

fn process_hook_event(event: hook::HookEvent) -> Result<()> {
    let paused = { context_lock().paused };
    if paused {
        return Ok(());
    }

    let hwnd = match watcher::foreground_window() {
        Some(hwnd) => hwnd,
        None => return Ok(()),
    };

    if !watcher::is_idea_window(hwnd)? {
        return Ok(());
    }

    let desired_mode = match event {
        hook::HookEvent::SlashSequence => ime::ImeMode::Chinese,
        hook::HookEvent::EnterPressed => ime::ImeMode::English,
    };

    let current_mode = ime::current_mode(hwnd)?;
    {
        let mut context = context_lock();
        context.current_mode = current_mode;
    }

    if current_mode == desired_mode {
        let (paused, tray_hwnd) = {
            let context = context_lock();
            (context.paused, hwnd_from_raw(context.hwnd_raw))
        };
        tray::update_icon(tray_hwnd, current_mode, paused)?;
        return Ok(());
    }

    if ime::set_mode(hwnd, desired_mode)? {
        let confirmed = ime::current_mode(hwnd)?;
        let tray_hwnd = {
            let mut context = context_lock();
            context.current_mode = confirmed;
            hwnd_from_raw(context.hwnd_raw)
        };

        tray::update_icon(tray_hwnd, confirmed, false)?;
        if confirmed == desired_mode {
            notify::show_mode_switch(tray_hwnd, desired_mode)?;
        }
    }

    Ok(())
}
