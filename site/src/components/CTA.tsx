import { Download, Github, Terminal, ExternalLink, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { CopySnippet } from "@/components/ui/CopySnippet";
import { PlatformIcon } from "@/components/ui/PlatformIcons";
import { PLATFORMS, GITHUB_RELEASE_LATEST, type DownloadAsset } from "@/lib/downloads";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function CTA() {
  return (
    <section id="downloads" className="py-20 border-t border-zinc-800/60 bg-gradient-to-b from-zinc-950 via-zinc-900/30 to-zinc-950 relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        {/* Header */}
        <div className="text-center max-w-2xl mx-auto mb-14">
          <img
            src={iconSrc}
            alt="Ferryx"
            className="h-16 w-16 mx-auto mb-6 rounded-2xl drop-shadow-xl"
          />
          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100">
            Download Ferryx
          </h2>
          <p className="mt-4 text-zinc-400 text-base sm:text-lg">
            Native, ultra-fast agentic workspaces for macOS, Windows, and Linux. Choose your direct binary installer below.
          </p>
        </div>

        {/* 3-Column Multi-Platform Downloads Grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-16">
          {/* macOS Card */}
          <div className="rounded-2xl border border-zinc-800 bg-zinc-950/80 p-6 flex flex-col justify-between shadow-xl hover:border-zinc-700 transition-colors">
            <div>
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl bg-zinc-900 border border-zinc-800">
                    <PlatformIcon platform="macos" className="h-6 w-6 text-zinc-100" />
                  </div>
                  <div>
                    <h3 className="text-lg font-semibold text-zinc-100">macOS</h3>
                    <p className="text-xs text-zinc-500 font-mono">{PLATFORMS.macos.badge}</p>
                  </div>
                </div>
                <Badge variant="outline" className="text-[10px] text-zinc-400 border-zinc-800">
                  DMG
                </Badge>
              </div>
              <p className="text-xs text-zinc-400 mb-5">
                Native builds for Apple Silicon (M1–M4) and Intel x86_64 architectures.
              </p>

              <div className="space-y-2.5">
                {PLATFORMS.macos.assets.map((asset: DownloadAsset) => (
                  <a
                    key={asset.id}
                    href={asset.url}
                    className={`flex items-center justify-between p-3 rounded-xl border text-xs transition-all ${
                      asset.recommended
                        ? 'bg-zinc-100 text-zinc-900 border-zinc-200 hover:bg-zinc-200 shadow-sm font-semibold'
                        : 'bg-zinc-900/80 text-zinc-200 border-zinc-800 hover:bg-zinc-850 hover:border-zinc-700'
                    }`}
                  >
                    <div className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-1.5">
                        <span>{asset.name}</span>
                        {asset.recommended && (
                          <Sparkles className="h-3 w-3 text-amber-600 inline" />
                        )}
                      </div>
                      <span className={`text-[10px] font-mono ${asset.recommended ? 'text-zinc-700' : 'text-zinc-500'}`}>
                        {asset.architecture}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <span className={`px-1.5 py-0.5 text-[10px] font-mono rounded ${
                        asset.recommended ? 'bg-zinc-200 text-zinc-800' : 'bg-zinc-800 text-zinc-300'
                      }`}>
                        {asset.fileType}
                      </span>
                      <Download className="h-3.5 w-3.5" />
                    </div>
                  </a>
                ))}
              </div>
            </div>

            <div className="mt-6 pt-4 border-t border-zinc-900 text-[11px] text-zinc-500 font-mono">
              Requires macOS 10.15 Catalina or later
            </div>
          </div>

          {/* Windows Card */}
          <div className="rounded-2xl border border-zinc-800 bg-zinc-950/80 p-6 flex flex-col justify-between shadow-xl hover:border-zinc-700 transition-colors">
            <div>
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl bg-zinc-900 border border-zinc-800">
                    <PlatformIcon platform="windows" className="h-6 w-6 text-zinc-100" />
                  </div>
                  <div>
                    <h3 className="text-lg font-semibold text-zinc-100">Windows</h3>
                    <p className="text-xs text-zinc-500 font-mono">{PLATFORMS.windows.badge}</p>
                  </div>
                </div>
                <Badge variant="outline" className="text-[10px] text-zinc-400 border-zinc-800">
                  EXE / MSIX
                </Badge>
              </div>
              <p className="text-xs text-zinc-400 mb-5">
                Direct NSIS standalone installer and signed modern Windows app packages.
              </p>

              <div className="space-y-2.5">
                {PLATFORMS.windows.assets.map((asset: DownloadAsset) => (
                  <a
                    key={asset.id}
                    href={asset.url}
                    target={asset.isStore ? '_blank' : undefined}
                    rel={asset.isStore ? 'noreferrer' : undefined}
                    className={`flex items-center justify-between p-3 rounded-xl border text-xs transition-all ${
                      asset.recommended
                        ? 'bg-zinc-100 text-zinc-900 border-zinc-200 hover:bg-zinc-200 shadow-sm font-semibold'
                        : 'bg-zinc-900/80 text-zinc-200 border-zinc-800 hover:bg-zinc-850 hover:border-zinc-700'
                    }`}
                  >
                    <div className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-1.5">
                        <span>{asset.name}</span>
                        {asset.recommended && (
                          <Sparkles className="h-3 w-3 text-amber-600 inline" />
                        )}
                      </div>
                      <span className={`text-[10px] font-mono ${asset.recommended ? 'text-zinc-700' : 'text-zinc-500'}`}>
                        {asset.architecture}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <span className={`px-1.5 py-0.5 text-[10px] font-mono rounded ${
                        asset.recommended ? 'bg-zinc-200 text-zinc-800' : 'bg-zinc-800 text-zinc-300'
                      }`}>
                        {asset.fileType}
                      </span>
                      {asset.isStore ? <ExternalLink className="h-3.5 w-3.5" /> : <Download className="h-3.5 w-3.5" />}
                    </div>
                  </a>
                ))}
              </div>
            </div>

            <div className="mt-6 pt-4 border-t border-zinc-900 text-[11px] text-zinc-500 font-mono">
              Windows 10/11 (64-bit) with WebView2 runtime
            </div>
          </div>

          {/* Linux Card */}
          <div className="rounded-2xl border border-zinc-800 bg-zinc-950/80 p-6 flex flex-col justify-between shadow-xl hover:border-zinc-700 transition-colors">
            <div>
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl bg-zinc-900 border border-zinc-800">
                    <PlatformIcon platform="linux" className="h-6 w-6 text-zinc-100" />
                  </div>
                  <div>
                    <h3 className="text-lg font-semibold text-zinc-100">Linux</h3>
                    <p className="text-xs text-zinc-500 font-mono">{PLATFORMS.linux.badge}</p>
                  </div>
                </div>
                <Badge variant="outline" className="text-[10px] text-zinc-400 border-zinc-800">
                  AppImage / DEB
                </Badge>
              </div>
              <p className="text-xs text-zinc-400 mb-5">
                Portable AppImage for all major distros and native Debian / Ubuntu deb packages.
              </p>

              <div className="space-y-2.5">
                {PLATFORMS.linux.assets.map((asset: DownloadAsset) => (
                  <a
                    key={asset.id}
                    href={asset.url}
                    className={`flex items-center justify-between p-3 rounded-xl border text-xs transition-all ${
                      asset.recommended
                        ? 'bg-zinc-100 text-zinc-900 border-zinc-200 hover:bg-zinc-200 shadow-sm font-semibold'
                        : 'bg-zinc-900/80 text-zinc-200 border-zinc-800 hover:bg-zinc-850 hover:border-zinc-700'
                    }`}
                  >
                    <div className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-1.5">
                        <span>{asset.name}</span>
                        {asset.recommended && (
                          <Sparkles className="h-3 w-3 text-amber-600 inline" />
                        )}
                      </div>
                      <span className={`text-[10px] font-mono ${asset.recommended ? 'text-zinc-700' : 'text-zinc-500'}`}>
                        {asset.architecture}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <span className={`px-1.5 py-0.5 text-[10px] font-mono rounded ${
                        asset.recommended ? 'bg-zinc-200 text-zinc-800' : 'bg-zinc-800 text-zinc-300'
                      }`}>
                        {asset.fileType}
                      </span>
                      <Download className="h-3.5 w-3.5" />
                    </div>
                  </a>
                ))}
              </div>
            </div>

            <div className="mt-6 pt-4 border-t border-zinc-900 text-[11px] text-zinc-500 font-mono">
              Requires WebKitGTK 4.0 / 4.1 & GTK 3
            </div>
          </div>
        </div>

        {/* Quickstart & Build From Source Container */}
        <div id="quickstart" className="rounded-2xl border border-zinc-800 bg-zinc-950/90 p-8 sm:p-10 shadow-2xl relative overflow-hidden text-center">
          <h3 className="text-xl sm:text-2xl font-bold tracking-tight text-zinc-100 mb-2">
            Build Ferryx From Source
          </h3>
          <p className="text-zinc-400 max-w-lg mx-auto text-sm mb-6">
            Clone the repository and run the developer build pipeline with Bun and Cargo Tauri.
          </p>

          <div className="max-w-xl mx-auto mb-8">
            <CopySnippet
              code="git clone https://github.com/Indosaram/ferryx.git && cd ferryx && bun install --cwd ui && bun install --cwd site && cd src-tauri && cargo tauri dev"
              className="text-left"
            />
          </div>

          <div className="flex flex-wrap items-center justify-center gap-4">
            <a href={GITHUB_RELEASE_LATEST} target="_blank" rel="noreferrer">
              <Button size="lg" className="bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold px-6">
                <Download className="mr-2 h-4 w-4" />
                Latest GitHub Release
              </Button>
            </a>
            <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer">
              <Button variant="outline" size="lg" className="border-zinc-800 hover:bg-zinc-900 text-zinc-300 px-6">
                <Github className="mr-2 h-4 w-4" />
                GitHub Repository
              </Button>
            </a>
          </div>

          <div className="mt-8 flex flex-wrap items-center justify-center gap-6 text-xs text-zinc-500 font-mono">
            <span className="flex items-center gap-1.5">
              <Terminal className="h-3.5 w-3.5 text-zinc-400" /> Rust + Tauri v2
            </span>
            <span>100% Open Source</span>
            <span>Zero Electron Bloat</span>
          </div>
        </div>
      </div>
    </section>
  );
}
