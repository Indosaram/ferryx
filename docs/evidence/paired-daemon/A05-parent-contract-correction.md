# A05 private catalog contract ownership

The first A05 handoff is not accepted: only two evidence reports were created.
Parent `git status` and diff inspection confirm no source or test candidate.
There is no RED, GREEN, restart or permissions proof yet.

The parent read both complete `remote/machine_protocol.rs` and
`remote/machine_protocol_lifecycle.rs`. Contrary to the original delegation
instruction, A02 supplies public project inventory and proxy persistence value
types, not a versioned private catalog or exposure envelope. Requiring reuse of
that nonexistent envelope caused the prerequisite stop.

The corrected source ownership is the plan's existing
`remote/workspace_catalog.rs`: define the private schema-version-1 envelope and
exposure metadata there, reusing `Project`, `Availability` and `Epoch` where
appropriate. Do not duplicate public wire response DTOs or change their contract.
No edit to machine protocol files is needed for this correction.

All original A05 obligations remain: actual failing isolated restart first;
durability before publication; canonical alias/concurrent registration;
write-failure rollback; unavailable rows; corrupt/newer preservation; private
permissions; reserved IDs; common daemon mutation gate; and independent runtime
verification with teardown. Existing atomic writer behavior must be checked,
including parent-directory sync failure semantics, not assumed correct by reuse.
Any necessary strict catalog writer stays within catalog scope rather than
changing unrelated authentication persistence.

The parent attempted `send` on the completed A05 node. The tool refused revival,
so the same Wave 1 run was amended with these corrections for A05 and verify-A05.
Previously accepted A04 nodes are unchanged and must not rerun. A05 remains open.
