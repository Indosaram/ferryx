// The panel's transcript state: pages are read after the last cursor and appended, one read at a
// time, so a refresh while another runs never duplicates or reorders a row. A failed background
// read keeps what is on screen; the export, which must hold every stored row, refuses instead.
import { useCallback, useEffect, useRef, useState } from "react";

import {
  exportTranscriptFile,
  readTranscriptAfter,
  type ExportResult,
  type TranscriptMessage,
  type TranscriptPageLoader,
} from "./transcript";

export type TranscriptSource = {
  readonly messages: readonly TranscriptMessage[];
  readonly refresh: () => Promise<void>;
  /** Reads the latest rows, then saves every stored row as a file. */
  readonly exportFile: () => Promise<ExportResult>;
};

/**
 * `load` null means there is no source yet (no host pinned): no rows. `version` changes whenever
 * the host heard the supervisor change, which reads what was stored since.
 */
export function useTranscript(load: TranscriptPageLoader | null, version?: unknown): TranscriptSource {
  const [messages, setMessages] = useState<readonly TranscriptMessage[]>([]);
  const loadRef = useRef(load);
  loadRef.current = load;
  const messagesRef = useRef<readonly TranscriptMessage[]>([]);
  const cursorRef = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const generation = useRef(0);

  // Reads what was stored since the last read. Resolves true when that read completed (or there
  // was no source or it was superseded), false when it failed.
  const read = useCallback(() => {
    const mine = generation.current;
    const run = queue.current.then(async (): Promise<boolean> => {
      const current = loadRef.current;
      if (!current || mine !== generation.current) return true;
      try {
        const got = await readTranscriptAfter(current, cursorRef.current);
        if (mine !== generation.current) return true;
        cursorRef.current = got.cursor;
        if (got.messages.length === 0) return true;
        const seen = new Set(messagesRef.current.map((message) => message.id));
        const next = [...messagesRef.current, ...got.messages.filter((message) => !seen.has(message.id))];
        messagesRef.current = next;
        setMessages(next);
        return true;
      } catch {
        return false;
      }
    });
    queue.current = run.then(() => undefined);
    return run;
  }, []);

  // Background refresh: a failed read keeps what is on screen; the next change reads again.
  const refresh = useCallback(async () => {
    await read();
  }, [read]);

  // A different source is a different conversation: start again from the beginning.
  useEffect(() => {
    generation.current += 1;
    cursorRef.current = 0;
    messagesRef.current = [];
    setMessages((previous) => (previous.length === 0 ? previous : []));
  }, [load]);

  useEffect(() => {
    void refresh();
  }, [load, version, refresh]);

  const exportFile = useCallback(async () => {
    if (!(await read())) {
      return { ok: false, message: "The latest transcript could not be read, so nothing was exported. Try again." } as const;
    }
    return exportTranscriptFile(messagesRef.current, Date.now());
  }, [read]);

  return { messages, refresh, exportFile };
}
