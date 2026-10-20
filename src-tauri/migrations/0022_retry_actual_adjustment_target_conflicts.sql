UPDATE sync_outbox
SET state = 'pending',
    last_attempt_at = NULL,
    error_json = NULL,
    recovery_kind = NULL,
    recovery_context_json = NULL,
    recovery_updated_at = NULL
WHERE entity_type = 'actual_adjustment_batch'
  AND state = 'conflict'
  AND json_extract(error_json, '$.message') LIKE '%ADJUSTMENT_BASELINE_CONFLICT%';
