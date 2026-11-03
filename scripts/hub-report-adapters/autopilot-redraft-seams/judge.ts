import { blocked, trace } from "./trace.ts";
// The model call is replaced by a recorded, scripted verdict.
export async function judgeProject(detail: string, note = "", ..._platform: string[]) {
  trace("judge", note);
  return JSON.parse(process.env.REDRAFT_VERDICT ?? "null") ?? blocked("judge-without-verdict");
}
const fn = () => blocked("judge");
export { fn as judgeMessage, fn as judgeBid, fn as judgeMercor, fn as planOrder };
export const hasBanned = () => [];
