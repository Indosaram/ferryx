import { useState, useEffect, useRef } from 'react';
import { Download, ChevronDown, ExternalLink, Sparkles } from 'lucide-react';
import { PLATFORMS, detectUserPlatform, GITHUB_RELEASE_LATEST, type PlatformId, type DownloadAsset } from '@/lib/downloads';
import { PlatformIcon } from '@/components/ui/PlatformIcons';
import { cn } from '@/lib/utils';

export interface DownloadMenuProps {
  variant?: 'hero' | 'navbar' | 'compact';
  className?: string;
}

export function DownloadMenu({ variant = 'hero', className }: DownloadMenuProps) {
  const [detectedPlatform, setDetectedPlatform] = useState<PlatformId>('macos');
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setDetectedPlatform(detectUserPlatform());
  }, []);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    }
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      document.addEventListener('keydown', handleKeyDown);
    }
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen]);

  const currentPlatformConfig = PLATFORMS[detectedPlatform];
  const primaryAsset = currentPlatformConfig.defaultAsset;

  return (
    <div ref={menuRef} className={cn('relative inline-flex items-center', className)}>
      {variant === 'hero' ? (
        <div className="inline-flex rounded-lg shadow-lg border border-zinc-700/80 bg-zinc-100 dark:bg-zinc-100 hover:bg-zinc-200 transition-all">
          <a
            href={primaryAsset.url}
            className="inline-flex items-center gap-2.5 px-5 py-3 text-base font-semibold text-zinc-950 hover:text-black transition-colors rounded-l-lg"
          >
            <PlatformIcon platform={detectedPlatform} className="h-5 w-5" />
            <span>Download for {currentPlatformConfig.name}</span>
            <span className="hidden sm:inline-block text-xs font-mono font-normal bg-zinc-300/80 text-zinc-900 px-1.5 py-0.5 rounded">
              {primaryAsset.fileType}
            </span>
          </a>
          <button
            type="button"
            onClick={() => setIsOpen(!isOpen)}
            aria-label="Select platform and architecture"
            aria-expanded={isOpen}
            className="inline-flex items-center px-3 border-l border-zinc-300 text-zinc-800 hover:bg-zinc-200/90 rounded-r-lg transition-colors"
          >
            <ChevronDown className={cn('h-4 w-4 transition-transform duration-200', isOpen && 'rotate-180')} />
          </button>
        </div>
      ) : variant === 'navbar' ? (
        <div className="inline-flex items-center rounded-md border border-zinc-800 bg-zinc-100 hover:bg-zinc-200 transition-colors shadow-sm">
          <a
            href={primaryAsset.url}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-zinc-900 hover:text-black"
          >
            <Download className="h-3.5 w-3.5" />
            <span>Download</span>
          </a>
          <button
            type="button"
            onClick={() => setIsOpen(!isOpen)}
            aria-label="Select platform"
            aria-expanded={isOpen}
            className="inline-flex items-center px-1.5 py-1.5 border-l border-zinc-300 text-zinc-700 hover:bg-zinc-200 transition-colors"
          >
            <ChevronDown className={cn('h-3.5 w-3.5 transition-transform duration-200', isOpen && 'rotate-180')} />
          </button>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => setIsOpen(!isOpen)}
          aria-expanded={isOpen}
          className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-zinc-300 hover:text-zinc-100 bg-zinc-900 border border-zinc-800 rounded-lg hover:border-zinc-700 transition-colors"
        >
          <Download className="h-4 w-4" />
          <span>All Platforms</span>
          <ChevronDown className={cn('h-3.5 w-3.5 transition-transform duration-200', isOpen && 'rotate-180')} />
        </button>
      )}

      {isOpen && (
        <div
          className={cn(
            'absolute top-full z-50 mt-2 w-80 sm:w-96 rounded-xl border border-zinc-800 bg-zinc-950/95 p-3 shadow-2xl backdrop-blur-md transition-all text-left animate-in fade-in zoom-in-95',
            variant === 'navbar' ? 'right-0' : 'left-1/2 -translate-x-1/2 sm:left-0 sm:translate-x-0'
          )}
        >
          <div className="px-2 py-1.5 border-b border-zinc-800/80 mb-2 flex items-center justify-between">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
              Download Ferryx Latest
            </span>
            <span className="text-[10px] text-zinc-500 font-mono">v0.1.0-alpha</span>
          </div>

          <div className="space-y-3 max-h-[70vh] overflow-y-auto pr-1">
            {(Object.keys(PLATFORMS) as PlatformId[]).map((platformKey) => {
              const platform = PLATFORMS[platformKey];
              const isCurrent = detectedPlatform === platformKey;

              return (
                <div key={platform.id} className="space-y-1.5">
                  <div className="flex items-center justify-between px-2 text-xs font-semibold text-zinc-300">
                    <div className="flex items-center gap-1.5">
                      <PlatformIcon platform={platform.id} className="h-3.5 w-3.5 text-zinc-400" />
                      <span>{platform.name}</span>
                    </div>
                    {isCurrent && (
                      <span className="text-[10px] px-1.5 py-0.2 text-emerald-400 bg-emerald-950/50 border border-emerald-800/50 rounded">
                        Detected
                      </span>
                    )}
                  </div>

                  <div className="grid gap-1">
                    {platform.assets.map((asset: DownloadAsset) => (
                      <a
                        key={asset.id}
                        href={asset.url}
                        onClick={() => setIsOpen(false)}
                        target={asset.isStore ? '_blank' : undefined}
                        rel={asset.isStore ? 'noreferrer' : undefined}
                        className={cn(
                          'group flex items-center justify-between p-2 rounded-lg text-xs transition-colors',
                          asset.recommended
                            ? 'bg-zinc-900/90 border border-zinc-800 hover:border-zinc-700 hover:bg-zinc-850 text-zinc-100'
                            : 'hover:bg-zinc-900/60 text-zinc-300 border border-transparent'
                        )}
                      >
                        <div className="flex flex-col gap-0.5">
                          <div className="flex items-center gap-1.5">
                            <span className="font-medium text-zinc-200 group-hover:text-zinc-100">
                              {asset.name}
                            </span>
                            {asset.recommended && (
                              <Sparkles className="h-3 w-3 text-amber-400 inline" />
                            )}
                          </div>
                          <span className="text-[11px] text-zinc-500 font-mono">
                            {asset.architecture}
                          </span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="px-1.5 py-0.5 text-[10px] font-mono rounded bg-zinc-800 text-zinc-300 border border-zinc-700">
                            {asset.fileType}
                          </span>
                          {asset.isStore ? (
                            <ExternalLink className="h-3.5 w-3.5 text-zinc-500 group-hover:text-zinc-300" />
                          ) : (
                            <Download className="h-3.5 w-3.5 text-zinc-500 group-hover:text-zinc-300" />
                          )}
                        </div>
                      </a>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="mt-3 pt-2 border-t border-zinc-800/80 flex items-center justify-between px-2 text-[11px] text-zinc-400">
            <a
              href={GITHUB_RELEASE_LATEST}
              target="_blank"
              rel="noreferrer"
              className="hover:text-zinc-200 flex items-center gap-1 transition-colors"
            >
              <ExternalLink className="h-3 w-3" />
              All releases on GitHub
            </a>
            <a
              href="#quickstart"
              onClick={() => setIsOpen(false)}
              className="hover:text-zinc-200 transition-colors"
            >
              Build from source →
            </a>
          </div>
        </div>
      )}
    </div>
  );
}
