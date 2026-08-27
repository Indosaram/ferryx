"use strict";
// ─── Explicit agent status (reported via native agent hooks → IPC) ──────────
// Why: status comes from hooks (Claude, Codex, etc.) — never inferred from terminal titles;
// a narrow interrupt fallback synthesizes a final `done` when an agent misses its cancellation hook.
Object.defineProperty(exports, "__esModule", { value: true });
exports.AGENT_STATUS_JSON_STRUCTURE_LIMITS = exports.AGENT_STATUS_MAX_SUBAGENTS = exports.AGENT_MODEL_MAX_LENGTH = exports.AGENT_TYPE_MAX_LENGTH = exports.AGENT_STATUS_STALE_AFTER_MS = exports.AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH = exports.AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH = exports.AGENT_STATUS_TOOL_INPUT_MAX_LENGTH = exports.AGENT_STATUS_TOOL_NAME_MAX_LENGTH = exports.AGENT_STATE_HISTORY_MAX = exports.AGENT_STATUS_STATES = exports.AGENT_STATUS_MAX_FIELD_LENGTH = void 0;
exports.pickParsedAgentStatusPayload = pickParsedAgentStatusPayload;
exports.isFreshNonDoneAgentStatus = isFreshNonDoneAgentStatus;
exports.agentSubagentsEqual = agentSubagentsEqual;
exports.normalizeAgentStatusPayload = normalizeAgentStatusPayload;
exports.parseAgentStatusPayload = parseAgentStatusPayload;
const agent_status_field_normalization_1 = require("./agent-status-field-normalization");
const json_text_structure_limit_1 = require("./json-text-structure-limit");
var agent_status_field_normalization_2 = require("./agent-status-field-normalization");
Object.defineProperty(exports, "AGENT_STATUS_MAX_FIELD_LENGTH", { enumerable: true, get: function () { return agent_status_field_normalization_2.AGENT_STATUS_MAX_FIELD_LENGTH; } });
exports.AGENT_STATUS_STATES = ['working', 'blocked', 'waiting', 'done'];
/** Maximum number of history entries kept per agent to bound memory. */
exports.AGENT_STATE_HISTORY_MAX = 20;
/**
 * Narrow an `AgentStatusIpcPayload` (or any superset) down to the status fields alone.
 * Why: the IPC shape is flattened, so a spread cannot be narrowed structurally — copying
 * a hook row into a client-visible projection would otherwise ship `launchToken`,
 * `connectionId`, `promptInteractionKey` and `providerSessionOnly` to every paired client.
 */
function pickParsedAgentStatusPayload(row) {
    return {
        state: row.state,
        prompt: row.prompt,
        ...(row.agentType !== undefined ? { agentType: row.agentType } : {}),
        ...(row.model !== undefined ? { model: row.model } : {}),
        ...(row.toolName !== undefined ? { toolName: row.toolName } : {}),
        ...(row.toolInput !== undefined ? { toolInput: row.toolInput } : {}),
        ...(row.interactivePrompt !== undefined ? { interactivePrompt: row.interactivePrompt } : {}),
        ...(row.lastAssistantMessage !== undefined
            ? { lastAssistantMessage: row.lastAssistantMessage }
            : {}),
        ...(row.interrupted !== undefined ? { interrupted: row.interrupted } : {}),
        ...(row.sessionBoundary !== undefined ? { sessionBoundary: row.sessionBoundary } : {}),
        ...(row.subagents !== undefined ? { subagents: row.subagents } : {})
    };
}
/** Maximum character length for the toolName field. */
exports.AGENT_STATUS_TOOL_NAME_MAX_LENGTH = 60;
/** Maximum character length for the toolInput preview. */
exports.AGENT_STATUS_TOOL_INPUT_MAX_LENGTH = 160;
/** Maximum character length for the lastAssistantMessage preview.
 *  Why: 8 KB fits a multi-paragraph summary while bounding per-pane cache against a buggy/malicious agent spamming huge strings. */
exports.AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH = 8000;
/** Maximum character length for the interactivePrompt field.
 *  Why: holds full AskUserQuestion JSON — truncating to a preview like toolInput would corrupt it and drop options; capped to still bound cache growth. */
exports.AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH = 16000;
/**
 * Freshness threshold for explicit agent status: retained past this so WorktreeCard's
 * sidebar dot can decay "working" back to "active" when the hook stream goes silent.
 */
exports.AGENT_STATUS_STALE_AFTER_MS = 30 * 60 * 1000;
function isFreshNonDoneAgentStatus(entry, now = Date.now(), staleAfterMs = exports.AGENT_STATUS_STALE_AFTER_MS) {
    // Why: an unconfirmed hydrated row may describe a turn that ended while no receiver was up; never fresh.
    return Boolean(entry &&
        entry.state !== 'done' &&
        entry.restoredUnconfirmed !== true &&
        now - entry.updatedAt <= staleAfterMs);
}
// Why: ReadonlySet<string> so .has() accepts any string without a cast here; the narrowing cast stays on the return line where it's proven safe.
const VALID_STATES = new Set(exports.AGENT_STATUS_STATES);
/** Maximum character length for the agentType label. Truncated on parse. */
exports.AGENT_TYPE_MAX_LENGTH = 40;
exports.AGENT_MODEL_MAX_LENGTH = 120;
/** Maximum subagent child rows carried per status entry. Bounds per-pane cache
 *  and IPC fanout against a runaway spawner. */
exports.AGENT_STATUS_MAX_SUBAGENTS = 32;
exports.AGENT_STATUS_JSON_STRUCTURE_LIMITS = {
    structuralTokens: 4096,
    nestingDepth: 16
};
const AGENT_SUBAGENT_ID_MAX_LENGTH = 64;
function normalizeSubagentSnapshot(value) {
    if (typeof value !== 'object' || value === null) {
        return null;
    }
    const obj = value;
    if (typeof obj.id !== 'string') {
        return null;
    }
    const id = obj.id.trim();
    if (id.length === 0 || id.length > AGENT_SUBAGENT_ID_MAX_LENGTH) {
        return null;
    }
    if (obj.state !== 'working' &&
        obj.state !== 'blocked' &&
        obj.state !== 'waiting' &&
        obj.state !== 'idle') {
        return null;
    }
    return {
        id,
        state: obj.state,
        startedAt: typeof obj.startedAt === 'number' && Number.isFinite(obj.startedAt) ? obj.startedAt : 0,
        agentType: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.agentType, exports.AGENT_TYPE_MAX_LENGTH),
        model: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.model, exports.AGENT_MODEL_MAX_LENGTH),
        description: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.description, exports.AGENT_STATUS_TOOL_INPUT_MAX_LENGTH)
    };
}
function normalizeSubagentsField(value) {
    if (!Array.isArray(value) || value.length === 0) {
        return undefined;
    }
    const normalized = [];
    for (const item of value) {
        const snapshot = normalizeSubagentSnapshot(item);
        if (snapshot) {
            normalized.push(snapshot);
            if (normalized.length >= exports.AGENT_STATUS_MAX_SUBAGENTS) {
                break;
            }
        }
    }
    return normalized.length > 0 ? normalized : undefined;
}
/** Structural equality for subagent lists so stores can reuse the previous
 *  array reference (and skip fanout) when nothing actually changed. */
function agentSubagentsEqual(a, b) {
    if (a === b) {
        return true;
    }
    if (!a || !b || a.length !== b.length) {
        return !a && !b;
    }
    for (let i = 0; i < a.length; i++) {
        const x = a[i];
        const y = b[i];
        if (x.id !== y.id ||
            x.state !== y.state ||
            x.startedAt !== y.startedAt ||
            x.agentType !== y.agentType ||
            x.model !== y.model ||
            x.description !== y.description) {
            return false;
        }
    }
    return true;
}
/**
 * Normalize and validate an already-parsed agent status object. Shared by the
 * JSON string entry point (`parseAgentStatusPayload`) and the object entry
 * point (`normalizeAgentStatusPayload`) so both paths enforce identical field
 * rules. Returns null when the payload is malformed or the state is invalid.
 */
function normalizeAgentStatusObject(parsed) {
    if (typeof parsed !== 'object' || parsed === null) {
        return null;
    }
    const obj = parsed;
    // Why: explicit typeof guard rejects non-string values instead of leaning on Set.has to return false for mismatched types.
    if (typeof obj.state !== 'string') {
        return null;
    }
    const state = obj.state;
    if (!VALID_STATES.has(state)) {
        return null;
    }
    return {
        state: state,
        prompt: (0, agent_status_field_normalization_1.normalizePromptField)(obj.prompt),
        // Why: normalize like the other single-line fields so embedded newlines (e.g. `agentType: "claude\nrogue"`) can't break single-line UI and equality checks.
        agentType: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.agentType, exports.AGENT_TYPE_MAX_LENGTH),
        model: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.model, exports.AGENT_MODEL_MAX_LENGTH),
        toolName: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.toolName, exports.AGENT_STATUS_TOOL_NAME_MAX_LENGTH),
        toolInput: (0, agent_status_field_normalization_1.normalizeOptionalField)(obj.toolInput, exports.AGENT_STATUS_TOOL_INPUT_MAX_LENGTH),
        interactivePrompt: (0, agent_status_field_normalization_1.normalizeInteractivePromptField)(obj.interactivePrompt, exports.AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH),
        lastAssistantMessage: (0, agent_status_field_normalization_1.normalizeOptionalMultilineField)(obj.lastAssistantMessage, exports.AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH),
        // Why: only meaningful on `done`; coerce to undefined elsewhere so it can't leak stale truth across transitions.
        interrupted: obj.interrupted === true && state === 'done' ? true : undefined,
        sessionBoundary: obj.sessionBoundary === true && state === 'done' ? true : undefined,
        subagents: normalizeSubagentsField(obj.subagents)
    };
}
/**
 * Normalize an already-structured agent status object (e.g. from IPC, already
 * deserialized by Electron). Skips the JSON round-trip parseAgentStatusPayload
 * needs — hook events can fire many times per second during a tool-use run.
 */
function normalizeAgentStatusPayload(payload) {
    return normalizeAgentStatusObject(payload);
}
/**
 * Parse and validate an agent status JSON payload received from explicit
 * hook integrations or OSC 9999. Returns null if the payload is malformed or
 * has an invalid state.
 */
function parseAgentStatusPayload(json) {
    try {
        (0, json_text_structure_limit_1.assertJsonTextStructureWithinLimits)(json, exports.AGENT_STATUS_JSON_STRUCTURE_LIMITS);
        return normalizeAgentStatusObject(JSON.parse(json));
    }
    catch {
        return null;
    }
}
