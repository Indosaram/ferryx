import { invoke as backendInvoke } from './backend';

export class Channel<T = unknown> {
  id: number;
  onmessage: (response: T) => void;
  constructor(onmessage?: (response: T) => void) {
    this.id = Math.floor(Math.random() * 1000000);
    this.onmessage = onmessage || (() => {});
  }
  send(response: T) {
    this.onmessage(response);
  }
}

export function invoke<T>(cmd: string, args?: unknown): Promise<T> {
  return Promise.resolve().then(() => backendInvoke(cmd, args) as T);
}
// ui/src/lib/tauri.ts short-circuits every command when this is false, which would
// bypass the mock backend entirely and leave the demo panes empty.
export const isTauri = () => true;

