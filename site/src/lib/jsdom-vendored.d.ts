// jsdom ships no types of its own and @types/jsdom is not installed here. The analytics DOM
// tests import it by path from the desktop UI's node_modules (CI installs those in the same
// job that runs `bun test --cwd site`), so declare just the surface those tests use.
declare module '*/ui/node_modules/jsdom/lib/api.js' {
  export class JSDOM {
    constructor(html?: string, options?: Record<string, unknown>);
    readonly window: Window & typeof globalThis & { close: () => void };
  }
}
