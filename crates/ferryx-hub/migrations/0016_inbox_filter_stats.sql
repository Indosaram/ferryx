-- omo-gateway 0034: inbox prefilter drops counted per KST day, source and rule.
CREATE TABLE IF NOT EXISTS inbox_filter_stats (
    day TEXT NOT NULL,
    source TEXT NOT NULL,
    rule TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, source, rule)
);
