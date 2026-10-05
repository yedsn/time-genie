-- Runtime unassigned sessions are device-local. Only their finalized time entry
-- is synchronized after the user confirms work, break, or discard.
DELETE FROM sync_outbox
WHERE entity_type = 'unassigned_session';

UPDATE sync_outbox
SET payload_json = json_set(payload_json, '$.origin_unassigned_session_id', NULL)
WHERE entity_type = 'time_entry'
  AND json_valid(payload_json)
  AND json_extract(payload_json, '$.origin_unassigned_session_id') IS NOT NULL;

UPDATE local_sync_state
SET last_error = NULL
WHERE last_error LIKE '%unassigned session%'
   OR last_error LIKE '%unassigned_session%';
