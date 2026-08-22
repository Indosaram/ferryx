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
        "flex items-center justify-between space-x-3 rounded-lg border border-zinc-800 bg-zinc-900/80 px-4 py-2.5 font-mono text-xs text-zinc-300 backdrop-blur transition-colors hover:border-zinc-700",
        className
      )}
    >
      <div className="flex items-center space-x-2 truncate">
        {prefix && <span className="select-none text-zinc-500 font-semibold">{prefix}</span>}
        <span className="truncate select-all text-zinc-200">{code}</span>
      </div>
      <button
        onClick={handleCopy}
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100 transition-colors"
        title="Copy to clipboard"
      >
        {copied ? (
          <Check className="h-3.5 w-3.5 text-emerald-400" />
        ) : (
          <Copy className="h-3.5 w-3.5" />
        )}
      </button>
    </div>
  );
}
