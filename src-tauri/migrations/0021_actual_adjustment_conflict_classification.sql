UPDATE sync_outbox
SET state = 'conflict',
    last_attempt_at = COALESCE(last_attempt_at, created_at)
WHERE entity_type = 'actual_adjustment_batch'
  AND state IN ('pending', 'failed', 'sending')
  AND json_extract(error_json, '$.message') LIKE '%ADJUSTMENT_BASELINE_CONFLICT%';
