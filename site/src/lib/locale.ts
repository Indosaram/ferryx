export type LocaleCode = 'en' | 'zh-cn';

export interface LandingLocale {
  code: LocaleCode;
  lang: 'en' | 'zh-CN';
  basePath: string;
  switchPath: string;
  labels: { skipPreview: string };
  copy: { previewTitle: string; previewDescription: string };
  nav: { release: string; features: string; docs: string; architecture: string; language: string; theme: string; discord: string; github: string };
  hero: { title: string; titleEnd: string; subtitle: string; github: string; discord: string; agents: string; platforms: string; free: string; license: string; facts: string };
  features: { eyebrow: string; title: string; intro: string; cards: ReadonlyArray<{ eyebrow: string; title: string; description: string; points: ReadonlyArray<string>; link?: string }> };
  footer: { tagline: string; features: string; architecture: string; useCases: string; compare: string; status: string; privacy: string; copyright: string };
  download: { platform: string; select: string; all: string; label: string; detected: string; allReleases: string };
  benchmarks: { title: string; description: string; inventory: string; source: string; component: string; implementation: string; evidence: string };
}

const messages = {
  en: {
    lang: 'en',
    labels: { skipPreview: 'Skip past the live preview' },
    copy: {
      previewTitle: 'The real workspace, running in your browser',
      previewDescription: 'This live preview mounts the genuine Ferryx sidebar, tab bar, and split terminal view against a mocked backend, so you can explore the multi-agent workspace layout before you install anything.',
    },
    nav: { release: 'Latest release', how: 'How it works', features: 'Features', docs: 'Docs', architecture: 'Architecture', language: '中文', theme: 'Switch theme', discord: 'Join the Ferryx Discord', github: 'Ferryx on GitHub' },
    hero: { title: 'A native terminal', titleEnd: 'for coding agents.', subtitle: 'Like cmux, but on macOS, Windows and Linux. Claude Code, Codex and any CLI agent keep running when you close the app, remote Linux boxes connect without SSH, and every tab shows which agent is waiting on you.', github: 'View on GitHub', discord: 'Join Discord', agents: 'Any CLI agent', platforms: 'macOS · Windows · Linux', free: 'Free for individuals and small teams', license: 'macOS, Windows, and Linux · source-available under the', facts: 'product facts' },
    features: {"eyebrow":"Why Ferryx","title":"Speed. Isolation. Total control.","intro":"Ferryx pairs a headless Rust daemon that outlives the window with a measured WGPU terminal path and a leak-checked libghostty-vt boundary.","cards":[{"eyebrow":"Rendering","title":"Native Ghostty & wgpu Engine","description":"Desktop terminal panes render directly via native libghostty and a GPU-accelerated wgpu pipeline for crisp font rasterization and low-latency throughput.","points":["libghostty terminal core","wgpu GPU pipeline","50-frame offscreen p50 3.10 ms on M4 Max"]},{"eyebrow":"Agents","title":"Multi-Agent Workspaces","description":"Orchestrate parallel AI coding agents (Claude, Codex, Antigravity) in isolated split-panes with real-time status indicators.","points":["Isolated worktree per agent","Live status indicators","Launch from the tab bar"]},{"eyebrow":"Layout","title":"Flexible Split-Pane Tiling","description":"Arbitrary vertical and horizontal terminal splits with responsive pointer drag resizing and smooth layout transitions.","points":["Vertical & horizontal splits","Pointer drag resizing","Drag tabs into any pane"]},{"eyebrow":"Remote","title":"Mobile Web Pairing","description":"Secure, authenticated remote web access after you sign in to your Ferryx account with an email magic link. Stream terminal output through a dependency-free DOM grid and steer agent workflows on the go.","points":["Account sign-in pairing","Streamed terminal grid","Steer agents from a phone"]},{"eyebrow":"Architecture","title":"Zero Electron Overhead","description":"Built on Tauri v2 and native WebView2/WebKit engines paired with a headless Rust PTY daemon. The core is cross-platform; selected OS integrations remain macOS-first.","points":["Tauri v2 shell","Headless Rust PTY daemon","Cross-platform core, macOS-first integrations"],"link":"Read the architecture deep dive →"},{"eyebrow":"Reliability","title":"Resilient Persistence","description":"Workspace state snapshots automatically, and the background daemon keeps sessions alive when the app quits or updates. Stopping the daemon or rebooting the host can still end live terminals.","points":["Layout snapshots","Daemon survives the GUI","Reattach with replay"]},{"eyebrow":"Next release","title":"A Supervisor for Your Agent Lanes","description":"On main, not in the current download yet. The supervisor watches each supervised pane, sends agent questions to your Inbox, phone or Slack instead of answering for you, shows each lane's goal and next step, and confirms that an instruction it sends actually started a turn.","points":["Questions go to you; it never picks an answer for you","Holds off while you type; Take over pauses it","Shows exactly what it typed"]}]},
    footer: { tagline: 'A native terminal for coding agents', features: 'Features', architecture: 'Architecture Docs', useCases: 'Use Cases', compare: 'Compare', status: 'Relay Status', privacy: 'Privacy Policy', copyright: 'Ferryx Contributors. All rights reserved.' },
    download: { platform: 'Select platform', select: 'Select platform and architecture', all: 'All Platforms', label: 'Download Ferryx', detected: 'Detected', allReleases: 'All releases on GitHub' },
    benchmarks: { title: 'Architecture, Not a Benchmark', description: 'A source-linked inventory of what Ferryx implements. It does not claim comparative performance without comparative measurements.', inventory: 'Implementation Inventory', source: 'Each row names the repository location that supports the claim.', component: 'Component', implementation: 'Ferryx implementation', evidence: 'Evidence' },
  },
  'zh-cn': {
    lang: 'zh-CN',
    labels: { skipPreview: '跳过实时预览' },
    copy: {
      previewTitle: '在浏览器中体验真实工作区',
      previewDescription: '此实时预览使用模拟后端加载 Ferryx 真实的侧边栏、标签栏和分屏终端视图，让你在安装前探索多智能体工作区布局。演示中的应用界面为英文。',
    },
    nav: { release: '最新版本', how: '使用方式', features: '功能', docs: '文档（英文）', architecture: '架构', language: 'English', theme: '切换主题', discord: '加入 Ferryx Discord', github: '在 GitHub 上查看 Ferryx' },
    hero: { title: '为编码智能体打造的', titleEnd: '原生终端。', subtitle: '类似 cmux，但同时支持 macOS、Windows 和 Linux。关闭应用后 Claude Code、Codex 以及任意 CLI 智能体继续运行，远程 Linux 机器无需 SSH 即可连接，每个标签页都会显示哪个智能体在等你。', github: '在 GitHub 上查看', discord: '加入 Discord', agents: '支持任意 CLI 智能体', platforms: 'macOS · Windows · Linux', free: '个人与小团队免费', license: 'macOS、Windows 和 Linux · 源代码可用，采用', facts: '产品事实（英文）' },
    features: {"eyebrow":"为什么选择 Ferryx","title":"速度、隔离与完全掌控。","intro":"Ferryx 将比窗口活得更久的无头 Rust 守护进程，与经过测量的 WGPU 终端管线和经过泄漏检查的 libghostty-vt 边界结合在一起。","cards":[{"eyebrow":"渲染","title":"原生 Ghostty 与 wgpu 引擎","description":"桌面终端窗格通过原生 libghostty 与 GPU 加速的 wgpu 管线直接渲染，呈现清晰字体与低延迟输出。","points":["libghostty 终端核心","wgpu GPU 管线","M4 Max 上 50 帧离屏渲染 p50 为 3.10 毫秒"]},{"eyebrow":"智能体","title":"多智能体工作区","description":"在隔离的分屏窗格中并行运行 AI 编码智能体（Claude、Codex、Antigravity），并查看实时状态。","points":["每个智能体使用独立 worktree","实时状态指示","从标签栏启动"]},{"eyebrow":"布局","title":"灵活的分屏平铺","description":"支持任意垂直与水平终端分屏、指针拖动调整大小，以及流畅的布局过渡。","points":["垂直与水平分屏","拖动调整大小","将标签拖入任意窗格"]},{"eyebrow":"远程","title":"移动网页配对","description":"使用邮箱魔法链接登录 Ferryx 账户后远程访问。在无需依赖的 DOM 终端网格中查看流式输出，随时操控智能体工作。","points":["账户登录配对","流式终端网格","用手机操控智能体"]},{"eyebrow":"架构","title":"零 Electron 开销","description":"采用 Tauri v2 和原生 WebView2 / WebKit 引擎，搭配无头 Rust PTY 守护进程。核心支持跨平台，部分系统集成仍以 macOS 为主。","points":["Tauri v2 外壳","无头 Rust PTY 守护进程","跨平台核心，部分 macOS 优先集成"],"link":"深入了解架构（英文） →"},{"eyebrow":"可靠性","title":"可靠的持久化","description":"工作区状态自动快照，应用退出或更新时后台守护进程会保持会话运行。停止守护进程或重启主机仍可能结束正在运行的终端。","points":["布局快照","GUI 关闭后守护进程仍运行","重连并回放输出"]},{"eyebrow":"下一个版本","title":"为智能体通道准备的监督者","description":"已在 main 分支，当前下载版本中尚未包含。监督者会关注每个受监督的窗格，把智能体的提问发送到你的收件箱、手机或 Slack，而不是替你作答；它会显示每个通道的目标和下一步，并确认它发出的指令确实开始了一轮执行。","points":["提问交给你处理，它从不替你选择答案","你输入时它会等待；点击接管即可暂停","准确显示它输入了什么"]}]},
    footer: { tagline: '为编码智能体打造的原生终端', features: '功能', architecture: '架构文档（英文）', useCases: '使用场景（英文）', compare: '对比（英文）', status: '中继状态（英文）', privacy: '隐私政策（英文）', copyright: 'Ferryx 贡献者。保留所有权利。' },
    download: { platform: '选择平台', select: '选择平台和架构', all: '所有平台', label: '下载 Ferryx', detected: '已检测', allReleases: '在 GitHub 查看所有版本' },
    benchmarks: { title: '架构实现，而非性能对比', description: '以下清单列出 Ferryx 已实现的功能及对应源代码。没有对比测量数据时，我们不会宣称性能优于其他产品。', inventory: '实现清单', source: '每一行都标明支持相关说明的代码位置。', component: '组件', implementation: 'Ferryx 实现', evidence: '代码依据' },
  },
} as const;

export function getLocale(code: LocaleCode, baseUrl: string): LandingLocale {
  const locale = messages[code];
  const base = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`;
  return {
    code,
    lang: locale.lang,
    basePath: code === 'en' ? base : `${base}zh-cn/`,
    switchPath: code === 'en' ? `${base}zh-cn/` : base,
    labels: locale.labels,
    copy: locale.copy,
    nav: locale.nav,
    hero: locale.hero,
    features: locale.features,
    footer: locale.footer,
    download: locale.download,
    benchmarks: locale.benchmarks,
  };
}
