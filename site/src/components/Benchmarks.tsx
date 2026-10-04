import type { LandingLocale } from "@/lib/locale";

export function Benchmarks({ locale }: { locale: LandingLocale }) {
  const comparisonRows = locale.code === 'zh-cn' ? [
    { component: "终端解析器", implementation: "libghostty-vt", evidence: "src-tauri/src/native_terminal/sys/ffi.rs" },
    { component: "桌面渲染器", implementation: "WGPU 原生子表面", evidence: "src-tauri/src/native_terminal/renderer/" },
    { component: "PTY 生命周期", implementation: "无头 Rust 守护进程与顺序回放", evidence: "src-tauri/src/daemon/ · terminal/output_hub.rs" },
    { component: "嵌入式浏览器", implementation: "原生 WebView 分屏标签", evidence: "ui/src/components/browser/" },
    { component: "移动终端", implementation: "无需依赖的 DOM 网格", evidence: "ui/src/remote/RemoteTerminal.tsx" },
    { component: "智能体监控", implementation: "基于清单的状态检测", evidence: "src-tauri/src/agent_detect/" },
  ] : [
    { component: "Terminal parser", implementation: "libghostty-vt", evidence: "src-tauri/src/native_terminal/sys/ffi.rs" },
    { component: "Desktop renderer", implementation: "WGPU native child surfaces", evidence: "src-tauri/src/native_terminal/renderer/" },
    { component: "PTY lifecycle", implementation: "Headless Rust daemon with sequenced replay", evidence: "src-tauri/src/daemon/ · terminal/output_hub.rs" },
    { component: "Embedded browser", implementation: "Native WebView split-tabs", evidence: "ui/src/components/browser/" },
    { component: "Mobile terminal", implementation: "Dependency-free DOM grid", evidence: "ui/src/remote/RemoteTerminal.tsx" },
    { component: "Agent supervision", implementation: "Manifest-driven status detection", evidence: "src-tauri/src/agent_detect/" },
  ];

  return (
    <section id="architecture" className="py-24 sm:py-28 border-t border-line bg-page relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            {locale.benchmarks.title}
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            {locale.benchmarks.description}
          </p>
        </div>

        <div className="rounded-3xl border border-line bg-surface overflow-hidden shadow-card">
          <div className="p-6 border-b border-line bg-page-raised">
            <h3 className="text-[17px] font-medium tracking-[-0.015em] text-ink">{locale.benchmarks.inventory}</h3>
            <p className="text-[13px] text-ink-faint mt-1">{locale.benchmarks.source}</p>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-left">
              <thead className="border-b border-line bg-page-raised text-[11px] uppercase tracking-[0.12em] text-ink-faint">
                <tr>
                  <th className="py-3.5 px-6 font-medium">{locale.benchmarks.component}</th>
                  <th className="py-3.5 px-6 font-medium text-ink">{locale.benchmarks.implementation}</th>
                  <th className="py-3.5 px-6 font-medium text-ink-faint">{locale.benchmarks.evidence}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {comparisonRows.map((row) => (
                  <tr key={row.component} className="hover:bg-page-raised transition-colors">
                    <td className={`py-4 px-6 text-[15px] text-ink font-medium${locale.code === 'zh-cn' ? ' whitespace-nowrap' : ''}`}>{row.component}</td>
                    <td className="py-4 px-6 text-[14px] text-ink-soft">{row.implementation}</td>
                    <td className="py-4 px-6 font-mono text-[12px] text-ink-faint">{row.evidence}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  );
}

