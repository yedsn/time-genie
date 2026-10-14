ALTER TABLE sync_outbox ADD COLUMN recovery_kind TEXT;
ALTER TABLE sync_outbox ADD COLUMN recovery_context_json TEXT;
ALTER TABLE sync_outbox ADD COLUMN recovery_updated_at INTEGER;

CREATE INDEX idx_sync_outbox_timer_recovery
    ON sync_outbox(workspace_id, entity_type, entity_id, recovery_kind, state);

UPDATE sync_outbox
SET recovery_kind = 'historical_candidate',
    recovery_context_json = error_json,
    recovery_updated_at = COALESCE(last_attempt_at, created_at)
WHERE entity_type = 'time_entry'
  AND (error_json LIKE '%IDEMPOTENCY_CONFLICT%' OR error_json LIKE '%SYNC_CONFLICT%');
