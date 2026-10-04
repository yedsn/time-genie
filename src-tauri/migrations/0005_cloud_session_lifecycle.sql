ALTER TABLE local_sync_state ADD COLUMN auth_blocked INTEGER NOT NULL DEFAULT 0 CHECK (auth_blocked IN (0, 1));
ALTER TABLE local_sync_state ADD COLUMN auth_blocked_reason TEXT;
ALTER TABLE local_sync_state ADD COLUMN auth_blocked_at INTEGER;

