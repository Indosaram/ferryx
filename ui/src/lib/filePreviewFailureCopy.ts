import type { FilePreviewErrorReason } from "./filePreviewTypes";

/**
 * User copy for a preview refusal that is not a file-level reason (those are the frozen
 * `FilePreviewErrorReason`s): the preview service itself is missing. Reads the raw
 * `details.reason` of the rejected IPC call; returns null for anything else.
 */
export function previewServiceFailureHeadline(error: unknown): string | null {
  const details = (error as { details?: { reason?: unknown } | null } | null)?.details;
  switch (details?.reason) {
    case "previewServiceUnavailable":
      return "File previews are unavailable because the preview service could not start. Restart Ferryx to try again.";
    default:
      return null;
  }
}

/** Machine reason to user copy. Branching is on the reason, never on backend prose. */
export function failureHeadline(reason: FilePreviewErrorReason | null): string {
  switch (reason) {
    case "MissingFile":
      return "This file no longer exists.";
    case "PermissionDenied":
      return "This file cannot be read with the current permissions.";
    case "NotRegularFile":
      return "Only regular files can be previewed.";
    case "RemoteUnsupported":
      return "Files on remote sessions cannot be previewed on this machine.";
    case "TooLarge":
      return "This file is too large to preview.";
    case "UnsupportedEncoding":
      return "This file is not valid UTF-8 or BOM UTF-16 text.";
    case "UnsupportedFormat":
      return "This format cannot be previewed.";
    case "FileChanged":
      return "This file changed on disk. Reload to preview the current contents.";
    case "ExpiredHandle":
      return "This preview expired. Reload to request it again.";
    case null:
      return "This file could not be previewed.";
    default: {
      const unreachable: never = reason;
      return unreachable;
    }
  }
}
