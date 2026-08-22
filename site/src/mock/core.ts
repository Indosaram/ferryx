import { invoke as backendInvoke } from './backend';
export function invoke<T>(cmd: string, args?: unknown): Promise<T> {
  return Promise.resolve().then(() => backendInvoke(cmd, args) as T);
}
export const isTauri = () => true;
