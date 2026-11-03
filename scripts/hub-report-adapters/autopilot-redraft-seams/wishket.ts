import { blocked } from "./trace.ts";
import { TransientAsideError } from "./aside.ts";
// Copied verbatim from outsourcing-autopilot/src/wishket.ts; the rest of the module is browser I/O.
export const tierOnly = (detail: string) => /PRIME·PRO·BOOST 파트너에게/.test(detail);
export const listProjects = () => {
  if (process.env.REDRAFT_TRANSIENT) throw new TransientAsideError(process.env.REDRAFT_TRANSIENT);
  return blocked("wishket");
};
export const projectText = () => blocked("wishket");
export const ensureLogin = () => blocked("wishket");
export const apply = () => blocked("wishket");
