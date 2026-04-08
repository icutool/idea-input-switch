# IdeaIME

IdeaIME 是一个面向 Windows 11 x64 的 IntelliJ IDEA 输入法自动切换工具。

它在后台常驻托盘，监听 IDEA 中的 `//` 与 `Enter`：
- 在 IDEA 中连续输入 `//` 时，自动切到中文输入法
- 在 IDEA 中按下 `Enter` 时，自动切回英文输入法
- 不吞键，按键依然正常传递给 IDEA
- 默认使用应用内自定义通知条，显示约 1 秒后自动消失
- 保留系统气泡通知逻辑作为备用实现，但默认不启用

## 项目架构

程序由 6 个核心模块组成：

```text
src/
├── main.rs        # 程序入口，隐藏消息窗口、消息循环、状态路由
├── hook.rs        # 低级键盘 Hook，识别 // 与 Enter
├── ime.rs         # IME 查询与切换
├── watcher.rs     # 前台窗口与 IDEA 进程判断
├── notify.rs      # 自定义通知条 + 备用系统通知接口
├── tray.rs        # 系统托盘、菜单、图标、备用气泡通知
└── autostart.rs   # 开机自启注册表读写
```

### 运行流程

```text
键盘事件
  -> hook.rs 捕获
  -> main.rs 收到事件
  -> watcher.rs 判断当前是否是 IDEA
  -> ime.rs 查询当前输入法状态
  -> ime.rs 执行切换
  -> tray.rs 更新托盘状态
  -> notify.rs 显示 1 秒自定义通知条
```

### 模块职责

- `main.rs`
  负责初始化隐藏窗口、托盘、全局状态、消息循环，以及把 Hook 事件路由到 IME 切换逻辑。
- `hook.rs`
  使用 `WH_KEYBOARD_LL` 监听全局按键。`//` 连击窗口为 300ms，Enter 单独触发英文切换。
- `ime.rs`
  使用 `WM_IME_CONTROL` 和 `ImmSetOpenStatus` 查询/设置输入法开关状态。
- `watcher.rs`
  通过前台窗口对应进程名判断是否是 `idea64.exe` 或 `idea.exe`。
- `notify.rs`
  默认显示应用内通知条，显示 1 秒自动消失；同时保留系统通知接口以便后续切回或调试。
- `tray.rs`
  负责托盘图标、右键菜单、状态提示，以及备用的系统气泡通知实现。
- `autostart.rs`
  通过 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 控制开机自启。

## 编译

在项目根目录执行：

```powershell
cargo build --release
```

生成文件位置：

```text
target/release/idea-ime.exe
```

## 使用方式

1. 启动 `idea-ime.exe`
2. 程序进入系统托盘
3. 打开 IntelliJ IDEA
4. 输入 `//` 时自动切中文
5. 按 `Enter` 时自动切英文

## 修改 exe 图标

仓库已经预留了 Windows 资源编译入口：`build.rs`。

你只需要：

1. 准备一个 `.ico` 文件
2. 命名为 `app.ico`
3. 放到 `resources/app.ico`
4. 重新执行：

```powershell
cargo build --release
```

重新编译后，生成的 `idea-ime.exe` 会带上新的图标。

### 图标要求建议

- 建议使用 `.ico` 格式，不要直接放 `.png`
- 最好包含多个尺寸：16x16、32x32、48x48、256x256
- 托盘图标建议准备高对比度版本，避免在浅色任务栏中不清楚

## 备注

- 当前默认通知为应用内弹出条，不再主动调用系统气泡通知
- 系统通知相关逻辑仍保留在代码中，便于回退或调试
- 当前 exe 图标嵌入只有在 `resources/app.ico` 存在时才会启用
