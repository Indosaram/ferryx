import type { Terminal } from "@xterm/xterm";

import {
  recordCanvasFallback,
  recordWebglContextLoss,
  recordWebglCreated,
  recordWebglDisposed,
  recordWebglLoadFailure,
} from "./terminalRendererMetrics";

export async function loadTerminalAssets() {
  const [xtermModule, fitModule] = await Promise.all([
    import("@xterm/xterm"),
    import("@xterm/addon-fit"),
    import("@xterm/xterm/css/xterm.css"),
  ]);

  return {
    Terminal: xtermModule.Terminal,
    FitAddon: fitModule.FitAddon,
  };
}

export async function attachWebglRenderer(terminal: Terminal, signal?: AbortSignal): Promise<() => void> {
  let addon: InstanceType<(typeof import("@xterm/addon-webgl"))["WebglAddon"]> | null = null;
  let tracked = false;
  let disposed = false;

  const dispose = (canvasFallback: boolean) => {
    if (disposed) return;
    disposed = true;

    try {
      addon?.dispose();
    } finally {
      if (tracked) {
        tracked = false;
        recordWebglDisposed();
      }
      if (canvasFallback) recordCanvasFallback();
    }
  };

  try {
    const { WebglAddon } = await import("@xterm/addon-webgl");
    if (signal?.aborted) return () => undefined;

    addon = new WebglAddon();
    recordWebglCreated();
    tracked = true;

    addon.onContextLoss(() => {
      if (disposed) return;
      recordWebglContextLoss();
      dispose(true);
    });

    terminal.loadAddon(addon);
    return () => dispose(false);
  } catch {
    dispose(false);
    recordWebglLoadFailure();
    recordCanvasFallback();
    return () => undefined;
  }
}
