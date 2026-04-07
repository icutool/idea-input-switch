# IdeaIME — IntelliJ IDEA 智能输入法切换工具

> 需求说明 · 技术设计 · 开发文档  
> v1.0 | 语言: Rust | 平台: Windows
cargo build --release

---

## 1. 项目背景与目标

开发者在 IntelliJ IDEA 中编码时，常需切换输入法写中文注释，写完注释按回车后忘记切回英文输入法，导致输入的代码字符全部变成乱码，只能全部删除重来。这一痛点每天都在反复发生，严重打断编码节奏。

**IdeaIME** 是一款运行于 Windows 系统托盘的轻量 Rust 程序，专门解决上述问题。核心目标：

- 检测用户正在使用 IntelliJ IDEA
- 监听特定键序（`//`）自动切换中文输入法，方便写注释
- 监听回车键，自动切换回英文输入法
- 通过系统通知告知用户当前输入法状态
- 零感知运行，不影响 IDE 正常使用

---

## 2. 功能需求说明

### 2.1 核心触发逻辑

| 当前状态 | 触发条件 | 执行动作 | 通知内容 |
|---|---|---|---|
| IDEA 激活，英文输入法 | 连续输入 `//` | 切换到中文输入法 | 已切换：中文 |
| IDEA 激活，中文输入法 | 按下 Enter | 切换到英文输入法 | 已切换：英文 |
| IDEA 未激活 | 任意键 | 不处理，透传按键 | 无 |

### 2.2 功能细节要求

- **`//` 检测**：两次斜杠必须在 300ms 内连续输入才视为触发，避免误触
- **透传按键**：`//` 触发后，这两个字符照常透传给 IDEA，不吞掉按键
- **Enter 触发**：仅在当前输入法为中文状态时执行切换，英文状态下 Enter 正常透传
- **输入法切换**：使用 Windows IME API（`WM_IME_CONTROL`）直接操作搜狗等 IME
- **窗口检测**：通过 `GetForegroundWindow` + 进程名判断是否是 IDEA 窗口

### 2.3 系统通知

- 输入法切换成功后，弹出 Windows Toast 通知（右下角）
- 通知显示：图标 + 标题（IdeaIME）+ 正文（已切换到中文 / 已切换到英文）
- 通知自动消失，3 秒后淡出，不需要用户操作
- 同一方向的切换不重复通知（已经是英文时按 Enter 不弹通知）

### 2.4 系统托盘

- 程序启动后常驻系统托盘，不占用任务栏
- 托盘图标根据当前状态变化（中文显示红色图标，英文显示蓝色图标）
- 右键菜单：显示当前状态 / 暂停监听 / 退出
- 支持开机自启（写入注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`）

---

## 3. 非功能需求

| 指标 | 要求 |
|---|---|
| 内存占用 | < 10 MB（Rust 二进制，无运行时） |
| CPU 占用 | < 0.1% 空闲时（事件驱动，非轮询） |
| 启动时间 | < 200ms |
| 按键延迟 | 全局 Hook 处理 < 1ms，用户无感知 |
| 兼容性 | Windows 10 / 11，搜狗输入法 / 微软拼音 |
| 安全性 | 不记录按键内容，仅做结构匹配 |

---

## 4. 技术架构

### 4.1 为什么选择 Rust

- 极低的内存与 CPU 开销，适合常驻后台的系统工具
- 直接调用 Windows API（FFI），无需额外运行时
- 编译为单一可执行文件，分发简单
- `windows-rs` crate 提供完整的 Win32 API 绑定，无需手写 unsafe 声明

### 4.2 系统架构

程序由五个模块构成，通过 channel 通信：

```
┌─────────────────────────────────────────────────────┐
│                    main.rs                          │
│            初始化 · 消息循环 · channel 路由           │
└──────────┬──────────────────────────────────────────┘
           │  mpsc channel
    ┌──────▼───────┐   ┌──────────────┐   ┌──────────────┐
    │  hook.rs     │   │  watcher.rs  │   │  tray.rs     │
    │ 键盘 Hook    │   │ 窗口/进程检测 │   │ 系统托盘菜单  │
    │ WH_KEYBOARD  │   │ IDEA 判断    │   │ 右键菜单     │
    └──────┬───────┘   └──────────────┘   └──────────────┘
           │
    ┌──────▼───────┐   ┌──────────────┐
    │   ime.rs     │   │  notify.rs   │
    │ IME 状态查询  │──▶│ Toast 通知   │
    │ IME 状态切换  │   │ 右下角弹窗   │
    └──────────────┘   └──────────────┘
```

| 模块 | 职责 | 关键 Win32 API |
|---|---|---|
| `hook.rs` | 全局监听键盘事件，识别 `//` 和 Enter | `SetWindowsHookExW` |
| `ime.rs` | 获取 / 设置 IME 状态 | `ImmGetDefaultIMEWnd`, `SendMessageW` |
| `watcher.rs` | 检测前台窗口是否为 IDEA 进程 | `GetForegroundWindow`, `QueryFullProcessImageName` |
| `notify.rs` | 发送 Windows Toast 通知 | WinRT `ToastNotification` |
| `tray.rs` | 系统托盘图标与右键菜单 | `tray-icon` crate |

### 4.3 关键依赖 Crates

| Crate | 版本 | 用途 |
|---|---|---|
| `windows` | 0.58 | Win32 API 全套绑定（IME / Hook / Toast） |
| `tray-icon` | 0.14 | 跨平台系统托盘（Windows 使用原生 API） |
| `tokio` | 1.x | async 运行时，channel 通信 |
| `anyhow` | 1.x | 统一错误处理 |
| `tracing` | 0.1 | 结构化日志，便于调试 |
| `winreg` | 0.52 | 开机自启注册表读写 |

---

## 5. 核心实现要点

### 5.1 全局键盘 Hook

使用 Windows 低级键盘钩子（`WH_KEYBOARD_LL`）在系统消息循环中捕获所有按键。

**关键要点：**

- Hook 回调必须在有消息循环（`GetMessage` / `DispatchMessage`）的线程上注册
- Hook 处理函数要尽可能快，耗时操作通过 channel 发送到主线程处理
- `//` 检测：维护一个时间戳，两次 `VK_OEM_2`（`/`）之间间隔 < 300ms 则触发
- Enter 检测：`VK_RETURN` 按下时查询当前 IME 状态，仅中文时切换
- 按键需透传：调用 `CallNextHookEx` 让按键继续传递给 IDEA

```rust
// 示意：Hook 回调核心逻辑
unsafe extern "system" fn keyboard_proc(
    ncode: i32, wparam: WPARAM, lparam: LPARAM
) -> LRESULT {
    if ncode >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        match kb.vkCode as u16 {
            VK_OEM_2 => handle_slash(),   // '/' 键
            VK_RETURN => handle_enter(),  // Enter 键
            _ => {}
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam) // 必须透传
}
```

### 5.2 IME 状态控制

复用提供的 Go 代码思路，Rust 实现完全相同的 Win32 调用：

```rust
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_IME_CONTROL: u32 = 0x283;
const IMC_GETOPENSTATUS: usize = 0x0005;
const IMC_SETOPENSTATUS: usize = 0x0006;

unsafe fn get_ime_status(hwnd: HWND) -> bool {
    let hime = ImmGetDefaultIMEWnd(hwnd);
    let status = SendMessageW(hime, WM_IME_CONTROL, 
                              WPARAM(IMC_GETOPENSTATUS), LPARAM(0));
    status.0 == 1  // 1 = 中文，0 = 英文
}

unsafe fn set_ime_status(hwnd: HWND, chinese: bool) {
    let hime = ImmGetDefaultIMEWnd(hwnd);
    let value = if chinese { 1 } else { 0 };
    SendMessageW(hime, WM_IME_CONTROL,
                 WPARAM(IMC_SETOPENSTATUS), LPARAM(value));
}
```

> 注意：此方式对搜狗输入法有效，对部分版本微软拼音可能需要 `ImmSetOpenStatus` 作为备用方案。

### 5.3 IDEA 窗口检测

通过进程名判断比窗口标题更可靠（IDEA 窗口标题随项目变化）：

```rust
unsafe fn is_idea_active() -> bool {
    let hwnd = GetForegroundWindow();
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
    let mut buf = [0u16; 260];
    let mut size = 260u32;
    QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, 
                               PWSTR(buf.as_mut_ptr()), &mut size);
    
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    path.to_lowercase().contains("idea64.exe")
}
```

检测频率：仅在按键事件触发时检测，不做定时轮询，避免 CPU 浪费。

### 5.4 Toast 通知

两种方案，建议优先尝试方案 A：

**方案 A：WinRT Toast（效果更好）**
```rust
// 使用 windows crate 的 WinRT API
use windows::UI::Notifications::*;
use windows::Data::Xml::Dom::*;

fn send_toast(message: &str) -> anyhow::Result<()> {
    let xml = XmlDocument::new()?;
    xml.LoadXml(&format!(
        "<toast><visual><binding template='ToastGeneric'>\
         <text>IdeaIME</text><text>{}</text>\
         </binding></visual></toast>", message
    ))?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(
        &"IdeaIME".into()
    )?;
    notifier.Show(&ToastNotification::CreateToastNotification(&xml)?)?;
    Ok(())
}
```

**方案 B：Shell 气泡通知（兼容性更好，实现更简单）**

通过 `Shell_NotifyIcon` + `NIF_INFO` 在托盘图标上弹出气泡提示，无需注册 AppUserModelId。

---

## 6. 项目目录结构

```
idea-ime/
├── Cargo.toml
├── build.rs                  # 嵌入 Windows 资源（图标、版本信息）
├── resources/
│   ├── icon_cn.ico           # 中文状态托盘图标（红色）
│   ├── icon_en.ico           # 英文状态托盘图标（蓝色）
│   └── manifest.xml          # 请求必要权限
└── src/
    ├── main.rs               # 入口，初始化各模块，启动消息循环
    ├── hook.rs               # 全局键盘 Hook 注册与回调逻辑
    ├── ime.rs                # IME 状态查询与切换（Win32 API）
    ├── watcher.rs            # 前台窗口 / 进程检测
    ├── notify.rs             # Toast / 气泡通知封装
    ├── tray.rs               # 系统托盘图标与右键菜单
    └── autostart.rs          # 开机自启注册表操作
```

---

## 7. 开发计划

| 阶段 | 目标 | 主要任务 | 预计时间 |
|---|---|---|---|
| P0 | 核心功能 | IME 切换 + 键盘 Hook + IDEA 窗口检测 | 2 ~ 3 天 |
| P1 | 用户体验 | Toast 通知 + 系统托盘 + 暂停功能 | 1 ~ 2 天 |
| P2 | 完善 | 开机自启 + 托盘图标状态 + 日志 | 1 天 |
| P3 | 测试发布 | 兼容性测试（搜狗 / 微软拼音）+ 打包 + README | 1 天 |

---

## 8. 风险与注意事项

### 8.1 已知风险

- **搜狗输入法版本兼容性**：`WM_IME_CONTROL` 消息在不同版本搜狗下行为有差异，建议在多个版本测试，准备 `ImmSetOpenStatus` 作为回退方案
- **Windows Defender 告警**：全局键盘 Hook 可能触发杀软警告，发布时建议代码签名或在 README 中提示用户添加白名单
- **Hook 超时**：系统对 Hook 回调有时间限制（默认 300ms），超时会被自动移除，回调函数必须保持轻量，所有耗时逻辑通过 channel 异步处理

### 8.2 开发建议

- 先用已有的 Go 代码验证 IME 切换逻辑是否对当前搜狗版本生效，再迁移到 Rust
- 键盘 Hook 调试期间，建议在虚拟机或备用机上开发，避免 Hook 卡死导致主机输入失灵
- 使用 `tracing` + 文件日志（写入 `%APPDATA%\IdeaIME\`），便于在托盘模式（无控制台）下排查问题
- `Cargo.toml` 加入以下配置最小化二进制体积：

```toml
[profile.release]
opt-level = 3
strip = true
lto = true
codegen-units = 1
```

---

## 附录：参考资料

- [Windows IME API 文档](https://docs.microsoft.com/windows/win32/intl/input-method-manager)
- [windows-rs crate](https://github.com/microsoft/windows-rs)
- [tray-icon crate](https://github.com/tauri-apps/tray-icon)
- [WH_KEYBOARD_LL Hook 文档](https://docs.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc)
- [Toast 通知 WinRT](https://docs.microsoft.com/windows/apps/design/shell/tiles-and-notifications)
