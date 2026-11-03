// Test seam: every external effect a draft could reach is recorded here instead of executed.
import { appendFileSync } from "node:fs";
export function trace(kind: string, detail = ""): void {
  appendFileSync(process.env.REDRAFT_TRACE!, `${JSON.stringify({ kind, detail })}\n`);
}
export function blocked(kind: string): never {
  trace(kind);
  throw new Error(`redraft seam: ${kind} must not run in this test`);
}
