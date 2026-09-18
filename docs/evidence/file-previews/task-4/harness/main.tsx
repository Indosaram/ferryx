/**
 * Isolated browser harness for the plan task 4 image renderer.
 *
 * It mounts the real `FilePreviewImage` with the real UI token layer, serving
 * fixture images over HTTP from the dev server (a stand-in for the task 1
 * loopback capability origin — this harness deliberately contains no capability
 * server, no IPC and no modal chrome, which other tasks own).
 */
import { StrictMode, useCallback, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import "@ui/index.css";
import { FilePreviewImage } from "@ui/components/FilePreviewImage";
import type { FilePreviewFailure, FilePreviewPayload } from "@ui/lib/filePreviewTypes";

type Fixture = {
  readonly id: string;
  readonly label: string;
  readonly payload: Omit<FilePreviewPayload, "handle">;
};

const FIXTURES: readonly Fixture[] = [
  {
    id: "png",
    label: "PNG 1600x1000",
    payload: base("grid-1600x1000.png", "image/png", "/fixtures/grid-1600x1000.png", 38_990),
  },
  {
    id: "jpeg",
    label: "JPEG 1200x750",
    payload: base("photo-1200x750.jpg", "image/jpeg", "/fixtures/photo-1200x750.jpg", 60_255),
  },
  {
    id: "gif",
    label: "GIF 640x400",
    payload: base("palette-640x400.gif", "image/gif", "/fixtures/palette-640x400.gif", 34_446),
  },
  {
    id: "gif-animated",
    label: "GIF animated",
    payload: base("animated-320x200.gif", "image/gif", "/fixtures/animated-320x200.gif", 1_152),
  },
  {
    id: "webp",
    label: "WebP 900x563",
    payload: base("shot-900x563.webp", "image/webp", "/fixtures/shot-900x563.webp", 8_970),
  },
  {
    id: "corrupt",
    label: "Corrupt PNG",
    payload: base("corrupt.png", "image/png", "/fixtures/corrupt.png", 4_192),
  },
  {
    id: "svg",
    label: "SVG (unsupported)",
    payload: base("diagram.svg", "image/svg+xml", "/fixtures/diagram.svg", 127),
  },
  {
    id: "no-url",
    label: "Missing media URL",
    payload: { ...base("broken.png", "image/png", "/fixtures/grid-1600x1000.png", 0), mediaUrl: null },
  },
];

function base(
  displayName: string,
  mediaType: string,
  mediaUrl: string,
  byteLength: number,
): Omit<FilePreviewPayload, "handle"> {
  return {
    displayName,
    kind: "image",
    byteLength,
    encoding: null,
    mediaType,
    mediaUrl,
    text: null,
    lineCount: null,
    target: null,
  };
}

function Harness() {
  const [fixtureId, setFixtureId] = useState<string>("png");
  const [generation, setGeneration] = useState(1);
  const [log, setLog] = useState<readonly string[]>([]);
  const generationRef = useRef(generation);
  generationRef.current = generation;

  const fixture = FIXTURES.find((entry) => entry.id === fixtureId) ?? FIXTURES[0];
  const payload: FilePreviewPayload = { handle: `${fixture.id}-${generation}`, ...fixture.payload };

  const append = useCallback((line: string) => {
    setLog((previous) => [...previous, line]);
  }, []);

  const select = useCallback((id: string) => {
    setFixtureId(id);
    setGeneration((value) => value + 1);
  }, []);

  return (
    <div className="flex h-screen w-screen flex-col gap-3 bg-background p-3 text-foreground">
      <div className="flex flex-wrap gap-1.5">
        {FIXTURES.map((entry) => (
          <button
            key={entry.id}
            type="button"
            data-fixture={entry.id}
            onClick={() => select(entry.id)}
            className={
              entry.id === fixtureId
                ? "rounded-md bg-accent px-2 py-1 text-[11px] text-foreground"
                : "rounded-md border border-border px-2 py-1 text-[11px] text-muted-foreground hover:bg-accent/40"
            }
          >
            {entry.label}
          </button>
        ))}
        <button
          type="button"
          data-fixture="race"
          onClick={() => {
            select("png");
            select("webp");
          }}
          className="rounded-md border border-border px-2 py-1 text-[11px] text-muted-foreground hover:bg-accent/40"
        >
          Replace mid-load
        </button>
      </div>

      <div
        data-testid="harness-frame"
        className="flex min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-card"
      >
        <FilePreviewImage
          payload={payload}
          generation={generation}
          onReload={() => append(`reload ${payload.handle}`)}
          onExternalOpen={() => append(`external ${payload.handle}`)}
          onFailure={(failure: FilePreviewFailure) =>
            append(`failure ${payload.handle} reason=${String(failure.reason)}`)
          }
        />
      </div>

      <pre
        data-testid="harness-log"
        className="h-16 shrink-0 overflow-auto rounded-md border border-border bg-card p-2 font-mono text-[10px] text-muted-foreground"
      >
        {log.join("\n")}
      </pre>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Harness />
  </StrictMode>,
);
