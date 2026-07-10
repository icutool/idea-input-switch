mod microsoft;
mod sogou;
mod win32;

use anyhow::Result;
use tracing::info;
use windows::Win32::Foundation::HWND;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputMethod {
    #[default]
    Sogou,
    Microsoft,
}

impl InputMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sogou => "搜狗输入法",
            Self::Microsoft => "微软拼音",
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Sogou => "sogou",
            Self::Microsoft => "microsoft",
        }
    }

    pub fn from_config_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sogou" => Some(Self::Sogou),
            "microsoft" | "ms" | "microsoft_pinyin" => Some(Self::Microsoft),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImeMode {
    #[default]
    Unknown,
    English,
    Chinese,
}

pub trait ImeStrategy: Sync {
    fn input_method(&self) -> InputMethod;
    fn name(&self) -> &'static str;
    fn current_mode(&self, hwnd: HWND) -> Result<ImeMode>;
    fn set_mode(&self, hwnd: HWND, desired: ImeMode) -> Result<bool>;
}

pub fn current_mode(hwnd: HWND, input_method: InputMethod) -> Result<ImeMode> {
    let strategy = strategy_for(input_method);
    let mode = strategy.current_mode(hwnd)?;
    info!(
        ?mode,
        ?input_method,
        strategy_input_method = ?strategy.input_method(),
        strategy = strategy.name(),
        hwnd = ?hwnd.0,
        "read current input mode"
    );
    Ok(mode)
}

pub fn set_mode(hwnd: HWND, desired: ImeMode, input_method: InputMethod) -> Result<bool> {
    let strategy = strategy_for(input_method);
    info!(
        ?desired,
        ?input_method,
        strategy_input_method = ?strategy.input_method(),
        strategy = strategy.name(),
        hwnd = ?hwnd.0,
        "setting input mode"
    );
    let changed = strategy.set_mode(hwnd, desired)?;
    info!(
        ?desired,
        ?input_method,
        strategy_input_method = ?strategy.input_method(),
        strategy = strategy.name(),
        hwnd = ?hwnd.0,
        changed,
        "input mode set command result"
    );
    Ok(changed)
}

fn strategy_for(input_method: InputMethod) -> &'static dyn ImeStrategy {
    match input_method {
        InputMethod::Sogou => &sogou::STRATEGY,
        InputMethod::Microsoft => &microsoft::STRATEGY,
    }
}
