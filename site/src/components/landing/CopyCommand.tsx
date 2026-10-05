import { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";

export default function CopyCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(command);
    } catch {
      return;
    }
    setCopied(true);
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }
    timerRef.current = window.setTimeout(() => {
      setCopied(false);
      timerRef.current = null;
    }, 2000);
  };

  return (
    <div className="relative mt-4 rounded-xl border border-code-border bg-code-bg">
      <pre className="whitespace-pre-wrap break-all p-4 pr-12 font-mono text-[13px] leading-relaxed text-code-ink"><code>{command}</code></pre>
      <button
        type="button"
        aria-label="Copy install command"
        onClick={handleCopy}
        className="absolute right-2 top-2 inline-flex h-8 w-8 items-center justify-center rounded-md text-code-ink/70 transition-colors hover:bg-white/10 hover:text-code-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
      >
        {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
      </button>
      <span className="sr-only" aria-live="polite">
        {copied ? "Copied" : ""}
      </span>
    </div>
  );
}
