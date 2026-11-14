import type { AccountHttpRequest, AccountHttpResponse } from "../lib/tauri";

/**
 * Test double for the desktop `cmd_account_http_request` command: it replays the backend call
 * as the `fetch` a component test stubs, so desktop tests keep asserting on account URLs.
 */
export async function accountIpcViaFetch(args: unknown): Promise<AccountHttpResponse> {
  const request = args as AccountHttpRequest;
  const headers: Record<string, string> = {};
  if (request.body !== null) headers["Content-Type"] = "application/json";
  if (request.bearer !== null) headers.Authorization = `Bearer ${request.bearer}`;
  const res: Partial<Response> = await fetch(`${request.origin}${request.path}`, {
    method: request.method,
    headers,
    ...(request.body !== null ? { body: request.body } : {}),
  });
  const status = typeof res.status === "number" ? res.status : res.ok ? 200 : 500;
  const body = typeof res.text === "function" ? await res.text() : JSON.stringify(await res.json?.());
  return { status, body };
}
