// The composer's view of a converse result: the reply text and whether it carries proof (D9).
// Shared by the desktop surface and the phone's pinned composer so both render the same chip.
import type { PanelActionResult } from '../../lib/supervisorPanel/actions';
import { isReplyUnverified } from '../../lib/supervisorPanel/model';
import type { ComposerSendResult } from './types';

export function toComposerResult(result: PanelActionResult): ComposerSendResult {
  if (!result.ok) return { ok: false, message: result.message };
  return result.reply
    ? { ok: true, reply: { text: result.reply.text, unverified: isReplyUnverified(result.reply.evidence) } }
    : { ok: true };
}
