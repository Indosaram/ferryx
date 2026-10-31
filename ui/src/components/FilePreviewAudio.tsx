import { useEffect, useRef } from "react";

import type { FilePreviewAudioProps } from "../lib/filePreviewTypes";
import { mediaFailure } from "./FilePreviewVideo";

export function FilePreviewAudio({
  payload,
  generation,
  onFailure,
}: FilePreviewAudioProps) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const url = payload.mediaUrl;
  /** One media element per request: a reload (new generation) or new URL remounts it. */
  const instanceKey = `${generation}:${url ?? ""}`;
  const activeKeyRef = useRef(instanceKey);

  useEffect(() => {
    activeKeyRef.current = instanceKey;
    const audio = audioRef.current;
    return () => {
      if (!audio) return;
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    };
  }, [instanceKey]);

  if (!url) {
    return (
      <p data-testid="file-preview-audio-missing" className="px-4 py-6 text-sm text-muted-foreground">
        This audio file has no preview stream.
      </p>
    );
  }

  return (
    <div className="flex h-full min-h-0 items-center justify-center p-6" data-testid="file-preview-audio">
      <audio
        key={instanceKey}
        ref={audioRef}
        data-testid="file-preview-audio-element"
        src={url}
        controls
        preload="metadata"
        className="w-full max-w-xl"
        onError={(event) => {
          // A late error from a replaced element (teardown abort) must not fail the new preview.
          if (activeKeyRef.current !== instanceKey) return;
          const failure = mediaFailure(payload.displayName, event.currentTarget.error);
          if (failure) onFailure(failure);
        }}
      />
    </div>
  );
}
