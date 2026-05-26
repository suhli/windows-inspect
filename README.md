# Windows Inspect

Windows 桌面网络连接与流量监控工具。实时查看当前 **ESTABLISHED** TCP 连接，按多种维度分组统计，并展示总上下行速率与历史曲线。

仅支持 **Windows 10 / 11**。

## 功能

- **连接采集**：枚举 IPv4 / IPv6 TCP 连接（`GetExtendedTcpTable`），过滤已建立连接
- **流量统计**：结合网卡计数器与 ETW 内核事件，计算总速率及各分组下载/上传速度
- **三种分组视图**
  - 按目标 IP + 端口
  - 按目标 IP + 进程
  - 按 IP + 端口 + 进程
- **实时图表**：总下载/上传速率折线图，每 2 秒自动刷新
- **表格排序**：点击表头排序，当前列显示 ▲ / ▼ 方向指示
- **悬浮模式**：无边框、窗口置顶、可调透明度（默认 50%），适合叠在屏幕一角常驻监控；再次点击恢复
- **深色标题栏**：与界面风格一致的系统标题栏配色

## 截图

将 `assets/icon.png` 作为应用图标；界面为深色主题（`#020617` 背景）。

## 环境要求

| 项目 | 说明 |
|------|------|
| 操作系统 | Windows 10 / 11（x64） |
| 权限 | **管理员**（清单 `requireAdministrator`；ETW 采集需要） |
| 构建 | [Rust](https://www.rustup.rs/) 稳定版 + MSVC 工具链 |

## 构建与运行

```powershell
# 克隆后进入项目目录
cd windows-inspect

# 调试运行
cargo run

# 发布构建
cargo build --release
# 可执行文件：target\release\windows-inspect.exe
```

首次构建会编译 Slint UI 并嵌入 `assets/icon.png` 为 exe 图标，耗时可能稍长。

> 若未以管理员身份启动，ETW 可能无法开启，表格中的分组速率会不可用，状态栏会提示相关信息。

## 使用说明

### 主界面

- **TCP 连接数 / 总下载速度 / 总上传速度**：顶部统计卡片
- **折线图**：蓝色为下载，绿色为上传
- **刷新**：立即重新采集
- **状态栏**：连接数量、分组统计、ETW 状态、上次更新时间

### 表格排序

1. 切换到对应标签页
2. 点击列标题进行排序
3. 再次点击同一列可在升序 / 降序间切换
4. 当前排序列显示 **▲**（升序）或 **▼**（降序）

### 悬浮模式

1. 点击 **悬浮**：隐藏标题栏、窗口置顶、默认 50% 透明度
2. 拖动 **透明度** 滑块（10%–100%）调节半透明程度
3. 在顶部标题文字区域按住鼠标可 **拖动窗口**
4. 点击 **退出悬浮** 恢复普通窗口

## 项目结构

```
windows-inspect/
├── assets/
│   └── icon.png              # 应用图标
├── ui/
│   └── app.slint             # Slint 界面定义
├── src/
│   ├── main.rs               # 入口、UI 回调、定时刷新
│   ├── net.rs                # TCP 连接枚举、分组、速率计算
│   ├── etw.rs                # ETW 内核网络事件采集
│   ├── speed.rs              # 速率追踪与图表路径
│   └── titlebar.rs           # 标题栏样式、透明度、悬浮拖动
├── build.rs                  # Slint 编译、exe 图标嵌入
├── windows-inspect.exe.manifest  # 管理员权限清单
└── Cargo.toml
```

## 技术栈

- **[Rust](https://www.rust-lang.org/)** — 主逻辑
- **[Slint](https://slint.dev/)** — 跨平台 UI（本项目中仅面向 Windows）
- **[windows](https://github.com/microsoft/windows-rs)** — Win32 API（TCP 表、DWM、分层窗口等）
- **[ferrisetw](https://github.com/1jeffc/ferrisetw)** — ETW 内核 `TCP/IP` 提供程序
- **[sysinfo](https://github.com/GuillaumeGomez/sysinfo)** — 进程名解析

## 工作原理（简述）

1. 周期性读取系统 TCP 连接表，筛选 `ESTABLISHED` 状态
2. 通过 `sysinfo` 将 PID 映射为进程名
3. ETW 订阅内核网络事件，按 `(远程 IP, 端口, PID)` 累计字节
4. 结合网卡层计数器与连接级统计，由 `SpeedTracker` 计算瞬时速率并绘制曲线
5. 按当前标签与排序列对分组结果排序后刷新 UI

## 常见问题

**Q: 为什么必须用管理员运行？**  
A: 应用清单要求提升权限；ETW 内核跟踪在普通用户下常无法启动。

**Q: 分组表格里速度为 0？**  
A: 确认 ETW 已启动（状态栏无「ETW 未启动」提示）。若仍异常，可点击刷新或重启程序。

**Q: 悬浮模式透明度不生效？**  
A: 可再次切换「悬浮」/「退出悬浮」，或拖动透明度滑块触发更新。

## 许可证

未指定开源许可证；使用前请与仓库维护者确认。
