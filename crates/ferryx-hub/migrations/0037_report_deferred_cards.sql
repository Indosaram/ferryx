-- Preserve an approval card and its receipt while owner rules defer delivery.
ALTER TABLE chat_outbox ADD COLUMN report_not_before TEXT;
