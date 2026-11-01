-- A claimed mutation names the call that holds it: a token per call, the owning process's
-- instance id and pid. Whether the claim still runs comes from that owner, never from the
-- clock: claim_until only lets status report a stalled owner. An owner records its outcome
-- by token, so a late receipt is never dropped.
ALTER TABLE report_mutations ADD COLUMN owner_token TEXT;
ALTER TABLE report_mutations ADD COLUMN owner_instance TEXT;
ALTER TABLE report_mutations ADD COLUMN owner_pid INTEGER;
ALTER TABLE report_mutations ADD COLUMN claim_until TEXT;
