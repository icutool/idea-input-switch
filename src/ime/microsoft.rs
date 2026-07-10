use anyhow::Result;
use windows::Win32::Foundation::HWND;

use super::{win32, ImeMode, ImeStrategy, InputMethod};

pub(super) static STRATEGY: MicrosoftStrategy = MicrosoftStrategy;

pub(super) struct MicrosoftStrategy;

impl ImeStrategy for MicrosoftStrategy {
    fn input_method(&self) -> InputMethod {
        InputMethod::Microsoft
    }

    fn name(&self) -> &'static str {
        "microsoft_conversion_mode"
    }

    fn current_mode(&self, hwnd: HWND) -> Result<ImeMode> {
        win32::current_conversion_mode(hwnd)
    }

    fn set_mode(&self, hwnd: HWND, desired: ImeMode) -> Result<bool> {
        win32::set_conversion_mode(hwnd, desired)
    }
}
