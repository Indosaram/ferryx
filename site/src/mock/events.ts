import { emitEvent, on } from './backend';
export type UnlistenFn = () => void;
export type Event<T> = { event: string; id: number; payload: T };
let nextId = 1;
export function listen<T>(event: string, handler: (event: Event<T>) => void): Promise<UnlistenFn> {
  const id = nextId++;
  return Promise.resolve(on(event, (payload) => handler({ event, id, payload: payload as T })));
}
export function emit(event: string, payload?: unknown): Promise<void> { emitEvent(event, payload); return Promise.resolve(); }
export function emitTo(_target: unknown, event: string, payload?: unknown): Promise<void> { return emit(event, payload); }
