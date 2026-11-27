import React, {
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AlertCircle,
  ArrowUp,
  Check,
  ChevronDown,
  CornerDownRight,
  FileText,
  ListPlus,
  Loader2,
  Pencil,
  Mic,
  Plus,
  Quote,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { cn } from "../../lib/cn";
import type { ChatAttachment } from "./MobileChatComponents";
import type { HeldMessage } from "./referenceQueue";
import type { FollowUpMessage } from "./referenceFollowUp";
import type {
  ReferenceComposerCapabilities,
  ReferenceComposerModelApply,
  ReferenceComposerOptionApply,
  ReferenceFileReceipt,
} from "./referenceTypes";
import { insertAtCaret, probeDictation, type DictationSession } from "./referenceDictation";
import { REFERENCE_CONTROL_EFFORT_ID, REFERENCE_CONTROL_MODEL_ID } from "./referenceControl";
import { isImeConfirmingKey } from "./imeKeys";
import type {
  AdditionalDirsControl,
  AgentControl,
  ChipItem,
  ChipSources,
  CitationInsert,
  ExecutionModeControl,
  FastModeControl,
  PermissionControl,
  PermissionMode,
  QueueDeliveryMode,
  QueueModeControl,
  ThinkingControl,
} from "./composer/chipTypes";
import {
  chipMarkdown,
  chipTriggerAt,
  citationMarkdown,
  staticChipCandidates,
  type ChipTrigger,
} from "./composer/chips";
import {
  decideTextPaste,
  isImageFile,
  isPromptTooLong,
  pasteTextFile,
  promptLengthMessage,
} from "./composer/composerLimits";
import { INITIAL_RECALL, stepRecall, type RecallState } from "./composer/recallHistory";
import {
  readStashes,
  removeStash,
  saveStash,
  stashableDraft,
  type StashedPrompt,
} from "./composer/promptStash";
import { unescapeMarkdownText } from "./composer/markdown";
import { VoiceConversationButton, type VoiceConversationProps } from "../../components/supervisor/VoiceConversationButton";

export type { ChatAttachment };

export interface MobileChatComposerHandle {
  clearDraft: () => void;
  focus: () => void;
  /** Insert a quote citation chip with an editable comment at the caret (composer.md). */
  insertCitation: (citation: CitationInsert) => void;
}

/** Where a queued follow-up goes when the send button is pressed mid-turn. */
export type FollowUpDefault = "queue" | "steer";

export interface ComposerControlChange {
  readonly controlId: string;
  readonly valueId: string;
  readonly label: string;
  readonly apply: ReferenceComposerModelApply | ReferenceComposerOptionApply;
}

export interface ComposerControlNotice {
  readonly status: "applied" | "rejected" | "unknown" | "pending" | "picker";
  readonly message: string;
}

/**
 * The message box of the reference chat (plan task 12).
 *
 * The draft belongs to the target (owning host + backend session + daemon incarnation), so the
 * owner passes the text in and takes every edit back out; the composer keeps its own copy only
 * when it is used standalone. A file is staged on the owning host by the owner and comes back as
 * an editable mention inside the draft, never as an opaque attachment this component would have
 * to upload.
 *
 * Held rows are messages the reference chat could not deliver when the user asked; nothing here
 * sends one on its own.
 */
export interface MobileChatComposerProps {
  /** The owner-scoped draft. Absent means the composer keeps its own text. */
  readonly value?: string;
  readonly onValueChange?: (text: string) => void;
  /** The owning host refused to persist the draft; the draft is still here and the user is told. */
  readonly draftUnsaved?: boolean;
  /** Files already staged on the owning host for this target; the draft holds their mentions. */
  readonly attachments?: readonly ReferenceFileReceipt[];
  /** Stage files on the owning host; caret is where the mention is inserted. */
  readonly onAttachFiles?: (files: readonly File[], caret: number) => void;
  readonly showAttachments?: boolean;
  /**
   * Where a staged image's bytes can be shown. The host owns the file, so this client never
   * builds a blob URL itself: without a URL the tile falls back to the file name, like t3code.
   */
  readonly attachmentPreviewUrl?: (receipt: ReferenceFileReceipt) => string | null;
  readonly onRemoveAttachment?: (attachmentId: string) => void;
  readonly attaching?: boolean;
  readonly attachError?: string | null;
  /** Held messages for this target, in the order the user wrote them. */
  readonly heldMessages?: readonly HeldMessage[];
  /** Held rows whose send is still on its way; each stays disabled until its own answer lands. */
  readonly heldSendingIds?: readonly string[];
  readonly heldUnsaved?: boolean;
  readonly onEditHeld?: (id: string, text: string) => void;
  readonly onSendHeld?: (id: string) => void;
  readonly onRemoveHeld?: (id: string) => void;
  /**
   * Follow-ups admitted while the agent is working. Shown whenever present.
   * Cancel is only offered before delivery.
   */
  readonly queuedMessages?: readonly FollowUpMessage[];
  readonly onCancelQueued?: (id: string) => void;
  /** Admit the draft to the target queue. `false` keeps the text in the box. */
  readonly onQueue?: (text: string) => boolean | void;
  /** Deliver now on the advertised steer path. `false` keeps the text in the box. */
  readonly onSteer?: (text: string) => boolean | void;
  /**
   * A prompt is waiting on the original pane. The card itself is drawn in the message stream by
   * the owner, never here, so the message box stays reachable with the keyboard open; this box
   * only keeps Send beside Stop while the prompt is up.
   */
  readonly promptOpen?: boolean;
  /** Something the user must know about this lane (a refused Stop, a held send, a lost host). */
  readonly warning?: string | null;
  /** One thing the user can do about the warning (retry, reconnect), drawn beside it. */
  readonly warningAction?: { readonly label: string; readonly onAction: () => void } | null;
  /** Send the draft. `false` means the owner kept it (held, refused) and the box must not clear. */
  readonly onSend: (text: string) => boolean | void;
  /**
   * Free-form voice for this conversation. Present, the composer draws a talk button beside
   * dictation and sends what was said through this same `onSend`, so a voice turn is a normal turn.
   */
  readonly voiceConversation?: VoiceConversationProps;
  readonly onStop?: () => void;
  readonly onClearDraft?: () => void;
  readonly isRunning?: boolean;
  readonly disabled?: boolean;
  readonly placeholder?: string;
  readonly className?: string;
  /**
   * Controls for this target only. Absent, null, or an empty list hides that control.
   * Nothing here is filled in from the agent name or a previous pane.
   */
  readonly composerCapabilities?: ReferenceComposerCapabilities | null;
  readonly onApplyControl?: (change: ComposerControlChange) => void;
  readonly controlNotice?: ComposerControlNotice | null;
  /** A change discards dictation and drops a late result. Absent is one unnamed fence. */
  readonly dictationFence?: string;
  /** Terminal mode, or any other surface that must not keep the microphone open. */
  readonly dictationSuspended?: boolean;
  /** Prompts already sent in this thread, oldest first, for ArrowUp recall. */
  readonly sentPrompts?: readonly string[];
  /** Inline chip sources for @ threads/models, # pull requests and $ skills. */
  readonly chipSources?: ChipSources;
  /** Permission mode control; absent hides it. mod+shift+a cycles the modes. */
  readonly permissionControl?: PermissionControl;
  /** Execution mode (Plan / Act / Auto / Accept-Edits); absent hides it. */
  readonly executionModeControl?: ExecutionModeControl;
  /** Custom `--agent` selector; absent hides it. */
  readonly agentControl?: AgentControl;
  /** Thinking level (off..max); absent hides it. */
  readonly thinkingControl?: ThinkingControl;
  /** Fast mode toggle; absent hides it. */
  readonly fastModeControl?: FastModeControl;
  /** `--add-dir` pills; absent hides it. */
  readonly additionalDirsControl?: AdditionalDirsControl;
  /** Steering / follow-up delivery mode; absent hides it. */
  readonly queueModeControl?: QueueModeControl;
  /** Which action the send button takes mid-turn; mod+enter does the opposite. */
  readonly followUpDefault?: FollowUpDefault;
  /** Save a queued message in place after an in-composer edit. */
  readonly onEditQueued?: (id: string, text: string) => void;
  /** Remove one queued message. */
  readonly onRemoveQueued?: (id: string) => void;
  /** Send a queued message now, into the running turn when there is one. */
  readonly onSendQueuedNow?: (id: string) => void;
  /** Interrupt the active turn and run this queued message immediately. */
  readonly onInterruptQueued?: (id: string) => void;
  /** Stop the running turn from the composer when the draft is empty. */
  readonly onInterrupt?: () => void;
}

/** How long a first tap on Stop stays armed on a touch screen before it needs arming again. */
const STOP_ARM_MS = 3000;

interface SlashSpan {
  readonly start: number;
  readonly token: string;
}

function slashTokenAt(value: string, caret: number): SlashSpan | null {
  const at = Number.isFinite(caret) ? Math.min(Math.max(0, Math.trunc(caret)), value.length) : value.length;
  const match = /(?:^|\s)(\/[^\s]*)$/.exec(value.slice(0, at));
  if (!match?.[1]) return null;
  return { start: at - match[1].length, token: match[1] };
}

function slashCommandText(name: string): string {
  const bare = name.replace(/^\/+/, "").trim();
  return bare.length === 0 ? "/" : `/${bare} `;
}

function slashNameMatches(name: string, token: string): boolean {
  const bare = name.replace(/^\/+/, "").toLowerCase();
  const typed = token.replace(/^\/+/, "").toLowerCase();
  return bare.startsWith(typed);
}

/** A held message named in a button's label: enough to tell rows apart, never a whole paragraph. */
function heldName(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > 40 ? `${flat.slice(0, 39)}…` : flat;
}

function coarsePointer(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(pointer: coarse)").matches === true;
}

/** The keyboard shape the textarea produces; React events satisfy it structurally. */
type SurfaceKeyEvent = {
  readonly key: string;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
  readonly keyCode?: number;
  readonly isComposing?: boolean;
  readonly preventDefault: () => void;
};

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Cmd/Ctrl+Shift+V keeps a large paste editable instead of attaching it. A browser's paste
 * event carries no modifier keys and React's synthetic event drops them too, so the chord is
 * read from the keydown that always precedes the paste (and from the event itself when a test
 * or a future browser does report it).
 */
function keepsPasteEditable(event: unknown, chordFromKeydown: boolean): boolean {
  if (chordFromKeydown) return true;
  const parts = [event, (event as { nativeEvent?: unknown } | null)?.nativeEvent];
  for (const part of parts) {
    const chord = part as { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean } | null | undefined;
    if (chord && (chord.metaKey === true || chord.ctrlKey === true) && chord.shiftKey === true) return true;
  }
  return false;
}

export const MobileChatComposer = React.forwardRef<
  MobileChatComposerHandle,
  MobileChatComposerProps
>(({
  value,
  onValueChange,
  draftUnsaved = false,
  attachments = [],
  onAttachFiles,
  showAttachments = true,
  onRemoveAttachment,
  attachmentPreviewUrl,
  attaching = false,
  attachError = null,
  heldMessages = [],
  heldSendingIds = [],
  heldUnsaved = false,
  onEditHeld,
  onSendHeld,
  onRemoveHeld,
  queuedMessages = [],
  onCancelQueued,
  onQueue,
  onSteer,
  promptOpen = false,
  warning = null,
  warningAction = null,
  onSend,
  voiceConversation,
  onStop,
  onClearDraft,
  isRunning = false,
  disabled = false,
  placeholder = "Ask the agent…",
  className,
  composerCapabilities = null,
  onApplyControl,
  controlNotice = null,
  dictationFence = "",
  dictationSuspended = false,
  sentPrompts,
  chipSources,
  permissionControl,
  executionModeControl,
  agentControl,
  thinkingControl,
  fastModeControl,
  additionalDirsControl,
  queueModeControl,
  followUpDefault = "queue",
  onEditQueued,
  onRemoveQueued,
  onSendQueuedNow,
  onInterruptQueued,
  onInterrupt,
}, ref) => {
  const isControlled = value !== undefined;
  const [internalText, setInternalText] = useState("");
  const text = isControlled ? value : internalText;
  const [editingHeldId, setEditingHeldId] = useState<string | null>(null);
  const [editingHeldText, setEditingHeldText] = useState("");
  // Staged files and held rows fold behind count chips so they never push the box off screen.
  const [filesOpen, setFilesOpen] = useState(false);
  const [heldOpen, setHeldOpen] = useState(false);
  const isComposingRef = useRef(false);
  // On a touch screen Stop takes two taps: the first arms it, so a slip never ends a turn.
  const [stopArmed, setStopArmed] = useState(false);
  const stopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
  }, []);
  useEffect(() => {
    if (!isRunning) setStopArmed(false);
  }, [isRunning]);
  const handleStop = useCallback(() => {
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
    if (!coarsePointer() || stopArmed) {
      setStopArmed(false);
      onStop?.();
      return;
    }
    setStopArmed(true);
    stopTimerRef.current = setTimeout(() => setStopArmed(false), STOP_ARM_MS);
  }, [onStop, stopArmed]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textRef = useRef(text);
  textRef.current = text;
  const primaryFollowUpRef = useRef<"queue" | "steer" | null>(null);
  const fenceRef = useRef(dictationFence);
  fenceRef.current = dictationFence;
  const dictationProbe = useMemo(
    () => probeDictation(typeof window === "undefined" ? undefined : window),
    [],
  );
  const dictationRef = useRef<DictationSession | null>(null);
  const partialSpanRef = useRef<{ start: number; end: number } | null>(null);
  const pendingCaretRef = useRef<number | null>(null);
  const [dictating, setDictating] = useState(false);
  /** Whether the keydown right before a paste held the keep-it-editable chord. */
  const pasteChordRef = useRef(false);
  const [caretPos, setCaretPos] = useState(0);
  const [slashIndex, setSlashIndex] = useState(0);
  const [slashDismissed, setSlashDismissed] = useState(false);

const discardDictation = useCallback(() => {
    dictationRef.current?.discard();
    dictationRef.current = null;
    partialSpanRef.current = null;
    setDictating(false);
  }, []);

  useEffect(() => {
    discardDictation();
  }, [dictationFence, discardDictation]);

  useEffect(() => {
    if (dictationSuspended) discardDictation();
  }, [dictationSuspended, discardDictation]);

  useEffect(() => () => {
    dictationRef.current?.discard();
    dictationRef.current = null;
  }, []);

  useLayoutEffect(() => {
    const next = pendingCaretRef.current;
    if (next === null) return;
    pendingCaretRef.current = null;
    const el = textareaRef.current;
    if (!el) return;
    el.setSelectionRange(Math.min(Math.max(0, next), el.value.length), Math.min(Math.max(0, next), el.value.length));
  }, [text]);

  const rememberCaret = useCallback((node: HTMLTextAreaElement | null = textareaRef.current) => {
    if (node === null || typeof node.selectionStart !== "number") return;
    setCaretPos(node.selectionStart);
  }, []);

  const setText = useCallback(
    (next: string) => {
      if (!isControlled) setInternalText(next);
      onValueChange?.(next);
    },
    [isControlled, onValueChange],
  );

  const writeAt = useCallback((start: number, end: number, insertion: string) => {
    if (isComposingRef.current) return;
    const current = textRef.current;
    const next = insertAtCaret(current, start, end, insertion);
    pendingCaretRef.current = next.caret;
    partialSpanRef.current = insertion.length === 0 ? null : { start: Math.min(start, end), end: next.caret };
    setText(next.value);
    setCaretPos(next.caret);
  }, [setText]);

  const adjustHeight = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const nextHeight = Math.min(el.scrollHeight, 144);
    el.style.height = `${Math.max(nextHeight, 36)}px`;
  }, []);

  useEffect(() => {
    adjustHeight();
  }, [text, adjustHeight]);

  const clearDraft = useCallback(() => {
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
    onClearDraft?.();
  }, [setText, onClearDraft]);

  const focus = useCallback(() => {
    textareaRef.current?.focus();
  }, []);

  useImperativeHandle(ref, () => ({
    clearDraft,
    focus,
    insertCitation: (citation: CitationInsert) => {
      const at = textareaRef.current?.selectionStart ?? textRef.current.length;
      partialSpanRef.current = null;
      writeAt(Math.min(at, textRef.current.length), Math.min(at, textRef.current.length), citationMarkdown(citation, ""));
      focus();
    },
  }), [clearDraft, focus, writeAt]);

  /** Where a mention goes: the caret the single input surface reports. */
  const caret = useCallback((): number => {
    const el = textareaRef.current;
    // The element's own selection is authoritative; the tracked position is the fallback for
    // a textarea that is not in the DOM yet.
    return el !== null && typeof el.selectionStart === "number" ? el.selectionStart : caretPos;
  }, [caretPos]);

  const clearSentField = useCallback(() => {
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
  }, [setText]);

  const handleQueue = useCallback(() => {
    if (disabled || attaching || isPromptTooLong(textRef.current)) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    const consumed = onQueue?.(trimmed);
    if (consumed === false) return;
    clearSentField();
  }, [attaching, clearSentField, disabled, onQueue, text]);

  const handleSteer = useCallback(() => {
    if (disabled || attaching || isPromptTooLong(textRef.current)) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    const consumed = onSteer?.(trimmed);
    if (consumed === false) return;
    clearSentField();
  }, [attaching, clearSentField, disabled, onSteer, text]);

  // --- G3 parity: limits, recall, stash, chips, queued edit, permission ---
  const storageRef = useRef<Storage | null>(typeof window === "undefined" ? null : window.localStorage);
  const [stashes, setStashes] = useState<readonly StashedPrompt[]>(() => readStashes(storageRef.current));
  const [stashMenuOpen, setStashMenuOpen] = useState(false);
  const recallRef = useRef<RecallState>(INITIAL_RECALL);
  const editingQueuedRef = useRef<string | null>(null);
  const [editingQueuedId, setEditingQueuedId] = useState<string | null>(null);
  const savedDraftRef = useRef("");
  const [chipMenu, setChipMenu] = useState<{
    trigger: ChipTrigger;
    query: string;
    items: readonly ChipItem[];
    index: number;
  } | null>(null);
  const [citationEdit, setCitationEdit] = useState<{ index: number; comment: string } | null>(null);
  /** The directory typed into the `--add-dir` field, before it becomes a pill. */
  const [newDir, setNewDir] = useState("");

  const activeCaret = caretPos;
  const activeCaretRef = useRef(activeCaret);
  activeCaretRef.current = activeCaret;

  const beginQueuedEdit = useCallback((messageId: string, messageText: string) => {
    savedDraftRef.current = textRef.current;
    editingQueuedRef.current = messageId;
    setEditingQueuedId(messageId);
    recallRef.current = INITIAL_RECALL;
    setText(messageText);
    pendingCaretRef.current = messageText.length;
  }, [setText]);

  const endQueuedEdit = useCallback((restoreDraft: boolean) => {
    const saved = savedDraftRef.current;
    savedDraftRef.current = "";
    editingQueuedRef.current = null;
    setEditingQueuedId(null);
    // While editing, the composer holds the queued text; the previous draft returns after.
    setText(restoreDraft ? saved : textRef.current);
  }, [setText]);

  const commitQueuedEdit = useCallback(() => {
    const id = editingQueuedRef.current;
    if (id === null) return;
    const current = textRef.current;
    const trimmed = current.trim();
    if (trimmed.length > 0) onEditQueued?.(id, trimmed);
    endQueuedEdit(true);
  }, [endQueuedEdit, onEditQueued]);

  // If the queued message being edited starts or is removed, the edit ends: changed content
  // moves into the composer when the previous draft was empty, and is discarded otherwise.
  useEffect(() => {
    const id = editingQueuedRef.current;
    if (id === null) return;
    if (queuedMessages.some((message) => message.id === id)) return;
    const previousEmpty = savedDraftRef.current.trim().length === 0;
    endQueuedEdit(!previousEmpty);
  }, [queuedMessages, endQueuedEdit]);

  const restoreStash = useCallback((entry: StashedPrompt) => {
    recallRef.current = INITIAL_RECALL;
    setText(entry.text);
    pendingCaretRef.current = entry.text.length;
    setStashMenuOpen(false);
    // The stash keeps the draft's attachment NAMES (composer.md:210-212 says a stash holds "the
    // current prompt and its attachments"), but neither surface has a name-to-file route: the host
    // stages bytes the picker handed it, so re-attaching by name would have to guess a path. The
    // text comes back, and the card below reports the files the owner could not restore instead of
    // pretending it did. See the lane's CLAIM for this as a stated non-parity.
  }, [setText]);

  const toggleStash = useCallback(() => {
    const current = textRef.current;
    const names = attachments.map((att) => att.displayName);
    const stashable = stashableDraft(current, names);
    if (stashable !== null) {
      setStashes(saveStash(storageRef.current, stashable));
      clearDraft();
      setStashMenuOpen(false);
      return;
    }
    const existing = readStashes(storageRef.current);
    setStashes(existing);
    if (existing.length === 1) restoreStash(existing[0]);
    else if (existing.length > 1) setStashMenuOpen((open) => !open);
  }, [attachments, clearDraft, restoreStash]);

  const chipTrigger = useMemo(() => {
    if (!chipSources) return null;
    for (const trigger of ["@", "#", "$"] as const) {
      const hit = chipTriggerAt(text, activeCaret, trigger);
      if (hit) return hit;
    }
    return null;
  }, [text, activeCaret, chipSources]);

  useEffect(() => {
    if (chipTrigger === null || !chipSources) {
      setChipMenu(null);
      return;
    }
    const { trigger, query } = chipTrigger;
    let cancelled = false;
    const needle = query.toLowerCase();
    const apply = (items: readonly ChipItem[]) => {
      if (cancelled) return;
      const filtered = items.filter((item) => item.label.toLowerCase().includes(needle));
      setChipMenu({ trigger, query, items: filtered, index: 0 });
    };
    const source =
      trigger === "@" ? chipSources.threads : trigger === "#" ? chipSources.pullRequests : null;
    if (source) {
      setChipMenu({ trigger, query, items: [], index: 0 });
      source(query)
        .then(apply)
        .catch(() => apply([]));
    } else {
      apply(staticChipCandidates(trigger, chipSources));
    }
    return () => {
      cancelled = true;
    };
  }, [chipTrigger, chipSources]);

  const insertChip = useCallback((item: ChipItem) => {
    const menu = chipMenu;
    if (menu === null) return;
    const span = chipTriggerAt(textRef.current, activeCaretRef.current, menu.trigger);
    if (span === null) return;
    partialSpanRef.current = null;
    writeAt(span.start, span.end, `${chipMarkdown(item)} `);
    setChipMenu(null);
  }, [chipMenu, writeAt]);

  /** Citation chips in the draft: readable markdown with the comment as the link title. */
  const citationSpans = useMemo(() => {
    const spans: { start: number; end: number; label: string; preview: string; text: string; messageId: string; comment: string }[] = [];
    // The label is escape-aware: a quote holding `]` or an encoded line break is still one label.
    const pattern = /\[((?:\\.|[^\\\]\n])*)\]\(ferryx:\/\/cite\/([^)\s]+)(?:\s+"([^"]*)")?\)/g;
    let match = pattern.exec(text);
    while (match !== null) {
      // The whole quote is in the label; the chip row shows its first line only.
      const quoted = unescapeMarkdownText(match[1]);
      spans.push({
        start: match.index,
        end: match.index + match[0].length,
        label: match[1],
        preview: quoted.split("\n")[0] ?? "",
        text: quoted,
        messageId: decodeURIComponent(match[2]),
        comment: match[3] ?? "",
      });
      match = pattern.exec(text);
    }
    return spans;
  }, [text]);

  const saveCitationComment = useCallback(() => {
    if (citationEdit === null) return;
    const span = citationSpans[citationEdit.index];
    if (span) {
      const comment = citationEdit.comment.trim();
      const next = citationMarkdown(
        { messageId: span.messageId, text: span.text },
        comment,
      );
      partialSpanRef.current = null;
      writeAt(span.start, span.end, next);
    }
    setCitationEdit(null);
  }, [citationEdit, citationSpans, writeAt]);

  const cyclePermission = useCallback(() => {
    if (!permissionControl || permissionControl.modes.length === 0) return;
    const modes = permissionControl.modes;
    const current = modes.indexOf(permissionControl.current ?? modes[0]);
    permissionControl.onChange(modes[(current + 1) % modes.length] as PermissionMode);
  }, [permissionControl]);


  const handleSend = useCallback(() => {
    // A running pane still takes typed text (that is how a TUI is driven), and a prompt waiting
    // on the original pane takes a typed answer: the owner decides what the text means.
    // a file still staging has no mention yet: sending now would leave it behind
    if (disabled || attaching || isPromptTooLong(textRef.current)) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    if (editingQueuedRef.current !== null) {
      // Enter while a queued message is open in the composer saves it in place.
      commitQueuedEdit();
      return;
    }
    const consumed = onSend(trimmed);
    if (consumed === false) return;
    // The owner keeps the sent text until the host acknowledges it, so a failed send puts it
    // back; the field itself clears the moment the send goes out.
    recallRef.current = INITIAL_RECALL;
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
  }, [commitQueuedEdit, disabled, attaching, text, onSend, setText]);

  const slashSpan = composerCapabilities && composerCapabilities.slashCommands.length > 0
    ? slashTokenAt(text, activeCaret)
    : null;
  const slashOptions = slashSpan === null || composerCapabilities === null
    ? []
    : composerCapabilities.slashCommands.filter((command) => slashNameMatches(command.name, slashSpan.token));
  const slashOpen = slashOptions.length > 0;
  const slashVisible = slashOpen && !slashDismissed;

  const insertSlash = useCallback((name: string) => {
    const span = slashTokenAt(textRef.current, activeCaret);
    if (span === null) return;
    const insertion = slashCommandText(name);
    partialSpanRef.current = null;
    writeAt(span.start, span.start + span.token.length, insertion);
    setSlashIndex(0);
  }, [activeCaret, writeAt]);

  const slashKey = useCallback((event: SurfaceKeyEvent): boolean => {
    if (!slashVisible) return false;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSlashIndex((index) => (index + 1) % slashOptions.length);
      return true;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setSlashIndex((index) => (index - 1 + slashOptions.length) % slashOptions.length);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setSlashDismissed(true);
      return true;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      if (isImeConfirmingKey(event as never, isComposingRef.current)) return false;
      event.preventDefault();
      const chosen = slashOptions[slashIndex] ?? slashOptions[0];
      if (chosen) insertSlash(chosen.name);
      return true;
    }
    return false;
  }, [insertSlash, slashIndex, slashOptions, slashVisible]);

  const applyDictation = useCallback((transcript: string, fence: string) => {
    if (fenceRef.current !== fence || isComposingRef.current) return;
    const current = textRef.current;
    const el = textareaRef.current;
    const span = partialSpanRef.current;
    const start = span ? span.start : (el?.selectionStart ?? current.length);
    const end = span ? span.end : (el?.selectionEnd ?? start);
    writeAt(start, end, transcript);
  }, [writeAt]);

  const toggleDictation = useCallback(() => {
    if (!dictationProbe.available || dictationSuspended) return;
    if (dictationRef.current) {
      dictationRef.current.stop();
      dictationRef.current = null;
      partialSpanRef.current = null;
      setDictating(false);
      return;
    }
    const fence = fenceRef.current;
    const session = dictationProbe.create({
      onPartial: (transcript) => applyDictation(transcript, fence),
      onFinal: (transcript) => {
        applyDictation(transcript, fence);
        partialSpanRef.current = null;
      },
      onError: (error) => {
        if (fenceRef.current !== fence) return;
        if (error.code === "permission-denied" || error.code === "not-allowed") {
          discardDictation();
          return;
        }
        dictationRef.current?.stop();
        dictationRef.current = null;
        setDictating(false);
      },
    });
    dictationRef.current = session;
    setDictating(true);
    session.start(fence);
  }, [applyDictation, dictationProbe, dictationSuspended, discardDictation]);

  const chipKey = useCallback((event: SurfaceKeyEvent): boolean => {
    const menu = chipMenu;
    if (menu === null || menu.items.length === 0) return false;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setChipMenu((current) => current === null ? null : { ...current, index: (current.index + 1) % current.items.length });
      return true;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setChipMenu((current) => current === null ? null : { ...current, index: (current.index - 1 + current.items.length) % current.items.length });
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setChipMenu(null);
      return true;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      if (isImeConfirmingKey(event as never, Boolean(event.isComposing))) return false;
      event.preventDefault();
      const chosen = menu.items[menu.index] ?? menu.items[0];
      if (chosen) insertChip(chosen);
      return true;
    }
    return false;
  }, [chipMenu, insertChip]);

  const attachFilesAt = useCallback((files: readonly File[]) => {
    if (files.length === 0 || onAttachFiles === undefined) return false;
    const at = activeCaret;
    onAttachFiles(files, at);
    return true;
  }, [activeCaret, onAttachFiles]);

  const handlePaste = useCallback(
    (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
      if (disabled) return;
      // The keep-editable chord belongs to this one paste: it is spent here whether the paste
      // becomes an attachment, an image, or nothing at all.
      const keepShortcut = keepsPasteEditable(event, pasteChordRef.current);
      pasteChordRef.current = false;
      const data = event.clipboardData;
      const images = Array.from(data?.files ?? []).filter(isImageFile);
      if (images.length > 0) {
        event.preventDefault();
        attachFilesAt(images);
        return;
      }
      const pasted = data?.getData("text/plain") ?? "";
      if (pasted.length === 0) return;
      const at = caret();
      // A paste over a selection replaces it: the draft it leaves is smaller than the sum, so the
      // limit check must count only the text the selection gave back (composer.md:8-13).
      const selection = textareaRef.current;
      const replaced =
        selection !== null && selection.selectionEnd > selection.selectionStart
          ? selection.selectionEnd - selection.selectionStart
          : 0;
      if (decideTextPaste(pasted, textRef.current.length, at, keepShortcut, replaced).attach) {
        event.preventDefault();
        onAttachFiles?.([pasteTextFile(pasted)], at);
        return;
      }
      if (keepShortcut) {
        // Pasting a large text as text: the controlled draft takes it in here, so the box holds
        // exactly the same text whether or not the browser inserts into a mirrored value.
        event.preventDefault();
        writeAt(at, at, pasted);
      }
    },
    [attachFilesAt, caret, disabled, onAttachFiles, writeAt],
  );

  const handleDrop = useCallback(
    (event: React.DragEvent<HTMLTextAreaElement>) => {
      const images = Array.from(event.dataTransfer?.files ?? []).filter(isImageFile);
      if (images.length === 0) return;
      event.preventDefault();
      attachFilesAt(images);
    },
    [attachFilesAt],
  );

  const caretAtStart = useCallback((): boolean => {
    const el = textareaRef.current;
    return (el !== null && typeof el.selectionStart === "number" ? el.selectionStart : caretPos) === 0;
  }, [caretPos]);

  const applyRecall = useCallback((next: RecallState, direction: "up" | "down") => {
    recallRef.current = next;
    setText(next.recalled);
    pendingCaretRef.current = direction === "up" ? 0 : next.recalled.length;
  }, [setText]);

  /** The composer's one keydown pipeline, on the single textarea surface. */
  const surfaceKeyDown = useCallback(
    (event: SurfaceKeyEvent): boolean => {
      const mod = event.metaKey || event.ctrlKey;
      // Read by the paste handler: a paste event itself carries no modifiers. Only the paste
      // chord sets it, so mod+shift+A and the other shortcuts cannot leak into a later paste.
      if (event.key === "v" || event.key === "V") pasteChordRef.current = mod && event.shiftKey;
      if (mod && !event.shiftKey && (event.key === "s" || event.key === "S")) {
        event.preventDefault();
        toggleStash();
        return true;
      }
      if (mod && event.shiftKey && (event.key === "A" || event.key === "a")) {
        event.preventDefault();
        cyclePermission();
        return true;
      }
      if (mod && event.shiftKey && (event.key === "D" || event.key === "d")) {
        event.preventDefault();
        toggleDictation();
        return true;
      }
      if (mod && event.shiftKey && event.key === "Enter") {
        if (isImeConfirmingKey(event as never, Boolean(event.isComposing))) return false;
        event.preventDefault();
        const oldest = queuedMessages.find((message) => message.phase === "queued") ?? queuedMessages[0];
        if (oldest && onSendQueuedNow) onSendQueuedNow(oldest.id);
        return true;
      }
      if (mod && event.key === "Enter") {
        if (isImeConfirmingKey(event as never, Boolean(event.isComposing))) return false;
        event.preventDefault();
        // mod+enter is the opposite of the button's action; no follow-up is a plain send.
        if (primaryFollowUpRef.current === "queue") handleSteer();
        else if (primaryFollowUpRef.current === "steer") handleQueue();
        else handleSend();
        return true;
      }
      if (slashKey(event)) return true;
      if (chipKey(event)) return true;
      if (event.altKey && event.key === "ArrowUp") {
        // At the start of the composer, edit the most recently queued message.
        if (caretAtStart() && queuedMessages.length > 0 && onEditQueued !== undefined) {
          event.preventDefault();
          const newest = queuedMessages[queuedMessages.length - 1];
          beginQueuedEdit(newest.id, newest.text);
          return true;
        }
        return false;
      }
      if (!mod && !event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
        const direction = event.key === "ArrowUp" ? "up" : "down";
        const next = stepRecall({
          state: recallRef.current,
          prompts: sentPrompts ?? [],
          direction,
          composerEmpty: textRef.current.length === 0 && attachments.length === 0,
          text: textRef.current,
          caret: activeCaret,
        });
        if (next !== null) {
          event.preventDefault();
          applyRecall(next, direction);
          return true;
        }
        return false;
      }
      if (event.key === "Enter" && !event.shiftKey && !mod) {
        if (isImeConfirmingKey(event as never, Boolean(event.isComposing))) return false;
        // Enter sends, from a list or a quote too (composer.md:31 "Enter sends. In a list or
        // quote, Shift+Enter continues it"). A plain textarea has no code block to protect, so
        // Enter is always the send.
        event.preventDefault();
        if (primaryFollowUpRef.current === "queue") handleQueue();
        else if (primaryFollowUpRef.current === "steer") handleSteer();
        else handleSend();
        return true;
      }
      return false;
    },
    [
      applyRecall,
      attachments.length,
      beginQueuedEdit,
      caretAtStart,
      caretPos,
      chipKey,
      cyclePermission,
      handleQueue,
      handleSend,
      handleSteer,
      onEditQueued,
      onSendQueuedNow,
      queuedMessages,
      sentPrompts,
      slashKey,
      toggleDictation,
      toggleStash,
    ],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      // An IME composition owns the keyboard: Enter there confirms a candidate, never a send.
      if (isComposingRef.current) return;
      surfaceKeyDown(e);
    },
    [surfaceKeyDown],
  );

  const handleCompositionStart = useCallback(() => {
    isComposingRef.current = true;
  }, []);

  const handleCompositionEnd = useCallback(() => {
    isComposingRef.current = false;
  }, []);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (!files || files.length === 0) return;
      const chosen = Array.from(files);
      // read the caret before the input takes the focus
      const at = caret();
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
      onAttachFiles?.(chosen, at);
    },
    [caret, onAttachFiles],
  );

  const beginHeldEdit = useCallback((message: HeldMessage) => {
    setEditingHeldId(message.id);
    setEditingHeldText(message.text);
  }, []);

  const commitHeldEdit = useCallback(() => {
    if (editingHeldId === null) return;
    const trimmed = editingHeldText.trim();
    if (trimmed.length > 0) onEditHeld?.(editingHeldId, trimmed);
    setEditingHeldId(null);
    setEditingHeldText("");
  }, [editingHeldId, editingHeldText, onEditHeld]);

  const canSubmit = text.trim().length > 0 && !disabled && !attaching && !isPromptTooLong(text);
  // A prompt waiting on the original pane is a question this box answers, so the box keeps Send
  // beside Stop while one is up. Queue and steer appear only while the agent is working, the
  // draft is nonempty, and that target currently advertises the action.
  const answersWaitingPrompt = promptOpen;
  const followUp = !answersWaitingPrompt && isRunning && canSubmit ? composerCapabilities?.followUp : null;
  const offersQueue = followUp?.queue === true;
  const offersSteer = followUp?.steer === true;
  // With both advertised, the send button takes the user's follow-up default; mod+enter flips it.
  const primaryFollowUp = offersQueue && offersSteer
    ? followUpDefault
    : offersQueue ? "queue" : offersSteer ? "steer" : null;
  primaryFollowUpRef.current = primaryFollowUp;
  const showsSend = primaryFollowUp === null && (!isRunning || answersWaitingPrompt);
  const showsInterrupt = !answersWaitingPrompt && isRunning && onInterrupt !== undefined && text.trim().length === 0;
  const textareaLabel = placeholder.replace(/…$/, "");
  const modelChoices = composerCapabilities?.models?.choices ?? [];
  const selectedModel = modelChoices.find((choice) => choice.id === composerCapabilities?.models?.current) ?? null;
  const effortChoices = selectedModel?.efforts.filter((effort) => effort.length > 0) ?? [];
  const optionControls = (composerCapabilities?.options ?? []).filter(
    (option) => option.apply !== "start-only" && option.values.length > 0,
  );
  const showsModel = composerCapabilities?.models != null && modelChoices.length > 0;
  const showsEffort = showsModel && effortChoices.length > 0;
  const showsControls = showsModel || showsEffort || optionControls.length > 0;
  const hasExtras =
    draftUnsaved ||
    warning !== null ||
    attachError !== null ||
    isPromptTooLong(text) ||
    stashMenuOpen ||
    attachments.length > 0 ||
    heldMessages.length > 0 ||
    queuedMessages.length > 0;
  const fileCountLabel = `${attachments.length} ${attachments.length === 1 ? "file" : "files"}`;
  const heldCountLabel = `${heldMessages.length} held`;

  return (
    <div
      data-testid="mobile-chat-composer"
      className={cn(
        "mx-auto mb-2 flex w-[calc(100%-1.5rem)] max-w-[60rem] flex-col rounded-xl border border-chat-composer-border bg-chat-composer-panel pb-safe select-none",
        className
      )}
    >
      {hasExtras && (
      // Banners, files and held rows scroll on their own, so the message box always stays on
      // screen above the keyboard.
      <div
        data-testid="chat-composer-extras"
        className="max-h-[45dvh] overflow-y-auto overscroll-contain scrollbar-sleek"
      >
      {draftUnsaved && (
        <div
          data-testid="chat-composer-draft-unsaved"
          role="status"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-danger"
        >
          <AlertCircle aria-hidden="true" className="size-3.5 shrink-0" />
          <span>This draft could not be saved on this device. It stays here until you send it.</span>
        </div>
      )}
      {warning !== null && (
        <div
          data-testid="chat-composer-warning"
          role="status"
          aria-live="polite"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-foreground-secondary"
        >
          <span className="min-w-0 flex-1">{warning}</span>
          {warningAction !== null && (
            <button
              type="button"
              data-testid="chat-composer-warning-action"
              onClick={warningAction.onAction}
              className="flex min-h-11 shrink-0 items-center rounded-md px-3 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {warningAction.label}
            </button>
          )}
        </div>
      )}
      {attachError !== null && (
        <div
          data-testid="chat-composer-attach-error"
          role="alert"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-danger"
        >
          <span>{attachError}</span>
        </div>
      )}
      {isPromptTooLong(text) && (
        <p
          data-testid="chat-composer-length-error"
          role="alert"
          className="px-3 pt-2 pb-1 text-xs text-chat-danger"
        >
          {promptLengthMessage(text)}
        </p>
      )}
      {(attachments.length > 0 || heldMessages.length > 0) && (
        <div className="flex items-center gap-2 px-3 pt-2 pb-1">
          {attachments.length > 0 && (
            <button
              type="button"
              data-testid="chat-composer-files-toggle"
              aria-expanded={filesOpen}
              aria-controls="chat-composer-attachments"
              onClick={() => setFilesOpen((open) => !open)}
              className="flex min-h-11 items-center gap-1.5 rounded-full border border-chat-border bg-chat-surface px-3 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <FileText aria-hidden="true" className="size-4" />
              <span>{fileCountLabel}</span>
              <ChevronDown aria-hidden="true" className={cn("size-4 transition-transform motion-reduce:transition-none", filesOpen && "rotate-180")} />
            </button>
          )}
          {heldMessages.length > 0 && (
            <button
              type="button"
              data-testid="chat-composer-held-toggle"
              aria-expanded={heldOpen || editingHeldId !== null}
              aria-controls="chat-composer-held"
              onClick={() => setHeldOpen((open) => !open)}
              className={cn(
                "flex min-h-11 items-center gap-1.5 rounded-full border border-chat-border bg-chat-surface px-3 text-sm transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                heldUnsaved ? "text-chat-danger" : "text-chat-foreground",
              )}
            >
              <span>{heldCountLabel}</span>
              <ChevronDown aria-hidden="true" className={cn("size-4 transition-transform motion-reduce:transition-none", (heldOpen || editingHeldId !== null) && "rotate-180")} />
            </button>
          )}
        </div>
      )}
      {attachments.length > 0 && (
        <div
          id="chat-composer-attachments"
          data-testid="chat-composer-attachments"
          hidden={!filesOpen}
          className={cn("flex items-center gap-2 px-3 pt-2 pb-1 overflow-x-auto scrollbar-none", !filesOpen && "hidden")}
        >
          {attachments.map((att) => {
            const id = att.receipt.attachmentId;
            const url = att.receipt.mediaType.startsWith("image/") ? (attachmentPreviewUrl?.(att) ?? null) : null;
            const isImage = att.receipt.mediaType.startsWith("image/");
            return (
              <div
                key={id}
                data-testid={`attachment-preview-${id}`}
                className={cn(
                  "group relative flex shrink-0",
                  isImage
                    ? "size-16 overflow-hidden rounded-lg border border-chat-border bg-chat-surface"
                    : "max-w-[15rem] items-center gap-2 rounded-lg border border-chat-border bg-chat-surface p-1.5 pr-1",
                )}
              >
                {isImage && url !== null ? (
                  <img
                    data-testid={`attachment-thumbnail-${id}`}
                    src={url}
                    alt={att.displayName}
                    loading="lazy"
                    className="size-full object-cover"
                  />
                ) : isImage ? (
                  <div className="flex size-full items-center justify-center px-1 text-center text-xs text-chat-foreground-secondary">
                    {att.displayName}
                  </div>
                ) : (
                  <>
                    <div className="flex size-8 items-center justify-center rounded bg-chat-surface-raised text-chat-foreground-secondary">
                      <FileText className="size-4" aria-hidden="true" />
                    </div>
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-xs font-mono font-medium text-chat-foreground">
                        {att.displayName}
                      </span>
                      <span className="truncate text-xs font-mono text-chat-foreground-secondary">
                        {formatFileSize(att.receipt.sizeBytes)} · {att.mentionText.trim()}
                      </span>
                    </div>
                  </>
                )}
                <button
                  type="button"
                  data-testid={`remove-attachment-${id}`}
                  aria-label={"Remove staged file " + att.displayName}
                  onClick={() => onRemoveAttachment?.(id)}
                  className={cn(
                    "flex shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                    isImage ? "absolute right-1 top-1 size-6 bg-chat-screen/80" : "size-11",
                  )}
                >
                  <X className="size-4" aria-hidden="true" />
                </button>
              </div>
            );
          })}
        </div>
      )}
      {queuedMessages.length > 0 && (
        <div data-testid="queued-follow-up-list" className="flex flex-col gap-1 px-3 pt-2 pb-1 border-b border-chat-border">
          {queuedMessages.map((message) => {
            const waiting = message.phase === "queued";
            const editingThis = editingQueuedId === message.id;
            const rowLabel = heldName(message.text);
            return (
              <div
                key={message.id}
                data-testid={`queued-follow-up-${message.id}`}
                className={cn("flex items-center gap-2 rounded-lg border py-0.5 pl-2 pr-0.5", editingThis ? "border-chat-foreground-secondary" : "border-transparent")}
              >
                <span className="min-w-0 flex-1 truncate text-xs text-chat-foreground">{message.text}</span>
                <span className="shrink-0 text-xs text-chat-foreground-secondary">
                  {message.phase === "unknown" ? "Not confirmed" : message.phase === "delivering" ? "Sending" : "Queued"}
                </span>
                <span className="flex shrink-0 items-center gap-0.5">
                  {editingThis ? (
                    <>
                      <button
                        type="button"
                        data-testid={`queued-follow-up-save-${message.id}`}
                        aria-label={"Save queued message " + rowLabel}
                        disabled={!waiting}
                        onClick={commitQueuedEdit}
                        className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      >
                        <Check aria-hidden="true" className="size-4" />
                      </button>
                      <button
                        type="button"
                        data-testid={`queued-follow-up-cancel-edit-${message.id}`}
                        aria-label={"Cancel editing queued message " + rowLabel}
                        onClick={() => endQueuedEdit(true)}
                        className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      >
                        <X aria-hidden="true" className="size-4" />
                      </button>
                    </>
                  ) : (
                    <>
                      {waiting && onSendQueuedNow && (
                        <button
                          type="button"
                          data-testid={`queued-follow-up-send-now-${message.id}`}
                          aria-label={"Send queued message now " + rowLabel}
                          onClick={() => onSendQueuedNow(message.id)}
                          className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                        >
                          <CornerDownRight aria-hidden="true" className="size-4" />
                        </button>
                      )}
                      {waiting && onInterruptQueued && (
                        <button
                          type="button"
                          data-testid={`queued-follow-up-interrupt-${message.id}`}
                          aria-label={"Interrupt for queued message " + rowLabel}
                          onClick={() => onInterruptQueued(message.id)}
                          className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                        >
                          <Square aria-hidden="true" className="size-3.5" />
                        </button>
                      )}
                      {waiting && onEditQueued && (
                        <button
                          type="button"
                          data-testid={`queued-follow-up-edit-${message.id}`}
                          aria-label={"Edit queued message " + rowLabel}
                          onClick={() => beginQueuedEdit(message.id, message.text)}
                          className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                        >
                          <Pencil aria-hidden="true" className="size-4" />
                        </button>
                      )}
                      {waiting && onRemoveQueued && (
                        <button
                          type="button"
                          data-testid={`queued-follow-up-remove-${message.id}`}
                          aria-label={"Remove queued message " + rowLabel}
                          onClick={() => onRemoveQueued(message.id)}
                          className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                        >
                          <Trash2 aria-hidden="true" className="size-4" />
                        </button>
                      )}
                    </>
                  )}
                  <button
                    type="button"
                    data-testid={`queued-follow-up-cancel-${message.id}`}
                    aria-label={"Cancel queued message " + rowLabel}
                    disabled={!waiting}
                    onClick={() => onCancelQueued?.(message.id)}
                    className="ml-2 flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <X aria-hidden="true" className="size-4" />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      )}
      {heldMessages.length > 0 && (
        <div
          id="chat-composer-held"
          data-testid="chat-composer-held"
          hidden={!heldOpen && editingHeldId === null}
          role="group"
          aria-label="Held messages"
          className={cn("flex flex-col gap-1 px-3 pt-2 pb-1 border-b border-chat-border", !heldOpen && editingHeldId === null && "hidden")}
        >
          <span className="text-xs text-chat-foreground-secondary">
            {heldUnsaved
              ? "Held here — this device could not save them. Send or remove one yourself."
              : "Held until you send them. Nothing is sent on its own."}
          </span>
          {heldMessages.map((message) => {
            const sending = heldSendingIds.includes(message.id);
            return (
              <div
                key={message.id}
                data-testid={`held-message-${message.id}`}
                className="flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface py-0.5 pl-3 pr-0.5"
              >
                {editingHeldId === message.id ? (
                  <input
                    data-testid={`held-message-input-${message.id}`}
                    aria-label={"Edit held message " + heldName(message.text)}
                    className="min-h-11 min-w-0 flex-1 rounded-md border border-chat-border bg-chat-screen px-2 py-1 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    value={editingHeldText}
                    disabled={sending}
                    onChange={(event) => setEditingHeldText(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !isImeConfirmingKey(event, false)) {
                        event.preventDefault();
                        commitHeldEdit();
                      }
                    }}
                  />
                ) : (
                  <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-sm text-chat-foreground">
                    {message.text}
                  </span>
                )}
                <span className="flex shrink-0 items-center gap-0.5">
                  <button
                    type="button"
                    data-testid={`held-message-send-${message.id}`}
                    aria-label={"Send held message " + heldName(message.text)}
                    disabled={sending || editingHeldId === message.id}
                    onClick={() => onSendHeld?.(message.id)}
                    className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {sending ? (
                      <Loader2 aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
                    ) : (
                      <ArrowUp aria-hidden="true" className="size-4" />
                    )}
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-edit-${message.id}`}
                    aria-label={"Edit held message " + heldName(message.text)}
                    disabled={sending}
                    onClick={() => (editingHeldId === message.id ? commitHeldEdit() : beginHeldEdit(message))}
                    className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Pencil aria-hidden="true" className="size-4" />
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-remove-${message.id}`}
                    aria-label={"Remove held message " + heldName(message.text)}
                    disabled={sending}
                    onClick={() => onRemoveHeld?.(message.id)}
                    className="ml-2 flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Trash2 aria-hidden="true" className="size-4" />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      )}
      </div>
      )}

      {/* One card, text first under it: single-surface wrapper + control row ported from paseo composer/input/input.tsx (Apache-2.0) and t3code ThreadComposer.tsx (MIT). */}
      <div className="flex flex-col px-3 py-2">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          aria-label="Attach files"
          className="hidden"
          onChange={handleFileChange}
          data-testid="file-upload-input"
        />

        <div className="rounded-[20px] border border-chat-composer-border bg-chat-composer-surface transition-colors focus-within:border-chat-foreground-secondary/40">
          {showsControls && (
            <div data-testid="composer-controls" className="flex flex-wrap items-end gap-2 px-2 pt-2">
              {showsModel && composerCapabilities?.models && (
                <label className="flex min-w-0 flex-col gap-0.5 text-xs text-chat-foreground-secondary">
                  <span id="composer-model-label">Model</span>
                  <select
                    data-testid="composer-model-control"
                    aria-labelledby="composer-model-label"
                    value={composerCapabilities.models.current ?? ""}
                    onChange={(event) => {
                      const choice = modelChoices.find((item) => item.id === event.currentTarget.value);
                      if (!choice || !composerCapabilities.models) return;
                      onApplyControl?.({
                        controlId: REFERENCE_CONTROL_MODEL_ID,
                        valueId: choice.id,
                        label: choice.label,
                        apply: composerCapabilities.models.apply,
                      });
                    }}
                    className="min-h-11 max-w-full rounded-md border border-chat-border bg-chat-surface px-2 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="" disabled>Model</option>
                    {modelChoices.map((choice) => (
                      <option key={choice.id} value={choice.id}>{choice.label}</option>
                    ))}
                  </select>
                </label>
              )}
              {showsEffort && composerCapabilities?.models && (
                <label className="flex min-w-0 flex-col gap-0.5 text-xs text-chat-foreground-secondary">
                  <span id="composer-effort-label">Effort</span>
                  <select
                    data-testid="composer-effort-control"
                    aria-labelledby="composer-effort-label"
                    value=""
                    onChange={(event) => {
                      const valueId = event.currentTarget.value;
                      if (!valueId || !composerCapabilities.models) return;
                      onApplyControl?.({
                        controlId: REFERENCE_CONTROL_EFFORT_ID,
                        valueId,
                        label: valueId,
                        apply: composerCapabilities.models.apply,
                      });
                    }}
                    className="min-h-11 max-w-full rounded-md border border-chat-border bg-chat-surface px-2 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="" disabled>Effort</option>
                    {effortChoices.map((effort) => (
                      <option key={effort} value={effort}>{effort}</option>
                    ))}
                  </select>
                </label>
              )}
              {optionControls.map((option) => (
                <label key={option.id} className="flex min-w-0 flex-col gap-0.5 text-xs text-chat-foreground-secondary">
                  <span id={`composer-option-label-${option.id}`}>{option.label}</span>
                  <select
                    data-testid={`composer-option-${option.id}`}
                    aria-labelledby={`composer-option-label-${option.id}`}
                    value={option.current ?? ""}
                    onChange={(event) => {
                      const valueId = event.currentTarget.value;
                      if (!valueId) return;
                      onApplyControl?.({
                        controlId: option.id,
                        valueId,
                        label: valueId,
                        apply: option.apply,
                      });
                    }}
                    className="min-h-11 max-w-full rounded-md border border-chat-border bg-chat-surface px-2 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="" disabled>{option.label}</option>
                    {option.values.map((value) => (
                      <option key={value} value={value}>{value}</option>
                    ))}
                  </select>
                </label>
              ))}
            </div>
          )}
          {controlNotice !== null && (
            <p
              data-testid="composer-control-notice"
              data-status={controlNotice.status}
              role="status"
              aria-live="polite"
              className="px-3 pt-2 text-xs text-chat-foreground-secondary"
            >
              {controlNotice.message}
            </p>
          )}
          {slashVisible && (
            <div
              id="composer-slash-list"
              data-testid="composer-slash-list"
              role="listbox"
              aria-label="Slash commands"
              className="flex flex-col gap-1 px-2 pt-2"
            >
              {slashOptions.map((command, index) => (
                <button
                  key={command.name}
                  type="button"
                  role="option"
                  data-testid={`composer-slash-option-${command.name.replace(/^\/+/, "")}`}
                  aria-selected={index === slashIndex}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => insertSlash(command.name)}
                  className="flex min-h-11 items-center gap-2 rounded-md px-2 text-left text-sm text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <span className="font-medium">{slashCommandText(command.name).trim()}</span>
                  {command.description ? (
                    <span className="truncate text-chat-foreground-secondary">{command.description}</span>
                  ) : null}
                </button>
              ))}
            </div>
          )}
          {chipMenu !== null && chipMenu.items.length > 0 && (
            <div
              id="composer-chip-menu"
              data-testid="composer-chip-menu"
              role="listbox"
              aria-label="Context to attach"
              className="flex flex-col gap-1 px-2 pt-2"
            >
              {chipMenu.items.map((item, index) => (
                <button
                  key={item.id}
                  type="button"
                  role="option"
                  data-testid={`composer-chip-option-${chipMenu.trigger}-${index}`}
                  aria-selected={index === chipMenu.index}
                  aria-label={item.detail ? `${item.label}, ${item.detail}` : item.label}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => insertChip(item)}
                  className="flex min-h-11 items-center gap-2 rounded-md px-2 text-left text-sm text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <span className="font-medium">{item.label}</span>
                  {item.detail ? <span className="truncate text-chat-foreground-secondary">{item.detail}</span> : null}
                </button>
              ))}
            </div>
          )}
          {stashMenuOpen && stashes.length > 0 && (
            <div
              data-testid="composer-stash-menu"
              role="menu"
              aria-label="Stashed prompts"
              className="flex flex-col gap-1 px-2 pt-2"
            >
              {stashes.map((entry) => (
                <div key={entry.id} role="none" className="flex items-center gap-1">
                  <button
                    type="button"
                    role="menuitem"
                    data-testid={`composer-stash-restore-${entry.id}`}
                    onClick={() => restoreStash(entry)}
                    className="flex min-h-11 min-w-0 flex-1 items-center gap-2 rounded-md px-2 text-left text-sm text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <span className="min-w-0 flex-1 truncate">{heldName(entry.text)}</span>
                    {entry.attachmentNames.length > 0 && (
                      <span className="shrink-0 text-xs text-chat-foreground-secondary">
                        {entry.attachmentNames.length} {entry.attachmentNames.length === 1 ? "file" : "files"}
                      </span>
                    )}
                  </button>
                  <button
                    type="button"
                    data-testid={`composer-stash-remove-${entry.id}`}
                    aria-label={"Remove stashed prompt " + heldName(entry.text)}
                    onClick={() => setStashes(removeStash(storageRef.current, entry.id))}
                    className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Trash2 aria-hidden="true" className="size-4" />
                  </button>
                </div>
              ))}
            </div>
          )}
          {(permissionControl != null && permissionControl.modes.length > 0) ||
          executionModeControl !== undefined ||
          agentControl !== undefined ||
          thinkingControl !== undefined ||
          fastModeControl !== undefined ||
          additionalDirsControl !== undefined ||
          queueModeControl !== undefined ? (
            <div
              data-testid="composer-harness-controls"
              className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-2 text-xs text-chat-foreground-secondary"
            >
              {executionModeControl !== undefined && executionModeControl.modes.length > 0 && (
                <label className="flex items-center gap-1">
                  <span id="composer-execution-mode-label">Mode</span>
                  <select
                    data-testid="composer-execution-mode-control"
                    aria-labelledby="composer-execution-mode-label"
                    value={executionModeControl.current ?? ""}
                    onChange={(event) => {
                      if (event.currentTarget.value) executionModeControl.onChange(event.currentTarget.value);
                    }}
                    className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {executionModeControl.current === null && (
                      <option value="" disabled>Mode</option>
                    )}
                    {executionModeControl.modes.map((mode) => (
                      <option key={mode} value={mode}>{mode}</option>
                    ))}
                  </select>
                </label>
              )}
              {agentControl !== undefined && agentControl.agents.length > 0 && (
                <label className="flex items-center gap-1">
                  <span id="composer-agent-label">Agent</span>
                  <select
                    data-testid="composer-agent-control"
                    aria-labelledby="composer-agent-label"
                    value={agentControl.current ?? ""}
                    onChange={(event) => {
                      if (event.currentTarget.value) agentControl.onChange(event.currentTarget.value);
                    }}
                    className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {agentControl.current === null && (
                      <option value="" disabled>Agent</option>
                    )}
                    {agentControl.agents.map((agent) => (
                      <option key={agent} value={agent}>{agent}</option>
                    ))}
                  </select>
                </label>
              )}
              {thinkingControl !== undefined && thinkingControl.levels.length > 0 && (
                <label className="flex items-center gap-1">
                  <span id="composer-thinking-label">Thinking</span>
                  <select
                    data-testid="composer-thinking-control"
                    aria-labelledby="composer-thinking-label"
                    value={thinkingControl.current ?? ""}
                    onChange={(event) => {
                      if (event.currentTarget.value) thinkingControl.onChange(event.currentTarget.value);
                    }}
                    className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {thinkingControl.current === null && (
                      <option value="" disabled>Thinking</option>
                    )}
                    {thinkingControl.levels.map((level) => (
                      <option key={level} value={level}>{level}</option>
                    ))}
                  </select>
                </label>
              )}
              {fastModeControl !== undefined && (
                <button
                  type="button"
                  data-testid="composer-fast-mode-toggle"
                  aria-pressed={fastModeControl.enabled}
                  onClick={() => fastModeControl.onChange(!fastModeControl.enabled)}
                  className={cn(
                    "flex min-h-8 items-center rounded-md border px-2 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                    fastModeControl.enabled
                      ? "border-chat-primary bg-chat-primary/10 text-chat-foreground"
                      : "border-chat-border bg-chat-surface text-chat-foreground-secondary hover:bg-chat-surface-hover",
                  )}
                >
                  Fast
                </button>
              )}
              {queueModeControl !== undefined && (
                <span
                  data-testid="composer-queue-mode-control"
                  role="group"
                  aria-label="Follow-up delivery"
                  className="flex items-center gap-2"
                >
                  <label className="flex items-center gap-1">
                    <span id="composer-steering-mode-label">Steer</span>
                    <select
                      data-testid="composer-steering-mode-control"
                      aria-labelledby="composer-steering-mode-label"
                      value={queueModeControl.steeringMode}
                      onChange={(event) => queueModeControl.onChangeSteering(event.currentTarget.value as QueueDeliveryMode)}
                      className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <option value="all">all</option>
                      <option value="one-at-a-time">one-at-a-time</option>
                    </select>
                  </label>
                  <label className="flex items-center gap-1">
                    <span id="composer-followup-mode-label">Queue</span>
                    <select
                      data-testid="composer-followup-mode-control"
                      aria-labelledby="composer-followup-mode-label"
                      value={queueModeControl.followUpMode}
                      onChange={(event) => queueModeControl.onChangeFollowUp(event.currentTarget.value as QueueDeliveryMode)}
                      className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <option value="all">all</option>
                      <option value="one-at-a-time">one-at-a-time</option>
                    </select>
                  </label>
                </span>
              )}
              {permissionControl != null && permissionControl.modes.length > 0 && (
                <label className="flex items-center gap-1">
                  <span id="composer-permission-label">Permission</span>
                  <select
                    data-testid="composer-permission-control"
                    aria-labelledby="composer-permission-label"
                    value={permissionControl.current ?? ""}
                    onChange={(event) => {
                      if (event.currentTarget.value) permissionControl.onChange(event.currentTarget.value as PermissionMode);
                    }}
                    className="min-h-8 rounded-md border border-chat-border bg-chat-surface px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {permissionControl.modes.map((mode) => (
                      <option key={mode} value={mode}>{mode}</option>
                    ))}
                  </select>
                </label>
              )}
            </div>
          ) : null}
          {additionalDirsControl !== undefined && (
            <div
              data-testid="composer-additional-dirs"
              role="group"
              aria-label="Additional directories"
              className="flex flex-wrap items-center gap-1 px-3 pt-1 text-xs text-chat-foreground-secondary"
            >
              {additionalDirsControl.dirs.map((dir, index) => (
                <span
                  key={`${dir}-${index}`}
                  data-testid={`composer-additional-dir-${index}`}
                  className="flex items-center gap-1 rounded-full border border-chat-border bg-chat-surface pl-2 pr-1 text-xs text-chat-foreground"
                >
                  <span className="max-w-[12rem] truncate font-mono">{dir}</span>
                  <button
                    type="button"
                    data-testid={`composer-additional-dir-remove-${index}`}
                    aria-label={`Remove additional directory ${dir}`}
                    onClick={() => additionalDirsControl.onRemove(dir)}
                    className="flex size-8 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary transition-colors hover:text-chat-danger focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <X aria-hidden="true" className="size-3.5" />
                  </button>
                </span>
              ))}
              <input
                data-testid="composer-additional-dir-input"
                aria-label="Add an additional directory"
                placeholder="Add a directory…"
                value={newDir}
                onChange={(event) => setNewDir(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !isImeConfirmingKey(event, false)) {
                    event.preventDefault();
                    const trimmed = newDir.trim();
                    if (trimmed.length > 0) additionalDirsControl.onAdd(trimmed);
                    setNewDir("");
                  }
                }}
                className="min-h-8 min-w-[8rem] flex-1 rounded-md border border-chat-border bg-chat-surface px-1 font-mono text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              />
              <button
                type="button"
                data-testid="composer-additional-dir-add"
                aria-label="Add directory"
                onClick={() => {
                  const trimmed = newDir.trim();
                  if (trimmed.length === 0) return;
                  additionalDirsControl.onAdd(trimmed);
                  setNewDir("");
                }}
                className="flex size-8 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <Plus aria-hidden="true" className="size-3.5" />
              </button>
            </div>
          )}
          <textarea
            ref={textareaRef}
            data-testid="chat-composer-textarea"
            rows={1}
            value={text}
            disabled={disabled}
            placeholder={placeholder}
            aria-label={textareaLabel}
            aria-controls={slashVisible ? "composer-slash-list" : undefined}
            aria-expanded={slashVisible || undefined}
            aria-autocomplete={slashVisible ? "list" : undefined}
            enterKeyHint={coarsePointer() ? "enter" : "send"}
            autoComplete="off"
            inputMode="text"
            onChange={(event) => {
              partialSpanRef.current = null;
              setSlashDismissed(false);
              recallRef.current = recallRef.current.index >= 0 && event.target.value === recallRef.current.recalled
                ? recallRef.current
                : INITIAL_RECALL;
              setText(event.target.value);
              rememberCaret(event.currentTarget);
            }}
            onSelect={(event) => rememberCaret(event.currentTarget)}
            onKeyUp={(event) => rememberCaret(event.currentTarget)}
            onClick={(event) => rememberCaret(event.currentTarget)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            onDrop={handleDrop}
            onDragOver={(event) => event.preventDefault()}
            onCompositionStart={handleCompositionStart}
            onCompositionEnd={handleCompositionEnd}
            className="min-h-11 w-full resize-none bg-transparent px-3.5 pt-2.5 font-sans text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring max-h-36 overflow-y-auto leading-relaxed scrollbar-sleek"
          />
          {citationSpans.length > 0 && (
            <div data-testid="composer-citations" className="flex flex-col gap-1 px-3 pt-1">
              {citationSpans.map((span, index) => (
                <div
                  key={`${span.start}-${span.messageId}`}
                  data-testid={`composer-citation-${index}`}
                  className="flex items-center gap-1 rounded-lg border border-chat-border bg-chat-surface px-2 py-0.5"
                >
                  <Quote aria-hidden="true" className="size-3.5 shrink-0 text-chat-foreground-secondary" />
                  <span className="min-w-0 flex-1 truncate text-xs text-chat-foreground">
                    {citationEdit !== null && citationEdit.index === index ? citationEdit.comment : span.comment.length > 0 ? span.comment : span.preview}
                  </span>
                  {citationEdit !== null && citationEdit.index === index ? (
                    <input
                      data-testid={`citation-comment-input-${index}`}
                      aria-label="Citation comment"
                      autoFocus
                      value={citationEdit.comment}
                      onChange={(event) => setCitationEdit({ index, comment: event.currentTarget.value })}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" && !isImeConfirmingKey(event, false)) {
                          event.preventDefault();
                          saveCitationComment();
                        }
                        if (event.key === "Escape") setCitationEdit(null);
                      }}
                      className="min-h-8 min-w-0 flex-1 rounded-md border border-chat-border bg-chat-screen px-1 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    />
                  ) : null}
                  {citationEdit !== null && citationEdit.index === index ? (
                    <button
                      type="button"
                      data-testid={`citation-comment-save-${index}`}
                      aria-label="Save citation comment"
                      onClick={saveCitationComment}
                      className="flex size-8 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <Check aria-hidden="true" className="size-3.5" />
                    </button>
                  ) : (
                    <button
                      type="button"
                      data-testid={`citation-comment-edit-${index}`}
                      aria-label={span.comment.length > 0 ? "Edit citation comment" : "Add citation comment"}
                      onClick={() => setCitationEdit({ index, comment: span.comment })}
                      className="flex size-8 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <Pencil aria-hidden="true" className="size-3.5" />
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* Stop sits at the left edge, Send at the right: a slip never turns an answer into a stop */}
          <div className="flex items-center justify-between gap-3 px-1 pb-1">
            <div className="flex items-center gap-1">
              {showAttachments ? <button
                type="button"
                data-testid="attach-file-button"
                aria-label="Attach file"
                title="Attach file"
                aria-busy={attaching || undefined}
                disabled={disabled || attaching}
                onClick={() => fileInputRef.current?.click()}
                className="flex size-11 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary hover:text-chat-foreground active:bg-chat-surface-raised transition-colors disabled:opacity-40 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <Plus aria-hidden="true" className="size-5" />
              </button> : null}
              {dictationProbe.available && (
                <button
                  type="button"
                  data-testid="dictation-mic-button"
                  aria-label={dictating ? "Stop dictation" : "Start dictation"}
                  aria-pressed={dictating}
                  title={dictating ? "Stop dictation" : "Start dictation"}
                  disabled={disabled || dictationSuspended}
                  onClick={toggleDictation}
                  className="flex size-11 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary hover:text-chat-foreground active:bg-chat-surface-raised transition-colors disabled:opacity-40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Mic aria-hidden="true" className="size-5" />
                </button>
              )}
              {voiceConversation ? <VoiceConversationButton {...voiceConversation} /> : null}

              {isRunning && (
                <button
                  type="button"
                  data-testid="stop-button"
                  aria-label={stopArmed ? "Tap again to stop the running turn" : "Stop the running turn"}
                  title="Stop the running turn"
                  onClick={handleStop}
                  className="flex h-11 shrink-0 items-center gap-1.5 rounded-full bg-chat-danger px-4 text-sm font-medium text-chat-screen hover:bg-chat-danger/90 active:bg-chat-danger/80 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Square aria-hidden="true" className="size-3.5 fill-current" />
                  <span aria-hidden="true">{stopArmed ? "Tap to confirm" : "Stop"}</span>
                </button>
              )}
              {showsInterrupt && (
                <button
                  type="button"
                  data-testid="interrupt-button"
                  aria-label="Interrupt the running turn"
                  title="Interrupt the running turn"
                  onClick={() => onInterrupt?.()}
                  className="flex h-11 shrink-0 items-center gap-1.5 rounded-full border border-chat-border px-4 text-sm font-medium text-chat-foreground hover:bg-chat-surface-hover transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Square aria-hidden="true" className="size-3.5 fill-current" />
                  <span>Interrupt</span>
                </button>
              )}
              {offersQueue && offersSteer && primaryFollowUp === "queue" && (
                <button
                  type="button"
                  data-testid="steer-button"
                  aria-label="Steer the running turn now"
                  title="Steer the running turn now"
                  disabled={!canSubmit}
                  onClick={handleSteer}
                  className="flex h-11 shrink-0 items-center rounded-full px-3 text-sm font-medium text-chat-foreground hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
                >
                  Steer
                </button>
              )}
            </div>

            {showsSend && (
              <button
                type="button"
                data-testid="send-button"
                aria-label="Send message"
                title="Send message"
                disabled={!canSubmit}
                onClick={handleSend}
                className={cn(
                  "flex size-11 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                  canSubmit
                    ? "bg-chat-primary text-chat-primary-foreground hover:bg-chat-primary/90"
                    : "bg-chat-surface-raised text-chat-foreground-secondary cursor-not-allowed"
                )}
              >
                <ArrowUp aria-hidden="true" className="size-5 stroke-[2.5]" />
              </button>
            )}
            {primaryFollowUp === "queue" && (
              <button
                type="button"
                data-testid="queue-follow-up-button"
                aria-label="Queue message to send when the turn ends"
                title="Queue message to send when the turn ends"
                disabled={!canSubmit}
                onClick={handleQueue}
                className="flex h-11 shrink-0 items-center gap-1.5 rounded-full bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground hover:bg-chat-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
              >
                <ListPlus aria-hidden="true" className="size-4" />
                <span>Queue</span>
              </button>
            )}
            {primaryFollowUp === "steer" && (
              <button
                type="button"
                data-testid="steer-button"
                aria-label="Steer the running turn now"
                title="Steer the running turn now"
                disabled={!canSubmit}
                onClick={handleSteer}
                className="flex h-11 shrink-0 items-center gap-1.5 rounded-full bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground hover:bg-chat-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
              >
                <CornerDownRight aria-hidden="true" className="size-4" />
                <span>Steer</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});

MobileChatComposer.displayName = "MobileChatComposer";
