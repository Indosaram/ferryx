import { useState } from "react";
import { Copy, Check } from "lucide-react";
import { cn } from "@/lib/utils";

interface CopySnippetProps {
  code: string;
  prefix?: string;
  className?: string;
}

export function CopySnippet({ code, prefix = "$", className }: CopySnippetProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div
      className={cn(
        "flex items-center justify-between space-x-3 rounded-xl border border-code-border bg-code-bg px-4 py-2.5 font-mono text-xs text-code-ink transition-colors",
        className
      )}
    >
      <div className="flex items-center space-x-2 truncate">
        {prefix && <span className="select-none text-ink-faint font-semibold">{prefix}</span>}
        <span className="truncate select-all text-code-ink">{code}</span>
      </div>
      <button
        onClick={handleCopy}
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-ink-faint hover:bg-white/10 hover:text-code-ink transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30"
        title="Copy to clipboard"
      >
        {copied ? (
          <Check className="h-3.5 w-3.5 text-code-ink" />
        ) : (
          <Copy className="h-3.5 w-3.5" />
        )}
      </button>
    </div>
  );
}
