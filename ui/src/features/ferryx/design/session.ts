import type { AttachmentReceipt, ConfirmedDraft, DeliveryReceipt, TargetRef } from "../../../lib/scopedContracts";
import type { DesignIdentity, GuestEvent, Selection } from "./model";
import { sameIdentity } from "./model";
export interface Capture { selection: Selection; png: Uint8Array; sha256: string }
/** Native bridge owns identity validation, overlay-free compositor capture and PNG crop. */
export interface DesignBridge {
  subscribe(listener: (event: GuestEvent) => void): Promise<() => void>;
  begin(identity: DesignIdentity, mode: "element" | "rectangle"): Promise<void>;
  cancel(identity: DesignIdentity): Promise<void>;
  capture(selection: Selection): Promise<Capture>;
  validate(identity: DesignIdentity): Promise<void>;
}
/** stage transfers these bytes to target.hostId; receipt only after remote hash verification. */
export interface AttachmentUploader {
  stage(target: TargetRef, bytes: Uint8Array, sha256: string, requestId: string): Promise<AttachmentReceipt>;
}
export interface DesignDelivery {
  validateTarget(target: TargetRef): Promise<void>;
  deliver(draft: ConfirmedDraft, requestId: string): Promise<DeliveryReceipt>;
}
export class DesignSession {
  capture?: Capture;
  draft?: ConfirmedDraft;
  error?: Error;
  private identity?: DesignIdentity;
  private unsubscribe?: () => void;
  private revision = 0;
  private requestId?: string;
  private confirmedRevision?: number;
  private sending?: Promise<DeliveryReceipt>;
  private receipt?: DeliveryReceipt;
  private listeners = new Set<() => void>();
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private changed() { for (const listener of this.listeners) listener(); }
  constructor(readonly bridge: DesignBridge, readonly uploader: AttachmentUploader, readonly delivery: DesignDelivery) {}
  async begin(identity: DesignIdentity, mode: "element" | "rectangle") {
    await this.cancel();
    this.identity = { ...identity }; const revision = this.revision;
    this.capture = undefined; this.draft = undefined; this.receipt = undefined; this.error = undefined;
    const unsubscribe = await this.bridge.subscribe(event => {
      const eventIdentity = event.type === "selected" ? event.selection.identity : event.identity;
      if (revision !== this.revision || !sameIdentity(identity, eventIdentity)) return;
      if (event.type !== "selected") { this.revision++; this.identity = undefined; this.unsubscribe?.(); this.unsubscribe = undefined; this.changed(); return; }
      void this.bridge.capture(event.selection).then(async capture => {
        await this.bridge.validate(identity);
        if (revision !== this.revision || !sameIdentity(identity, capture.selection.identity)) return;
        this.capture = { ...capture, png: capture.png.slice() }; this.changed();
      }).catch(error => { if (revision === this.revision) { this.error = error instanceof Error ? error : new Error(String(error)); this.changed(); } });
    });
    if (revision !== this.revision) { unsubscribe(); return; }
    this.unsubscribe = unsubscribe;
    try { await this.bridge.begin(identity, mode); } catch (error) { unsubscribe(); this.identity = undefined; throw error; }
  }
  async confirm(target: TargetRef, note: string, requestId: string): Promise<void> {
    if (!this.capture || this.sending) throw new Error("NO_CAPTURE");
    const capture = this.capture, revision = this.revision;
    const frozenTarget = Object.freeze({ ...target });
    await this.bridge.validate(capture.selection.identity);
    await this.delivery.validateTarget(frozenTarget);
    const attachment = await this.uploader.stage(frozenTarget, capture.png.slice(), capture.sha256, requestId);
    if (revision !== this.revision || capture !== this.capture) throw new Error("TARGET_EXPIRED");
    await this.bridge.validate(capture.selection.identity);
    await this.delivery.validateTarget(frozenTarget);
    if (attachment.hostId !== target.hostId || attachment.sha256 !== capture.sha256 || attachment.sizeBytes !== capture.png.length || attachment.mediaType !== "image/png") throw new Error("ATTACHMENT_MISMATCH");
    this.draft = Object.freeze({ target: frozenTarget, browserGeneration: capture.selection.identity.generation, note, attachments: Object.freeze([Object.freeze({ ...attachment })]) });
    this.requestId = requestId; this.confirmedRevision = revision; this.receipt = undefined; this.changed();
  }
  async send(): Promise<DeliveryReceipt> {
    if (this.receipt) return this.receipt;
    if (this.sending) return this.sending;
    if (!this.draft || !this.requestId || !this.capture) throw new Error("NOT_CONFIRMED");
    if (this.confirmedRevision !== this.revision) throw new Error("TARGET_EXPIRED");
    const draft = this.draft, requestId = this.requestId, identity = this.capture.selection.identity, revision = this.revision;
    this.sending = (async () => {
      await this.bridge.validate(identity); await this.delivery.validateTarget(draft.target);
      if (revision !== this.revision) throw new Error("TARGET_EXPIRED");
      const receipt = await this.delivery.deliver(draft, requestId);
      if (receipt.requestId !== requestId || JSON.stringify(receipt.target) !== JSON.stringify(draft.target)) throw new Error("DELIVERY_MISMATCH");
      this.receipt = receipt; return receipt;
    })();
    try { return await this.sending; } finally { this.sending = undefined; this.changed(); }
  }
  async cancel(): Promise<void> {
    this.revision++; this.unsubscribe?.(); this.unsubscribe = undefined;
    const identity = this.identity; this.identity = undefined;
    if (identity) await this.bridge.cancel(identity);
    this.changed(); // Unsent capture and confirmed draft intentionally survive cancellation/failure.
  }
}
