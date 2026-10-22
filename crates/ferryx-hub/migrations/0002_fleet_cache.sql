-- Last successful agents.list per fleet host, so a hung host can still be shown as stale.
CREATE TABLE IF NOT EXISTS fleet_inventory_cache (
    host_id TEXT PRIMARY KEY,
    sessions TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
