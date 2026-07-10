use anyhow::Result;
use windows::Win32::Foundation::HWND;

use super::{win32, ImeMode, ImeStrategy, InputMethod};

pub(super) static STRATEGY: SogouStrategy = SogouStrategy;

pub(super) struct SogouStrategy;

impl ImeStrategy for SogouStrategy {
    fn input_method(&self) -> InputMethod {
        InputMethod::Sogou
    }

    fn name(&self) -> &'static str {
        "sogou_open_status"
    }

    fn current_mode(&self, hwnd: HWND) -> Result<ImeMode> {
        win32::current_open_status_mode(hwnd)
    }

    fn set_mode(&self, hwnd: HWND, desired: ImeMode) -> Result<bool> {
        win32::set_open_status_mode(hwnd, desired)
    }
}
