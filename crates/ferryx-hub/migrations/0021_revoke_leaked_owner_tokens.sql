-- Builds before 2026.1028.1 posted each one-time cron owner token in plain text to the Slack
-- home channel, so every approval hash minted so far is public. Clear them all; owners approve
-- with `ferryx hub cron approve-rerun <id>` over the control socket instead. The stored chat
-- copies of those tokens are redacted by `cron_remediate::redact_stored_owner_tokens` at every
-- hub start.
UPDATE cron_problems
   SET detail = json_remove(detail, '$.approval_hash')
 WHERE json_valid(detail)
   AND json_type(detail, '$.approval_hash') IS NOT NULL;
