import { Suspense, lazy, useEffect, useRef, useState } from "react";
import type { LandingLocale } from "@/lib/locale";

// Astro hydrates this island on visibility; import the workspace only after that.
const LiveFerryxDemo = lazy(() => import("./LiveFerryxDemo"));

const PLACEHOLDER =
  "flex h-[560px] items-center justify-center rounded-[20px] border border-line bg-surface text-[13px] text-ink-soft";

export default function LazyLiveDemo({ locale }: { locale: LandingLocale }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [mount, setMount] = useState(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    if (typeof IntersectionObserver === "undefined") {
      setMount(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setMount(true);
          observer.disconnect();
        }
      },
      { rootMargin: "800px 0px" },
    );
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={hostRef} onKeyDownCapture={(event) => {
      // The desktop terminal consumes Tab. Let website visitors leave the demo
      // using the normal page sequence instead of forwarding Tab into the PTY.
      if (event.key === "Tab" && event.target instanceof HTMLTextAreaElement) {
        event.preventDefault();
        event.stopPropagation();
        const target = event.shiftKey
          ? document.querySelector<HTMLAnchorElement>('#preview > a')
          : document.querySelector<HTMLAnchorElement>('#features a');
        target?.focus();
      }
    }}>
      {mount ? (
        <Suspense fallback={<div className={PLACEHOLDER}>{locale.code === 'en' ? 'Loading the live preview...' : '正在加载实时预览……'}</div>}>
          <LiveFerryxDemo />
        </Suspense>
      ) : (
        <div className={PLACEHOLDER}>{locale.code === 'en' ? 'The live preview loads as you scroll to it.' : '滚动到此处时将加载实时预览。'}</div>
      )}
    </div>
  );
}
