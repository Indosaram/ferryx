import { useEffect, useRef } from "react";

import type { FilePreviewAudioProps } from "../lib/filePreviewTypes";

export function FilePreviewAudio({
  payload,
  generation,
  onFailure,
}: FilePreviewAudioProps) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const url = payload.mediaUrl;

  useEffect(() => {
    const audio = audioRef.current;
    return () => {
      if (!audio) return;
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    };
  }, [generation, url]);

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
        ref={audioRef}
        data-testid="file-preview-audio-element"
        src={url}
        controls
        preload="metadata"
        className="w-full max-w-xl"
        onError={() => {
          onFailure({
            reason: "UnsupportedFormat",
            message: `Playback of ${payload.displayName} stopped because the preview stream could not be read.`,
            details: null,
          });
        }}
      />
    </div>
  );
}
