use anyhow::Result;
use windows::Win32::Foundation::HWND;

use crate::{ime::ImeMode, tray};

pub fn show_mode_switch(hwnd: HWND, mode: ImeMode) -> Result<()> {
    let message = match mode {
        ImeMode::Chinese => "已切换到中文输入",
        ImeMode::English => "已切换到英文输入",
        ImeMode::Unknown => "输入法状态未知",
    };

    tray::show_balloon(hwnd, "IdeaIME", message)
}

pub fn show_pause_status(hwnd: HWND, paused: bool) -> Result<()> {
    let message = if paused {
        "已暂停 IDEA 输入法监听"
    } else {
        "已恢复 IDEA 输入法监听"
    };

    tray::show_balloon(hwnd, "IdeaIME", message)
}

pub fn show_autostart_status(hwnd: HWND, enabled: bool) -> Result<()> {
    let message = if enabled {
        "已开启开机自启"
    } else {
        "已关闭开机自启"
    };

    tray::show_balloon(hwnd, "IdeaIME", message)
}

pub fn show_status(hwnd: HWND, mode: ImeMode, paused: bool, autostart_enabled: bool) -> Result<()> {
    let mode_label = match mode {
        ImeMode::Chinese => "中文",
        ImeMode::English => "英文",
        ImeMode::Unknown => "未知",
    };
    let pause_label = if paused { "已暂停" } else { "监听中" };
    let autostart_label = if autostart_enabled {
        "开机自启：开"
    } else {
        "开机自启：关"
    };

    let message = format!("{pause_label} | 当前输入法：{mode_label} | {autostart_label}");
    tray::show_balloon(hwnd, "IdeaIME 状态", &message)
}
