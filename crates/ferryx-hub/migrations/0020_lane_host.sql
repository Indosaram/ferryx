-- The paired host a lane's pane runs on. NULL is this machine.
ALTER TABLE lanes ADD COLUMN host_id TEXT;
