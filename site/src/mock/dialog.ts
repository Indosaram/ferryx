export async function open(): Promise<null> { return null; }

// Mirrors the Tauri dialog plugin's save(): resolving null means the user cancelled,
// which is the only possible outcome in a browser-hosted demo.
export async function save(_options?: unknown): Promise<null> { return null; }
