export type PushState = "loading" | "unsupported" | "insecure" | "denied" | "disabled" | "enabled" | "busy" | "error";
export function secureTaskLink(_value: unknown, _origin: string): string | null { return null; }
export interface PushApi { request(path: string, body?: unknown): Promise<unknown> }
export class PushClient {
  constructor(readonly api: PushApi, readonly registration: ServiceWorkerRegistration) {}
  async enable(_showBody = false): Promise<PushState> { return "enabled"; }
  async disable(): Promise<PushState> { return "disabled"; }
}
