<div align="center">

[English](README.md) | [简体中文](README.zh-CN.md)

<img src="site/public/ferryx-icon.png" alt="Ferryx" width="120" height="120" />

# Ferryx

### 为编码智能体打造的原生终端，支持 macOS、Windows 和 Linux。

关闭应用后 Claude Code、Codex 以及任意 CLI 智能体继续运行，远程 Linux 机器无需 SSH 即可连接，每个标签页都会显示哪个智能体在等你。
原生 Rust 与 Tauri v2，libghostty 终端核心，不使用 Electron。通道监督者已在 `main` 分支，将随下一个版本发布。

[**下载**](https://github.com/Indosaram/ferryx/releases/latest) &nbsp;·&nbsp;
[**网站**](https://ferryx.dev/) &nbsp;·&nbsp;
[**文档**](https://ferryx.dev/docs/introduction/) &nbsp;·&nbsp;
[**Discord**](https://discord.gg/Z2hBkQEHUG)

<br />

<img src="site/public/demo.png" alt="Ferryx 在分屏终端旁的嵌入式浏览器标签中运行智能体" width="960" />

<sub>智能体在原生终端窗格中工作，旁边分屏显示嵌入式浏览器标签。</sub>

</div>

---

## 下载

| 平台 | 软件包 | |
| :--- | :--- | :--- |
| **macOS** | 通用 DMG（Apple Silicon 和 Intel） | [`.dmg`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_universal.dmg) |
| **Windows** | Microsoft Store（首选）/ 安装程序（备选） | [Store](https://apps.microsoft.com/detail/9NLHQL5JNLM4) / [`.exe`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_x64-setup.exe) |
| **Linux** | 便携式 AppImage（x64） | [`.AppImage`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.AppImage) |
| **Linux** | Debian / Ubuntu 软件包（x64） | [`.deb`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.deb) |
| **Linux（无头）** | 服务器 / VPS PTY 守护进程和 CLI（仅 x64；arm64 请[从源码构建](CONTRIBUTING.md#build-prerequisites)） | [`install.sh`](https://relay.ferryx.dev/install.sh)（`curl -fsSL https://relay.ferryx.dev/install.sh \| bash`） |

macOS、Windows 和 Linux 的下载链接会指向最新版本。进行本地发行版组装时，会将无头版 `ferryx-cli` 放在 Linux AppImage 和 `.deb` 旁边（文件名为
`ferryx-cli-linux-amd64`）；如果存在通用 macOS CLI，也会将其作为
`ferryx-cli-darwin-universal` 一同放置。因此，上面的一行安装命令在发行版包含当前构建产物时会解析到该产物，否则会安全失败。Linux arm64 没有预构建的 `ferryx-cli`；安装程序会以退出码 2 结束并提示从源码构建。安装程序会根据发行版的 `SHA256SUMS.txt` 校验下载的 `ferryx-cli`，不匹配时拒绝安装。其他下载请使用与二进制文件一同发布的校验和验证：

```bash
sha256sum -c SHA256SUMS.txt
```

## 速度、隔离与完全掌控

每一层都围绕低延迟和智能体自主协作进行设计。

### 原生 Ghostty 与 wgpu 引擎

桌面终端窗格使用 libghostty-vt 解析输出，并通过 Ferryx 自有的 GPU 加速 wgpu
管线渲染，提供清晰的字体栅格化效果和低延迟吞吐。

- libghostty 终端核心
- wgpu GPU 管线
- 离屏 WGPU 渲染基准（800x480 px，50 帧）：Apple M4 Max 上 p50 3.10 ms、p95 4.34 ms

### 多智能体工作区

在彼此隔离的分屏窗格中并行运行 AI 编码智能体（OmO、Claude Code、Codex、Antigravity、OpenCode、
pi、omp、GJC、GitHub Copilot CLI、Cursor CLI、Grok、Kimi、Cline、Aider、Crush、Droid），
并查看实时状态指示。

- 每个智能体使用独立 worktree
- 实时状态指示
- 可直接从标签栏启动

### 灵活的分屏平铺

支持任意垂直和水平分屏，提供指针拖动调整大小和流畅的布局过渡。

- 垂直和水平分屏
- 指针拖动调整大小
- 可将标签拖到任意窗格

### 移动端网页配对

登录 Ferryx 账户完成身份验证后远程访问。将终端输出流式传输到无需依赖的
DOM 终端网格，并随时随地操控智能体。

- 通过邮箱魔法链接登录账户
- 流式终端网格
- 用手机操控智能体

### 远程 Linux 机器配对（无需 SSH）

将无头 Linux 服务器、VPS 和云实例配对为拥有项目的机器。
Ferryx 使用仅向外发起的中继隧道，因此位于 NAT 或防火墙之后，或没有公网 IP / 开放 SSH 端口的机器也能顺畅连接。
当两台机器可以通过 UDP 互通时，已配对的会话还会在不中断终端的情况下切换到直连、端到端加密的 QUIC 路径；该路径中断时会回退到中继。中继始终是基础通道，直连路径是机会性的。
如需使用自己的中继服务而不是默认服务，请遵循
[自托管中继指南](https://ferryx.dev/docs/self-hosted-relay/)，了解 TLS、邮件投递、机器注册，以及独立的桌面账户和网关设置。

- 无头 `ferryx-cli` 不需要显示服务器，也不链接 GTK、WebKitGTK 或 JavaScriptCore；在 Linux 上只需要 glibc（`libc`、`libm`、`libgcc_s`）
- 仅向外发起的加密中继隧道：无需入站 SSH、开放端口或公网 IP
- 原生终端分屏和远程 Git worktree 可直接在该机器上运行

```bash
# 1. 在远程机器上通过一行命令安装（Linux x64、macOS）。安装 ferryx-cli 后会运行
#    `ferryx-cli up`：它会输出账户批准链接，并安装一个让守护进程持续运行的服务。
#    只想安装二进制文件时请传入 --no-up：
#    curl -fsSL https://relay.ferryx.dev/install.sh | sh -s -- --no-up
curl -fsSL https://relay.ferryx.dev/install.sh | bash

# 手动方式（使用了 --no-up，或安装时没有连接终端）：
# 2. 在专用服务或终端中启动无头 PTY 守护进程；
#    此前台进程运行时，请使用第二个终端执行第 3 步。
ferryx-cli --daemon

# 3. 通过电子邮件 magic link 将机器关联到账户（无需开放端口或在 GUI 中复制 PIN）
ferryx-cli account login --email you@example.com --origin https://your-account-service.example
# 在电子邮件中打开授权链接；命令会等待批准。
# 设置了 FERRYX_ACCOUNT_ORIGIN 或之前已保存账户 origin 时，可省略 --origin。
```

### 零 Electron 开销

使用 Tauri v2 和平台原生 WebKit 或 WebView2 引擎，并搭配无头
Rust PTY 守护进程。核心支持 macOS、Windows 和 Linux；Dock 徽章和透明原生表面窗口等部分操作系统集成仍以 macOS 为主，Windows 上的 emoji 不显示颜色。

- Tauri v2 外壳
- 无头 Rust PTY 守护进程
- macOS、Windows、Linux

### 可靠的持久化

工作区状态会自动生成快照，GUI 可以重新连接到正在运行的守护进程。
关闭 GUI 后，由守护进程拥有的进程仍会继续运行；停止守护进程或重启主机仍可能终止正在运行的 PTY。

- 布局快照
- 守护进程在 GUI 关闭后仍会运行
- 重新连接时重放输出

## Ferryx 适合你吗？

如果你符合以下情况，Ferryx 会很合适：

- 在同一个仓库上运行多个 CLI 编码智能体（例如 Claude Code 和 Codex），并希望它们共用一套 worktree 约定；
- 希望关闭窗口后智能体会话继续运行，回来时重放错过的输出；
- 想在手机上查看或操控长时间运行的智能体任务；
- 是个人，或规模足够小、可以免费使用的公司（参见[许可证 FAQ](https://ferryx.dev/docs/license/)）。

如果你符合以下情况，Ferryx 可能暂时不适合你：

- 只使用单一厂商的智能体。`claude --worktree` 或 Codex 内置的 worktree 已经能隔离该厂商的会话，需要安装的东西更少；
- 需要 SSO、集中管理的账户或云托管服务。Ferryx 目前不提供这些；
- 需要预构建的 Linux 或 Windows ARM64 二进制文件。目前尚不支持。（从下一个版本起，没有 GPU 适配器的主机会回退到较慢的 DOM 终端渲染器。）

## 性能

下面的数据是一次离屏测量，并非与其他终端的对比。

- 离屏 WGPU 渲染基准，50 帧，800x480 px：p50 3.10 ms，p95 4.34 ms。
- 硬件：Apple M4 Max（Metal，集成 GPU），macOS。
- 来源：[`bench/terminal/evidence/native-wgpu-phase2-summary.md`](bench/terminal/evidence/native-wgpu-phase2-summary.md)。

复现渲染数据：

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
  --example native_terminal_renderer_poc -- \
  --headless --output bench/terminal/evidence/native-wgpu-phase2-latest.png
```

运行终端解析器和吞吐量基线套件（写入 `bench/terminal/evidence/baseline-*`）：

```bash
bun run bench/terminal/runner.ts
```

数据取决于硬件；在依赖这些数据之前，请先在自己的机器上运行这两项测试。

## 架构各有不同

Ferryx 原生 Ghostty 和 Rust 组件的架构清单。这不是与基于 Electron 的 AI IDE 或终端模拟器之间的性能基准比较。

| 架构组件 | Ferryx 实现 |
| :--- | :--- |
| 终端解析器 | libghostty-vt |
| 桌面渲染 | WGPU 原生子表面 |
| PTY 生命周期 | 带输出重放的无头 Rust 守护进程 |
| 嵌入式浏览器 | 原生 WebView 分屏标签 |
| 移动端配对 | 账户登录网关，配合自定义 DOM 终端网格 |
| 智能体监控 | 基于清单的状态检测和通知 |

## 指南与比较

实用操作指南：

- [并行运行编码智能体](https://ferryx.dev/use-cases/parallel-ai-agents/)：说明智能体为何会在共享工作目录中相互冲突，以及如何通过每个智能体独立使用一个 worktree 来解决
- [Git worktree 实践](https://ferryx.dev/use-cases/git-worktree-workflow/)：手动命令，以及手动流程何时会变得繁琐
- [远程终端访问](https://ferryx.dev/use-cases/remote-terminal-access/)：如何通过手机查看长时间运行的任务

基于架构和许可条款对比其他方案，不作未经测量的性能声明：

- [Ferryx 与 Warp 对比](https://ferryx.dev/compare/warp/)
- [Ferryx 与 Wave Terminal 对比](https://ferryx.dev/compare/wave-terminal/)
- [Ferryx 与 Conductor 对比](https://ferryx.dev/compare/conductor/)
- [Ferryx 与 Crystal / Nimbalyst 对比](https://ferryx.dev/compare/crystal/)
- [Ferryx 与 tmux + git worktree 对比](https://ferryx.dev/compare/tmux-git-worktree/)
- [Ferryx 与 Ghostty](https://ferryx.dev/compare/ghostty/)：Ferryx 嵌入 libghostty-vt，并非 Ghostty 的竞争产品

## 文档与支持

- [用户指南](https://ferryx.dev/docs/guide/)：首次运行、工作区与 worktree、窗格、远程访问和智能体
- [故障排查](https://ferryx.dev/docs/troubleshooting/)：按症状索引的指南
- [隐私声明](https://ferryx.dev/privacy/)
- [安全策略](SECURITY.md)：如何私下报告漏洞、受支持的版本、威胁模型
- [贡献指南](CONTRIBUTING.md)：构建前置条件、测试命令和贡献许可
- [更新日志](CHANGELOG.md)：每个发布标签的变更
- [路线图](ROADMAP.md)：当前、下一步和以后的工作
- 问题交流：[Discord](https://discord.gg/Z2hBkQEHUG)

## 从源码构建

### 前置条件

- [Bun](https://bun.sh/) 1.4+
- [Rust](https://www.rust-lang.org/) stable（至少 1.89）
- [Zig](https://ziglang.org/download/) 0.17.0（构建固定版本的 Ghostty 时需要）
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)

在 Linux 上，编译桌面应用之前，请安装
[Tauri](https://v2.tauri.app/start/prerequisites/) 所需的 GTK 3、WebKitGTK 4.1 和其他系统库。无头 CLI 无需这些库即可构建：
`cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features --bin ferryx-cli`。

### 克隆并安装

```bash
git clone https://github.com/Indosaram/ferryx.git
cd ferryx
git submodule update --init --recursive
bun install
bun install --cwd ui
bun install --cwd site
```

### 获取内置的智能体运行时（mhc + bun）

hub 通过 [mhc](https://github.com/Indosaram/mhc) 运行 Slack、定时任务和判定轮次，而 mhc 的 eval 和 bash 工具需要 [bun](https://github.com/oven-sh/bun)。每个桌面安装包都内置这两者，其版本和 sha256 固定在 `src-tauri/resources/mhc/pin.json` 中。下面这条命令会从它们公开的 GitHub 发行版下载适合本机的版本，校验 mhc 的 minisign 签名以及两个文件的 sha256，然后放入已被 gitignore 的 `src-tauri/resources/mhc/payload/<target triple>/`：

```bash
bun scripts/fetch-mhc.mjs
```

该命令是幂等的：已存在且校验通过的下载会被跳过。`--target <triple>` 可为其他平台获取，`--target universal-apple-darwin` 会获取两个 macOS 架构。发布构建会自动运行它：`cargo tauri build` 在 `beforeBuildCommand` 中调用。开发构建不需要它；此时 hub 依次使用设置了的 `FERRYX_HUB_MHC_BIN`、运行中可执行文件旁内置的 mhc，最后是 PATH 中的 `mhc`。

### 运行桌面应用

Tauri 外壳会启动 `src-tauri/tauri.conf.json` 中定义的 UI 开发服务器：

```bash
cd src-tauri && cargo tauri dev
```

### 运行网站和文档

Astro + Starlight，端口为 14173：

```bash
bun run --cwd site dev
```

- 首页：<http://localhost:14173/>
- 文档：<http://localhost:14173/docs/introduction/>
- 快捷键参考：<http://localhost:14173/docs/shortcuts/>

### 验证更改

```bash
bun run --cwd ui build
bun run --cwd ui test
BASE_URL=/ bun run --cwd site build
cargo check --manifest-path src-tauri/Cargo.toml
```

## 架构

```text
ferryx/
├── src-tauri/   # Rust Tauri v2 核心：PTY 守护进程、IPC、通知、远程网关
├── ui/          # React 前端：原生终端表面、布局、远程配对
├── site/        # Astro + Starlight 首页和文档，部署到 Cloudflare Workers
└── docs/        # 规格说明、审计日志和参考资料
```

**Rust 原生核心（`src-tauri`）**负责伪终端生命周期、管理 Git worktree 租约、承载原生子 WebView，并运行用于移动端配对的、经过身份验证的 Axum WebSocket 网关。终端会话由无头守护进程而非 GUI 管理，因此关闭窗口不会影响 PTY。输出会写入带单调递增序列号的环形缓冲区；重新连接时会从上次的序列号继续重放。

**前端（`ui`）**是运行在 WebKit 或 WebView2 中的 React 外壳，负责窗格平铺、智能体标题检测、键盘快捷键和终端搜索覆盖层。

**网站（`site`）**是 Astro 和 Starlight 静态构建，用于托管首页、指南，以及一个运行真实产品组件的嵌入式演示。

## 参与贡献

欢迎贡献代码。请从 `main` 创建分支，运行上述验证命令，签署 [CLA](CLA.md)，并在拉取请求中说明用户可见的结果。

欢迎通过 [Discord](https://discord.gg/Z2hBkQEHUG) 提出问题和想法。

## 许可证

本项目以 [Ferryx License](LICENSE) 提供源代码。个人使用、在经认证的学校或大学中的教育用途、不由大型公司付费的开源工作、非营利组织，以及连同其关联组织计算员工和承包商少于 50 人且年收入低于 500 万美元的公司可免费使用。更大的公司在工作中使用时，需要为每位使用者订阅 Team（或更高）方案，过渡期在 Team 方案开放后 90 天结束。任何人都可以免费运行自托管中继（`ferryx-relay`）；桌面应用、CLI 以及移动和网页客户端仍遵循公司规模规则。改用 Ferryx License 之前发布的版本仍适用其发布时的 Sustainable Use License (SUL-1.0)。[许可证 FAQ](https://ferryx.dev/docs/license/) 列出了常见的允许和不允许情形及其对应条款。贡献代码需签署 [CLA](CLA.md)；参见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 代码签名政策

- **维护者**：[@Indosaram](https://github.com/Indosaram)
- **签名**：macOS DMG 已签名并经过公证。在 Windows 上，Microsoft Store 会对其发布的安装包签名；GitHub 上的 `.exe` 安装程序以及 Linux AppImage 和 `.deb` 未进行代码签名，请根据发行版的 `SHA256SUMS.txt` 校验。
- **隐私**：Ferryx 会在本地存储工作区数据。远程访问、嵌入式网站、外部编码智能体和更新服务可能通过网络交换数据。有关数据处理和删除选项，请参阅 [Ferryx 隐私政策](https://ferryx.dev/privacy/)。
