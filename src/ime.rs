use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::{
    ImmGetContext, ImmGetDefaultIMEWnd, ImmGetOpenStatus, ImmReleaseContext, ImmSetOpenStatus,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_IME_CONTROL};

const IMC_GETOPENSTATUS: usize = 0x0005;
const IMC_SETOPENSTATUS: usize = 0x0006;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImeMode {
    #[default]
    Unknown,
    English,
    Chinese,
}

pub fn current_mode(hwnd: HWND) -> Result<ImeMode> {
    unsafe {
        if hwnd.0.is_null() {
            return Ok(ImeMode::Unknown);
        }

        let ime_window = ImmGetDefaultIMEWnd(hwnd);
        if !ime_window.0.is_null() {
            let status = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_GETOPENSTATUS),
                LPARAM(0),
            );

            return Ok(if status.0 == 0 {
                ImeMode::English
            } else {
                ImeMode::Chinese
            });
        }

        let input_context = ImmGetContext(hwnd);
        if !input_context.0.is_null() {
            let open = ImmGetOpenStatus(input_context).as_bool();
            let _ = ImmReleaseContext(hwnd, input_context);
            return Ok(if open {
                ImeMode::Chinese
            } else {
                ImeMode::English
            });
        }
    }

    Ok(ImeMode::Unknown)
}

pub fn set_mode(hwnd: HWND, desired: ImeMode) -> Result<bool> {
    let open = matches!(desired, ImeMode::Chinese);

    unsafe {
        let ime_window = ImmGetDefaultIMEWnd(hwnd);
        if !ime_window.0.is_null() {
            let _ = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_SETOPENSTATUS),
                LPARAM(open as isize),
            );
        }

        let input_context = ImmGetContext(hwnd);
        if !input_context.0.is_null() {
            let _ = ImmSetOpenStatus(input_context, open);
            let _ = ImmReleaseContext(hwnd, input_context);
        }
    }

    Ok(current_mode(hwnd)? == desired)
}
