import type { AttachmentReceipt, TargetRef } from "../../../lib/scopedContracts";
import type { AttachmentUploader } from "./session";

/** Same-origin authenticated staging route; server must validate control permission and
 * full target, forward bytes to its host, and return only a verified staging receipt. */
export class HttpAttachmentUploader implements AttachmentUploader {
  constructor(private readonly endpoint: URL, private readonly fetcher: typeof fetch = fetch) {}
  async stage(target: TargetRef, bytes: Uint8Array, sha256: string, requestId: string): Promise<AttachmentReceipt> {
    const response = await this.fetcher(this.endpoint, { method: "POST", credentials: "same-origin", headers: {
      "Content-Type": "image/png", "X-Ferryx-Target": encodeURIComponent(JSON.stringify(target)),
      "X-Ferryx-Sha256": sha256, "X-Ferryx-Request-Id": requestId,
    }, body: bytes.slice() });
    const result = await response.json();
    if (!response.ok || result.ok !== true) throw new Error(result.error?.code ?? `UPLOAD_HTTP_${response.status}`);
    const receipt = result.data as AttachmentReceipt;
    if (receipt.hostId !== target.hostId || receipt.sha256 !== sha256 || receipt.sizeBytes !== bytes.length || receipt.mediaType !== "image/png" || !receipt.attachmentId) throw new Error("ATTACHMENT_MISMATCH");
    return receipt;
  }
}
