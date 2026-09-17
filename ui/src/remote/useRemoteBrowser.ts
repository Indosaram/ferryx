/**
 * Remote Browser screencast subscription lifecycle hook (§4.3, Phase 5)
 *
 * State flow: opening -> ready -> streaming/paused -> closing -> closed
 * Reconnect = new single-use ticket + new subscription; NO mutation auto-replay.
 * Frame buffer cleared on background / host change.
 * Object URLs released on replace and unmount.
 * Stale seq discarded for decode order enforcement.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HostEndpoint } from "../state/remoteHostStore";
import { BrowserClient } from "./browserClient";
import type {
  BrowserFrame,
  BrowserHelloMessage,
  BrowserStateMessage,
  BrowserSubscribeOptions,
} from "./browserProtocol";
import { remoteSocketUrl } from "./remoteClient";

export type RemoteBrowserStatus =
  | "opening"
  | "ready"
  | "streaming"
  | "paused"
  | "closing"
  | "closed";

export interface UseRemoteBrowserOptions {
  baseUrl: string;
  browserId: string | null;
  deviceToken: string;
  options?: BrowserSubscribeOptions;
  enabled?: boolean;
  host?: HostEndpoint | null;
}

export interface UseRemoteBrowserResult {
  status: RemoteBrowserStatus;
  frame: BrowserFrame | null;
  imageUrl: string | null;
  browserState: BrowserStateMessage | null;
  hello: BrowserHelloMessage | null;
  error: Error | null;
  client: BrowserClient | null;
  reconnect: () => void;
  sendAck: (streamId: number, seq: number) => void;
}

export function useRemoteBrowser({
  baseUrl,
  browserId,
  deviceToken,
  options,
  enabled = true,
  host = null,
}: UseRemoteBrowserOptions): UseRemoteBrowserResult {
  const [status, setStatus] = useState<RemoteBrowserStatus>(
    enabled && browserId ? "opening" : "closed",
  );
  const [frame, setFrame] = useState<BrowserFrame | null>(null);
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [browserState, setBrowserState] = useState<BrowserStateMessage | null>(null);
  const [hello, setHello] = useState<BrowserHelloMessage | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [reconnectNonce, setReconnectNonce] = useState(0);

  const clientRef = useRef<BrowserClient | null>(null);
  const currentStreamIdRef = useRef<number | null>(null);
  const lastDecodedSeqRef = useRef<number>(-1);
  const currentUrlRef = useRef<string | null>(null);

  // Decompose options to prevent infinite useEffect triggers from object identity
  const format = options?.format ?? "jpeg";
  const quality = options?.quality ?? 70;
  const intervalMs = options?.intervalMs;
  const maxEdge = options?.maxEdge;

  const subscribeOptions = useMemo<BrowserSubscribeOptions>(() => {
    const opts: BrowserSubscribeOptions = { format, quality };
    if (intervalMs !== undefined) opts.intervalMs = intervalMs;
    if (maxEdge !== undefined) opts.maxEdge = maxEdge;
    return opts;
  }, [format, quality, intervalMs, maxEdge]);

  const clearFrameBuffer = useCallback(() => {
    if (currentUrlRef.current) {
      URL.revokeObjectURL(currentUrlRef.current);
      currentUrlRef.current = null;
    }
    setImageUrl(null);
    setFrame(null);
  }, []);

  const reconnect = useCallback(() => {
    // Tear down current connection immediately
    if (clientRef.current) {
      clientRef.current.close();
      clientRef.current = null;
    }
    clearFrameBuffer();
    // Increment reconnect nonce to trigger fresh connection with new ticket and new subscription
    // NO mutation auto-replay is performed
    setReconnectNonce((n) => n + 1);
  }, [clearFrameBuffer]);

  const sendAck = useCallback((streamId: number, seq: number) => {
    if (clientRef.current) {
      clientRef.current.ackFrame(streamId, seq);
    }
  }, []);

  // Handle background / visibility change: clear frame buffer when document is hidden (§4.3)
  useEffect(() => {
    const handleVisibilityChange = () => {
      if (document.hidden) {
        clearFrameBuffer();
        setStatus((s) => (s === "streaming" ? "paused" : s));
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [clearFrameBuffer]);

  // Main connection effect
  useEffect(() => {
    if (!enabled || !browserId) {
      clearFrameBuffer();
      setStatus("closed");
      return;
    }

    let cancelled = false;
    const abortController = new AbortController();
    setStatus("opening");
    setError(null);

    async function init() {
      try {
        const target = `/api/v1/browser/${encodeURIComponent(browserId!)}`;
        // Obtain target-bound single-use socket ticket
        const wsUrl = await remoteSocketUrl(baseUrl, target, deviceToken, abortController.signal);
        if (cancelled) return;

        const client = new BrowserClient(wsUrl);
        clientRef.current = client;

        client.onClose(() => {
          if (!cancelled) {
            setStatus("closed");
          }
        });

        client.onError((err) => {
          if (!cancelled) {
            setError(err instanceof Error ? err : new Error(String(err)));
          }
        });

        client.onState((stateMsg) => {
          if (cancelled) return;
          setBrowserState(stateMsg);
          if (stateMsg.paused) {
            setStatus("paused");
          } else if (stateMsg.loading) {
            // Document loading
          }
        });

        client.onFrame((incomingFrame) => {
          if (cancelled) return;

          // Verify stream ID matches active stream
          if (
            currentStreamIdRef.current !== null &&
            incomingFrame.metadata.streamId !== currentStreamIdRef.current
          ) {
            return;
          }

          // Decode order enforcement: stale seq discarded
          if (incomingFrame.seq <= lastDecodedSeqRef.current) {
            return;
          }
          lastDecodedSeqRef.current = incomingFrame.seq;

          // Release previous object URL immediately
          if (currentUrlRef.current) {
            URL.revokeObjectURL(currentUrlRef.current);
            currentUrlRef.current = null;
          }

          // Create new object URL for this frame
          const blob = new Blob([incomingFrame.imageBytes as unknown as BlobPart], {
            type: incomingFrame.format === "jpeg" ? "image/jpeg" : "image/png",
          });
          const newUrl = URL.createObjectURL(blob);
          currentUrlRef.current = newUrl;

          setImageUrl(newUrl);
          setFrame(incomingFrame);

          // Confirm displayed frame with ACK
          client.ackFrame(incomingFrame.metadata.streamId, incomingFrame.seq);
        });

        // Wait for browserHello from server
        const helloMsg = await client.waitForHello();
        if (cancelled) {
          client.close();
          return;
        }
        setHello(helloMsg);
        setStatus("ready");

        // Automatically issue subscription once ready
        const subMsg = await client.subscribe(subscribeOptions);
        if (cancelled) {
          client.close();
          return;
        }
        currentStreamIdRef.current = subMsg.streamId;
        lastDecodedSeqRef.current = -1;
        setStatus("streaming");
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err : new Error(String(err)));
          setStatus("closed");
        }
      }
    }

    init();

    return () => {
      cancelled = true;
      abortController.abort();
      setStatus("closing");
      if (clientRef.current) {
        clientRef.current.close();
        clientRef.current = null;
      }
      clearFrameBuffer();
      currentStreamIdRef.current = null;
      lastDecodedSeqRef.current = -1;
      setStatus("closed");
    };
  }, [
    baseUrl,
    browserId,
    deviceToken,
    enabled,
    host,
    format,
    quality,
    intervalMs,
    maxEdge,
    reconnectNonce,
    clearFrameBuffer,
  ]);

  return {
    status,
    frame,
    imageUrl,
    browserState,
    hello,
    error,
    client: clientRef.current,
    reconnect,
    sendAck,
  };
}
