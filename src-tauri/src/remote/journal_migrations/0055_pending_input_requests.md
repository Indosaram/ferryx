# Journal schema step 55 — pending user-input requests

## Which store, and why it is not a SQL migration

T2 lands in the **daemon's JSON machine journal**
(`src-tauri/src/remote/machine_operation_journal.rs`, file `machine-operations.v1.json`), not in
`crates/ferryx-hub/migrations/`.

That follows the plan's placement rule (§3 T2, "Where it lives"): the pending request belongs to
the **daemon's** journal, never the hub. The hub runs only inside the daemon and is gated by lease
role and the `hub_stopped` flag (`hub_runtime.rs:1441`, `:2146`), so a prompt recorded there would
be unavailable on a Standby machine or while the hub is stopped — exactly when the desktop still
needs to show and answer the card. The daemon journal is the store that is always open, always the
same file, and already owns the per-session agent stamps a restarted daemon continues
(`machine_operation_journal.rs` `MachineSession::agent_stamps`).

There is consequently **no `0055_*.sql` in `crates/ferryx-hub/migrations/`**, and no number was
consumed or renumbered there: the highest committed hub migration is still `0053`. The plan assigns
`0055` to T2's *pending input requests*; this file is that step, expressed in the store T2 is
required to use. If a hub-side read model is ever wanted it is a projection of this table, never
the source of truth.

## The step

| Field | Added by | Default when absent |
|---|---|---|
| `Store.schema: u32` | step 55 | `0` (a journal written before this change) |
| `Store.pendingInputs: BTreeMap<String, PendingInput>` | step 55 | empty map |

`PENDING_INPUT_STEP = 55` is `pending_input::JOURNAL_SCHEMA_STEP`.

## How the step is applied

A JSON journal has no `ALTER TABLE`, so the step is not a file rewrite:

1. `Store` derives `Deserialize` and both new fields carry `#[serde(default)]`, so a pre-55 file
   parses unchanged — the compatibility rule the plan states for every new journal field.
2. `lift_schema(&mut store)` sets `schema = max(schema, 55)` **in memory** at load
   (`MachineOperationJournal::open` and `refresh_locked`). Opening a journal is therefore never a
   write: a read-only consumer sees the lifted view and the file on disk is untouched.
3. The next ordinary `persist` writes the file out with `"schema":55` and the `pendingInputs` map.
   There is no separate migration pass and no window in which the daemon must run one.

An unknown `version` (currently anything but `1`) is still refused with
`MACHINE_SERVICE_UNAVAILABLE`, as before; `schema` is additive to that and carries no refusal.

`pending_input_tests.rs::a_journal_written_before_step_55_reads_as_an_empty_pending_table` is the
receipt for this: it writes a literal pre-55 file, opens it, asserts an empty pending table, then
asserts the first write stamps `"schema":55` and keeps the card.

## Field compatibility

Every field on `PendingInput`, `PendingInputAttempt`, `PendingInputResolution` and
`PendingInputOption` is either required at step 55 or carries `#[serde(default)]`:

* `PendingInput` — `question`, `options`, `kind`, `needs_confirmation`, `last_refusal`, `attempt`,
  `resolution` default; `prompt_id`, `session_id`, `screen_revision`, `opened_at` are required.
* `PendingInputAttempt` — `answer` defaults (an attempt recorded before the answer was stored can
  still be read; it simply cannot be re-delivered by the restart path).
* `PendingInputResolution` — `receipt` defaults (a `gone` resolution has none).

All four types are `deny_unknown_fields`, like the rest of the journal: a field from a newer build
is a refusal, not a silent drop, which is the journal's existing rule.

## Retention

`update_pending_input` prunes resolved cards older than 7 days on every write. A card that is still
open is never pruned — retention must never discard a question an agent is still blocked on.
