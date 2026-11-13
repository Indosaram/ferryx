-- The version of the owner notice an obligation stands for: a hash of the gate verdict
-- (salience) and the bot's body, never of the evidence (quote, sender, incident). A changed
-- version re-issues the notice under a new report id; an evidence-only rewrite does not.
-- No backfill: a past row's `body` is the rendered outbox text (it carries the relayed
-- evidence), not the gate body, and its verdict was never stored, so no past row maps to one
-- version unambiguously. Past rows stay NULL; a NULL version is adopted, never reissued.
ALTER TABLE delivery_obligations ADD COLUMN version TEXT;
