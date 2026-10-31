import { useMemo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { openExternalUrl } from "../lib/browserTauri";
import { parseNotebook, type NotebookCell, type NotebookOutput } from "../lib/ipynbParse";

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

/**
 * Notebook Markdown is file content, not app chrome: links are buttons (never
 * `<a href>`, which middle click / drag could still navigate the webview with) that
 * hand http(s) targets to the system browser; anything else is inert text. Images
 * render only inline data URIs. No typography plugin is installed, so elements get
 * explicit Tailwind classes instead of relying on `prose`.
 */
const NOTEBOOK_MARKDOWN_COMPONENTS: Components = {
  a: ({ href, children }) => {
    const target = typeof href === "string" ? href : "";
    if (!isHttpUrl(target)) {
      return <span className="text-zinc-400">{children}</span>;
    }
    return (
      <button
        type="button"
        data-testid="notebook-link"
        title={`Open ${target} in your browser`}
        className="inline text-sky-400 underline underline-offset-2"
        onClick={() => {
          void openExternalUrl(target).catch(() => undefined);
        }}
      >
        {children}
      </button>
    );
  },
  img: ({ src, alt }) => {
    const source = typeof src === "string" ? src : "";
    const label = typeof alt === "string" ? alt : "";
    if (/^data:image\/(png|jpeg|gif|webp);base64,/i.test(source)) {
      return <img src={source} alt={label} className="max-w-full h-auto rounded" />;
    }
    return <span data-testid="notebook-image-blocked" className="text-zinc-500">[image: {label || source}]</span>;
  },
  h1: ({ children }) => <h1 className="mb-3 mt-4 text-2xl font-semibold text-zinc-100">{children}</h1>,
  h2: ({ children }) => <h2 className="mb-2 mt-4 text-xl font-semibold text-zinc-100">{children}</h2>,
  h3: ({ children }) => <h3 className="mb-2 mt-3 text-lg font-semibold text-zinc-100">{children}</h3>,
  h4: ({ children }) => <h4 className="mb-1 mt-3 text-base font-semibold text-zinc-100">{children}</h4>,
  h5: ({ children }) => <h5 className="mb-1 mt-2 text-sm font-semibold text-zinc-100">{children}</h5>,
  h6: ({ children }) => <h6 className="mb-1 mt-2 text-sm font-semibold text-zinc-300">{children}</h6>,
  p: ({ children }) => <p className="my-2">{children}</p>,
  ul: ({ children }) => <ul className="my-2 list-disc space-y-1 pl-6">{children}</ul>,
  ol: ({ children }) => <ol className="my-2 list-decimal space-y-1 pl-6">{children}</ol>,
  li: ({ children }) => <li className="pl-1">{children}</li>,
  pre: ({ children }) => (
    <pre className="my-2 overflow-x-auto rounded bg-zinc-900 p-2 font-mono text-xs text-zinc-200 [&>code]:bg-transparent [&>code]:p-0">
      {children}
    </pre>
  ),
  code: ({ className, children }) => (
    <code className={`rounded bg-zinc-800/80 px-1 py-0.5 font-mono text-[0.85em] text-zinc-200${className ? ` ${className}` : ""}`}>
      {children}
    </code>
  ),
};

function OutputItem({ output }: { output: NotebookOutput }) {
  if (output.kind === "text") {
    return (
      <pre className="font-mono text-xs text-zinc-300 bg-zinc-950/60 p-2 rounded whitespace-pre-wrap overflow-x-auto">
        {output.text}
      </pre>
    );
  }

  if (output.kind === "image") {
    return (
      <div className="py-1">
        <img
          src={`data:${output.mime};base64,${output.base64}`}
          alt="Cell output"
          className="max-w-full h-auto rounded"
        />
      </div>
    );
  }

  if (output.kind === "error") {
    return (
      <div className="bg-red-950/40 border border-red-800/60 text-red-200 p-2.5 rounded font-mono text-xs whitespace-pre-wrap overflow-x-auto">
        <div className="font-semibold text-red-400 mb-1">{output.name}</div>
        <div>{output.message}</div>
      </div>
    );
  }

  return null;
}

function MarkdownCell({ cell }: { cell: NotebookCell }) {
  return (
    <div
      data-testid="notebook-cell"
      data-kind="markdown"
      className="prose prose-invert prose-sm max-w-none text-zinc-200 leading-relaxed px-4 py-2"
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={NOTEBOOK_MARKDOWN_COMPONENTS}>
        {cell.source}
      </ReactMarkdown>
    </div>
  );
}

function CodeCell({ cell }: { cell: NotebookCell }) {
  const executionLabel = cell.executionCount !== null ? `In [${cell.executionCount}]:` : "In [ ]:";

  return (
    <div
      data-testid="notebook-cell"
      data-kind="code"
      className="my-2 border border-zinc-800/70 bg-zinc-900/50 rounded-md overflow-hidden"
    >
      <div className="flex items-start text-xs font-mono">
        <span className="w-16 flex-shrink-0 text-right pr-3 pt-2 text-zinc-500 select-none">
          {executionLabel}
        </span>
        <pre className="flex-1 font-mono text-xs text-zinc-200 p-2 overflow-x-auto whitespace-pre">
          {cell.source}
        </pre>
      </div>

      {cell.outputs.length > 0 && (
        <div className="border-t border-zinc-800/50 bg-zinc-950/40 p-3 space-y-2">
          {cell.outputs.map((output, idx) => (
            <OutputItem key={idx} output={output} />
          ))}
        </div>
      )}
    </div>
  );
}

function RawCell({ cell }: { cell: NotebookCell }) {
  return (
    <div
      data-testid="notebook-cell"
      data-kind="raw"
      className="my-2 p-3 bg-zinc-900/30 border border-zinc-800/40 rounded-md"
    >
      <pre className="font-mono text-xs text-zinc-400 whitespace-pre-wrap overflow-x-auto">
        {cell.source}
      </pre>
    </div>
  );
}

export function FilePreviewNotebook({ text }: { text: string }) {
  const notebook = useMemo(() => parseNotebook(text), [text]);

  if (!notebook) {
    return (
      <div className="p-4 text-center">
        <p data-testid="file-preview-notebook-invalid" className="text-sm text-red-400 font-medium">
          Invalid or unreadable Jupyter notebook.
        </p>
      </div>
    );
  }

  return (
    <div
      data-testid="file-preview-notebook"
      className="selectable w-full h-full overflow-y-auto p-4 space-y-3 bg-zinc-950 text-zinc-100"
    >
      {notebook.cells.map((cell, idx) => {
        if (cell.kind === "markdown") {
          return <MarkdownCell key={idx} cell={cell} />;
        }
        if (cell.kind === "code") {
          return <CodeCell key={idx} cell={cell} />;
        }
        return <RawCell key={idx} cell={cell} />;
      })}
    </div>
  );
}
