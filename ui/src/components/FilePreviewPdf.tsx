import type { FilePreviewPdfProps } from "../lib/filePreviewTypes";

export function FilePreviewPdf({ payload }: FilePreviewPdfProps) {
  const url = payload.mediaUrl;
  if (!url) {
    return (
      <p data-testid="file-preview-pdf-missing" className="px-4 py-6 text-sm text-muted-foreground">
        This PDF has no preview stream.
      </p>
    );
  }

  return (
    <iframe
      data-testid="file-preview-pdf-frame"
      title={payload.displayName}
      src={url}
      className="h-full w-full border-0 bg-background"
    />
  );
}
