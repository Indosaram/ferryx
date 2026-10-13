import { describe, expect, it } from "vitest";
import { HttpAttachmentUploader } from "./httpUploader";
const target = { hostId: "h", ownerId: "o", epoch: "1", backendSessionId: "s" };
const uploader = (response: Response) => new HttpAttachmentUploader(new URL("https://ferryx.test/staging"), (async () => response) as unknown as typeof fetch);
describe("http attachment uploader", () => {
  it("reports UPLOAD_HTTP_<status> for a non-ok non-JSON body instead of a JSON parse error", async () => {
    await expect(uploader(new Response("<html>bad gateway</html>", { status: 502 })).stage(target, new Uint8Array([1]), "h", "r")).rejects.toThrow("UPLOAD_HTTP_502");
  });
  it("prefers the structured error code from a non-ok JSON body", async () => {
    await expect(uploader(Response.json({ ok: false, error: { code: "CONTROL_DENIED" } }, { status: 403 })).stage(target, new Uint8Array([1]), "h", "r")).rejects.toThrow("CONTROL_DENIED");
  });
});