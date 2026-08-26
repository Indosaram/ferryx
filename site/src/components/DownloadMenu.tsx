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
  const [maxPanelHeight, setMaxPanelHeight] = useState<number>();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setDetectedPlatform(detectUserPlatform());
  }, []);

  // top-full anchoring means the panel's own offset decides how much room is left,
  // so measure the trigger instead of capping against the raw viewport height.
  useEffect(() => {
    if (!isOpen) return;
    const fit = () => {
      const trigger = menuRef.current?.getBoundingClientRect();
      if (trigger) setMaxPanelHeight(Math.max(240, window.innerHeight - trigger.bottom - 24));
    };
    fit();
    window.addEventListener('resize', fit);
    window.addEventListener('scroll', fit, { passive: true });
    return () => {
      window.removeEventListener('resize', fit);
      window.removeEventListener('scroll', fit);
    };
  }, [isOpen]);

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
        <div className="inline-flex items-stretch rounded-full bg-ink text-page hover:bg-ink-hover transition-colors duration-150 shadow-sm">
          <a
            href={primaryAsset.url}
            className="inline-flex items-center gap-2.5 px-5 h-11 text-[15px] font-medium text-page rounded-l-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
          >
            <PlatformIcon platform={detectedPlatform} className="h-5 w-5" />
            <span>Download for {currentPlatformConfig.name}</span>
            <span className="hidden sm:inline-block text-xs font-mono font-normal bg-page/15 text-page/90 px-1.5 py-0.5 rounded">
              {primaryAsset.fileType}
            </span>
          </a>
          <button
            type="button"
            onClick={() => setIsOpen(!isOpen)}
            aria-label="Select platform and architecture"
            aria-expanded={isOpen}
            className="inline-flex items-center px-3.5 border-l border-page/15 text-page/80 hover:text-page rounded-r-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
          >
            <ChevronDown className={cn('h-4 w-4 transition-transform duration-200', isOpen && 'rotate-180')} />
          </button>
        </div>
      ) : variant === 'navbar' ? (
        <div className="inline-flex items-center rounded-lg bg-ink text-page hover:bg-ink-hover transition-colors duration-150 shadow-sm">
          <a
            href={primaryAsset.url}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-page rounded-l-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
          >
            <Download className="h-3.5 w-3.5" />
            <span>Download</span>
          </a>
          <button
            type="button"
            onClick={() => setIsOpen(!isOpen)}
            aria-label="Select platform"
            aria-expanded={isOpen}
            className="inline-flex items-center px-2 py-1.5 border-l border-page/15 text-page/80 hover:text-page rounded-r-lg transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
          >
            <ChevronDown className={cn('h-3.5 w-3.5 transition-transform duration-200', isOpen && 'rotate-180')} />
          </button>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => setIsOpen(!isOpen)}
          aria-expanded={isOpen}
          className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-ink bg-surface border border-line rounded-lg hover:border-line-strong hover:bg-page-raised transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
        >
          <Download className="h-4 w-4" />
          <span>All Platforms</span>
          <ChevronDown className={cn('h-3.5 w-3.5 transition-transform duration-200', isOpen && 'rotate-180')} />
        </button>
      )}

      {isOpen && (
        <div
          className={cn(
            'absolute top-full z-50 mt-2 w-80 sm:w-96 rounded-2xl border border-line bg-surface p-3 shadow-menu transition-all text-left animate-in fade-in zoom-in-95 flex flex-col',
            variant === 'navbar' ? 'right-0' : 'left-1/2 -translate-x-1/2 sm:left-0 sm:translate-x-0'
          )}
          style={maxPanelHeight ? { maxHeight: `${maxPanelHeight}px` } : undefined}
        >
          <div className="px-2 py-1.5 border-b border-line mb-2 flex items-center justify-between">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-ink-faint">
              Download Ferryx Latest
            </span>
            <span className="text-[10px] text-ink-faint font-mono">v0.1.0-alpha</span>
          </div>

          <div className="space-y-3 min-h-0 flex-1 overflow-y-auto pr-1">
            {(Object.keys(PLATFORMS) as PlatformId[]).map((platformKey) => {
              const platform = PLATFORMS[platformKey];
              const isCurrent = detectedPlatform === platformKey;

              return (
                <div key={platform.id} className="space-y-1.5">
                  <div className="flex items-center justify-between px-2 text-xs font-semibold text-ink">
                    <div className="flex items-center gap-1.5">
                      <PlatformIcon platform={platform.id} className="h-3.5 w-3.5 text-ink-soft" />
                      <span>{platform.name}</span>
                    </div>
                    {isCurrent && (
                      <span className="text-[10px] px-1.5 py-0.5 text-ink-soft bg-page border border-line rounded">
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
                          'group flex items-center justify-between p-2 rounded-xl text-xs transition-colors',
                          asset.recommended
                            ? 'bg-ink text-page hover:bg-ink-hover'
                            : 'hover:bg-page text-ink'
                        )}
                      >
                        <div className="flex flex-col gap-0.5">
                          <div className="flex items-center gap-1.5">
                            <span className={cn('font-medium', asset.recommended ? 'text-page' : 'text-ink')}>
                              {asset.name}
                            </span>
                            {asset.recommended && (
                              <Sparkles className="h-3 w-3 text-page/70 inline" />
                            )}
                          </div>
                          <span className={cn('text-[11px] font-mono', asset.recommended ? 'text-page/60' : 'text-ink-faint')}>
                            {asset.architecture}
                          </span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className={cn('px-1.5 py-0.5 text-[10px] font-mono rounded border', asset.recommended ? 'bg-page/15 text-page border-page/20' : 'border-line bg-page text-ink-soft')}>
                            {asset.fileType}
                          </span>
                          {asset.isStore ? (
                            <ExternalLink className={cn('h-3.5 w-3.5', asset.recommended ? 'text-page/70 group-hover:text-page' : 'text-ink-faint group-hover:text-ink')} />
                          ) : (
                            <Download className={cn('h-3.5 w-3.5', asset.recommended ? 'text-page/70 group-hover:text-page' : 'text-ink-faint group-hover:text-ink')} />
                          )}
                        </div>
                      </a>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="mt-3 pt-2 border-t border-line flex items-center justify-end px-2 text-[11px] text-ink-faint">
            <a
              href={GITHUB_RELEASE_LATEST}
              target="_blank"
              rel="noreferrer"
              className="hover:text-ink flex items-center gap-1 transition-colors"
            >
              <ExternalLink className="h-3 w-3" />
              All releases on GitHub
            </a>
          </div>
        </div>
      )}
    </div>
  );
}
