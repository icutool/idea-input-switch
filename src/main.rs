#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod hook;
mod http_server;
mod ime;
mod logging;
mod notify;
mod tray;
mod watcher;

use std::sync::{
    mpsc::{channel, Receiver, TryRecvError},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tracing::{error, info, warn};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::Foundation::{GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, WINDOW_EX_STYLE,
    WM_APP, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const WINDOW_CLASS: PCWSTR = w!("IdeaInputSwitchHiddenWindow");
const WINDOW_TITLE: PCWSTR = w!("IdeaInputSwitch");
const WM_APP_PROCESS_EVENTS: u32 = WM_APP + 1;
const WM_APP_PROCESS_HTTP_REQUESTS: u32 = WM_APP + 2;
const MUTEX_NAME: PCWSTR = w!("Global\\IdeaInputSwitchSingleInstance");
const ENTER_LISTEN_WINDOW: Duration = Duration::from_secs(30);

static APP_CONTEXT: OnceLock<Mutex<AppContext>> = OnceLock::new();

struct AppContext {
    receiver: Receiver<hook::HookEvent>,
    http_receiver: Receiver<http_server::SwitchRequest>,
    paused: bool,
    autostart_enabled: bool,
    current_mode: ime::ImeMode,
    input_method: ime::InputMethod,
    enter_listener_until: Option<Instant>,
    hwnd_raw: isize,
}

fn main() -> Result<()> {
    logging::init();
    info!(pid = std::process::id(), "IdeaInputSwitch process starting");

    // ── 单例检测 ───────────────────────────────────────────
    let _mutex_guard = match try_acquire_single_instance() {
        SingleInstanceResult::AlreadyRunning(handle) => {
            // 已有实例运行，弹出提示后退出
            warn!("another IdeaInputSwitch instance is already running; exiting before starting HTTP server");
            show_already_running_notification(handle);
            return Ok(());
        }
        SingleInstanceResult::FirstInstance(handle) => {
            info!("single instance lock acquired");
            handle
        }
    };

    let (sender, receiver) = channel();
    let (http_sender, http_receiver) = channel();
    let autostart_enabled = autostart::is_enabled().unwrap_or(false);
    let input_method = config::load_input_method();
    info!(
        autostart_enabled,
        ?input_method,
        "application config loaded"
    );

    APP_CONTEXT
        .set(Mutex::new(AppContext {
            receiver,
            http_receiver,
            paused: false,
            autostart_enabled,
            current_mode: ime::ImeMode::English,
            input_method,
            enter_listener_until: None,
            hwnd_raw: 0,
        }))
        .map_err(|_| anyhow!("application context already initialized"))?;

    let hwnd = create_message_window().context("failed to create hidden window")?;
    {
        let mut context = context_lock();
        context.hwnd_raw = hwnd.0 as isize;
    }

    tray::add_icon(hwnd, ime::ImeMode::English, false).context("failed to add tray icon")?;

    // 启动成功，显示已启动提示
    let _ = notify::show_started(hwnd);

    let http_server = http_server::start(http_sender, hwnd, WM_APP_PROCESS_HTTP_REQUESTS)
        .context("failed to start HTTP server")?;
    let hook_thread = hook::start(sender, hwnd, WM_APP_PROCESS_EVENTS)
        .context("failed to start keyboard hook thread")?;

    info!("IdeaInputSwitch started");
    run_message_loop()?;
    info!("message loop exited; shutting down");

    http_server.stop();
    hook_thread.stop();
    tray::remove_icon(hwnd);
    info!("IdeaInputSwitch stopped");
    Ok(())
}

// ── 单例检测 ───────────────────────────────────────────────────────────────

enum SingleInstanceResult {
    FirstInstance(HANDLE),
    AlreadyRunning(HANDLE),
}

fn try_acquire_single_instance() -> SingleInstanceResult {
    unsafe {
        let handle = CreateMutexW(None, true, MUTEX_NAME).unwrap_or(HANDLE::default());

        let last_err = GetLastError();
        if last_err == ERROR_ALREADY_EXISTS {
            SingleInstanceResult::AlreadyRunning(handle)
        } else {
            SingleInstanceResult::FirstInstance(handle)
        }
    }
}

/// 已有实例运行时：创建临时窗口显示弹窗，等待其消失后退出
fn show_already_running_notification(mutex_handle: HANDLE) {
    if let Ok(hwnd) = create_message_window() {
        let (_, receiver) = std::sync::mpsc::channel();
        let (_, http_receiver) = std::sync::mpsc::channel();
        let _ = APP_CONTEXT.set(Mutex::new(AppContext {
            receiver,
            http_receiver,
            paused: false,
            autostart_enabled: false,
            current_mode: ime::ImeMode::English,
            input_method: ime::InputMethod::default(),
            enter_listener_until: None,
            hwnd_raw: hwnd.0 as isize,
        }));

        let _ = notify::show_already_running(hwnd);

        let start = std::time::Instant::now();
        let mut msg = MSG::default();
        loop {
            if start.elapsed().as_millis() > 2800 {
                break;
            }
            unsafe {
                let has = windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut msg,
                    None,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
                );
                if has.as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }

    if !mutex_handle.is_invalid() {
        unsafe {
            let _ = ReleaseMutex(mutex_handle);
        }
    }
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
                error!(?error, "failed to process keyboard event");
            }
            LRESULT(0)
        }
        WM_APP_PROCESS_HTTP_REQUESTS => {
            if let Err(error) = drain_http_requests() {
                error!(?error, "failed to process HTTP switch request");
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
        tray::ID_SELECT_SOGOU => select_input_method(hwnd, ime::InputMethod::Sogou),
        tray::ID_SELECT_MICROSOFT => select_input_method(hwnd, ime::InputMethod::Microsoft),
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
        context.input_method,
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

fn select_input_method(hwnd: HWND, input_method: ime::InputMethod) -> Result<()> {
    let (changed, current_mode, paused) = {
        let mut context = context_lock();
        let changed = context.input_method != input_method;
        if changed {
            config::save_input_method(input_method)?;
            context.input_method = input_method;
            context.current_mode = ime::ImeMode::Unknown;
        }

        (changed, context.current_mode, context.paused)
    };

    tray::update_icon(hwnd, current_mode, paused)?;

    if changed {
        notify::show_input_method_status(hwnd, input_method)
    } else {
        show_current_status(hwnd)
    }
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

fn drain_http_requests() -> Result<()> {
    loop {
        let request = {
            let context = context_lock();
            match context.http_receiver.try_recv() {
                Ok(request) => Some(request),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
            }
        };

        match request {
            Some(request) => process_http_switch_request(request),
            None => break,
        }
    }

    Ok(())
}

fn process_hook_event(event: hook::HookEvent) -> Result<()> {
    let (paused, input_method) = {
        let context = context_lock();
        (context.paused, context.input_method)
    };
    if paused {
        return Ok(());
    }

    if matches!(event, hook::HookEvent::EnterPressed) && !is_enter_listener_active() {
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
        hook::HookEvent::DocCommentEnter => {
            clear_enter_listener();
            ime::ImeMode::Chinese
        }
        hook::HookEvent::EnterPressed => {
            clear_enter_listener();
            ime::ImeMode::English
        }
    };
    info!(
        ?event,
        ?desired_mode,
        ?input_method,
        hwnd = ?hwnd.0,
        "keyboard rule matched"
    );

    let current_mode = ime::current_mode(hwnd, input_method)?;
    {
        let mut context = context_lock();
        context.current_mode = current_mode;
    }
    info!(
        ?event,
        ?current_mode,
        ?desired_mode,
        ?input_method,
        "current input mode read before keyboard switch"
    );

    if current_mode == desired_mode {
        if matches!(event, hook::HookEvent::SlashSequence) {
            arm_enter_listener();
        }

        let (paused, tray_hwnd) = {
            let context = context_lock();
            (context.paused, hwnd_from_raw(context.hwnd_raw))
        };
        tray::update_icon(tray_hwnd, current_mode, paused)?;
        info!(
            ?event,
            ?current_mode,
            "keyboard switch skipped because current mode already matches"
        );
        return Ok(());
    }

    if ime::set_mode(hwnd, desired_mode, input_method)? {
        let confirmed = ime::current_mode(hwnd, input_method)?;
        let tray_hwnd = {
            let mut context = context_lock();
            context.current_mode = confirmed;
            hwnd_from_raw(context.hwnd_raw)
        };

        tray::update_icon(tray_hwnd, confirmed, false)?;
        info!(
            ?event,
            ?desired_mode,
            ?confirmed,
            changed = confirmed == desired_mode,
            "keyboard input mode switch completed"
        );
        if confirmed == desired_mode {
            if matches!(event, hook::HookEvent::SlashSequence) {
                arm_enter_listener();
            }
            notify::show_mode_switch(tray_hwnd, desired_mode)?;
        }
    } else {
        warn!(
            ?event,
            ?desired_mode,
            ?input_method,
            "keyboard input mode switch command did not take effect"
        );
    }

    Ok(())
}

fn process_http_switch_request(request: http_server::SwitchRequest) {
    info!(?request.desired_mode, "processing HTTP input mode switch request");
    let response = match switch_foreground_mode_from_http(request.desired_mode) {
        Ok(response) => response,
        Err(error) => {
            error!(?error, ?request.desired_mode, "HTTP input mode switch failed");
            http_server::SwitchResponse::error(format!("输入法切换失败: {error}"))
        }
    };
    info!(
        success = response.success,
        changed = response.changed,
        ?response.mode,
        message = %response.message,
        "HTTP input mode switch response ready"
    );
    request.respond(response);
}

fn switch_foreground_mode_from_http(
    desired_mode: ime::ImeMode,
) -> Result<http_server::SwitchResponse> {
    let (tray_hwnd, input_method) = {
        let context = context_lock();
        (hwnd_from_raw(context.hwnd_raw), context.input_method)
    };
    let hwnd = match watcher::foreground_window() {
        Some(hwnd) => hwnd,
        None => {
            let _ = notify::show_http_switch_error(tray_hwnd, "未找到可切换的前台窗口");
            warn!(
                ?desired_mode,
                "HTTP switch failed because no foreground window was found"
            );
            return Ok(http_server::SwitchResponse::error("未找到可切换的前台窗口"));
        }
    };
    info!(?desired_mode, ?input_method, hwnd = ?hwnd.0, "HTTP switch target foreground window found");

    let current_mode = ime::current_mode(hwnd, input_method)?;
    let paused = {
        let mut context = context_lock();
        context.current_mode = current_mode;
        context.paused
    };
    info!(
        ?current_mode,
        ?desired_mode,
        ?input_method,
        paused,
        "current input mode read before HTTP switch"
    );

    if current_mode == desired_mode {
        let _ = tray::update_icon(tray_hwnd, current_mode, paused);
        let _ = notify::show_http_switch_result(tray_hwnd, desired_mode, false);
        let message = match desired_mode {
            ime::ImeMode::Chinese => "当前已经是中文输入",
            ime::ImeMode::English => "当前已经是英文输入",
            ime::ImeMode::Unknown => "当前输入法状态未知",
        };
        return Ok(http_server::SwitchResponse::success(
            desired_mode,
            false,
            message,
        ));
    }

    let changed_by_command = ime::set_mode(hwnd, desired_mode, input_method)?;
    let confirmed_mode = ime::current_mode(hwnd, input_method)?;
    {
        let mut context = context_lock();
        context.current_mode = confirmed_mode;
    }
    info!(
        ?desired_mode,
        ?confirmed_mode,
        ?input_method,
        changed_by_command,
        "HTTP input mode switch command completed"
    );

    let _ = tray::update_icon(tray_hwnd, confirmed_mode, paused);

    if confirmed_mode == desired_mode {
        let _ = notify::show_http_switch_result(tray_hwnd, desired_mode, true);
        let message = match desired_mode {
            ime::ImeMode::Chinese => "已切换到中文输入",
            ime::ImeMode::English => "已切换到英文输入",
            ime::ImeMode::Unknown => "输入法状态未知",
        };
        Ok(http_server::SwitchResponse::success(
            desired_mode,
            true,
            message,
        ))
    } else {
        let _ = notify::show_http_switch_error(tray_hwnd, "输入法切换未生效");
        warn!(
            ?desired_mode,
            ?confirmed_mode,
            ?input_method,
            "HTTP input mode switch did not take effect"
        );
        Ok(http_server::SwitchResponse::error("输入法切换未生效"))
    }
}

fn is_enter_listener_active() -> bool {
    let now = Instant::now();
    let mut context = context_lock();
    match context.enter_listener_until {
        Some(until) if now <= until => true,
        Some(_) => {
            context.enter_listener_until = None;
            false
        }
        None => false,
    }
}

fn arm_enter_listener() {
    let mut context = context_lock();
    context.enter_listener_until = Some(Instant::now() + ENTER_LISTEN_WINDOW);
}

fn clear_enter_listener() {
    let mut context = context_lock();
    context.enter_listener_until = None;
}
