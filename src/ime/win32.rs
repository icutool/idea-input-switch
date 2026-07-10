use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::{
    ImmGetContext, ImmGetConversionStatus, ImmGetDefaultIMEWnd, ImmGetOpenStatus,
    ImmReleaseContext, ImmSetConversionStatus, ImmSetOpenStatus, IME_CONVERSION_MODE,
    IME_SENTENCE_MODE,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_IME_CONTROL};

use super::ImeMode;

const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_SETCONVERSIONMODE: usize = 0x0002;
const IMC_GETOPENSTATUS: usize = 0x0005;
const IMC_SETOPENSTATUS: usize = 0x0006;
const IME_CMODE_NATIVE: u32 = 0x0001;

pub(crate) fn current_open_status_mode(hwnd: HWND) -> Result<ImeMode> {
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

pub(crate) fn set_open_status_mode(hwnd: HWND, desired: ImeMode) -> Result<bool> {
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

    Ok(current_open_status_mode(hwnd)? == desired)
}

pub(crate) fn current_conversion_mode(hwnd: HWND) -> Result<ImeMode> {
    unsafe {
        if hwnd.0.is_null() {
            return Ok(ImeMode::Unknown);
        }

        let input_context = ImmGetContext(hwnd);
        if !input_context.0.is_null() {
            let mut conversion = IME_CONVERSION_MODE(0);
            let mut sentence = IME_SENTENCE_MODE(0);
            let success =
                ImmGetConversionStatus(input_context, Some(&mut conversion), Some(&mut sentence))
                    .as_bool();
            let _ = ImmReleaseContext(hwnd, input_context);

            if success {
                return Ok(mode_from_conversion(conversion.0));
            }
        }

        let ime_window = ImmGetDefaultIMEWnd(hwnd);
        if !ime_window.0.is_null() {
            let status = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_GETCONVERSIONMODE),
                LPARAM(0),
            );

            return Ok(mode_from_conversion(status.0 as u32));
        }
    }

    current_open_status_mode(hwnd)
}

pub(crate) fn set_conversion_mode(hwnd: HWND, desired: ImeMode) -> Result<bool> {
    if matches!(desired, ImeMode::Unknown) {
        return Ok(false);
    }

    unsafe {
        let ime_window = ImmGetDefaultIMEWnd(hwnd);
        if !ime_window.0.is_null() {
            let _ = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_SETOPENSTATUS),
                LPARAM(1),
            );

            let current = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_GETCONVERSIONMODE),
                LPARAM(0),
            );
            let conversion = conversion_for_mode(current.0 as u32, desired);

            let _ = SendMessageW(
                ime_window,
                WM_IME_CONTROL,
                WPARAM(IMC_SETCONVERSIONMODE),
                LPARAM(conversion as isize),
            );
        }

        let input_context = ImmGetContext(hwnd);
        if !input_context.0.is_null() {
            let mut conversion = IME_CONVERSION_MODE(0);
            let mut sentence = IME_SENTENCE_MODE(0);
            let _ =
                ImmGetConversionStatus(input_context, Some(&mut conversion), Some(&mut sentence));
            let conversion = conversion_for_mode(conversion.0, desired);

            let _ = ImmSetOpenStatus(input_context, true);
            let _ =
                ImmSetConversionStatus(input_context, IME_CONVERSION_MODE(conversion), sentence);
            let _ = ImmReleaseContext(hwnd, input_context);
        }
    }

    Ok(current_conversion_mode(hwnd)? == desired)
}

fn conversion_for_mode(conversion: u32, desired: ImeMode) -> u32 {
    match desired {
        ImeMode::Chinese => conversion | IME_CMODE_NATIVE,
        ImeMode::English => conversion & !IME_CMODE_NATIVE,
        ImeMode::Unknown => conversion,
    }
}

fn mode_from_conversion(conversion: u32) -> ImeMode {
    if conversion & IME_CMODE_NATIVE == 0 {
        ImeMode::English
    } else {
        ImeMode::Chinese
    }
}
