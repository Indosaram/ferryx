import { blocked, trace } from "./trace.ts";
import { failed } from "./outcome.ts";
export async function bid() {
  const code = process.env.REDRAFT_BID_CODE;
  if (!code) return blocked("freelancer");
  trace("bid", code);
  return failed(code, "Recorded platform outcome");
}
const fn = () => blocked("freelancer");
export { fn as listMessages, fn as bannedKeywords, fn as lastMessages, fn as inboxUrl, fn as ongoingOrderIds, fn as orderDetailText,
  fn as sendMessage, fn as listProjects, fn as projectText, fn as ensureLogin, fn as apply, fn as worthJudging, fn as unreadCount };
