import { blocked } from "./trace.ts";
// Same transient classifier as production collect.ts imports; browser automation is blocked.
export const TRANSIENT = /ERR_INTERNET_DISCONNECTED|ERR_NETWORK_CHANGED|ERR_NAME_NOT_RESOLVED|ERR_CONNECTION_(RESET|CLOSED|REFUSED|TIMED_OUT)|ERR_TIMED_OUT|Failed to fetch|fetch failed|other side closed|Aside isn't running|daemon auth challenge|is not connected to the daemon|CDP command timeout/;
export class TransientAsideError extends Error {}
export const asideRun = () => blocked("aside");
export const withPage = () => blocked("aside");
export const inPage = () => blocked("aside");
export const asideAgent = () => blocked("aside");
