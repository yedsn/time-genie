-- Keep only the newest unresolved anchor in each workspace. Older unresolved
-- sessions are system-discarded and never become time entries.
UPDATE unassigned_segments
SET ended_at = COALESCE(ended_at, started_at),
    duration_seconds = 0,
    lease_token = NULL
WHERE session_id IN (
    SELECT older.id
    FROM unassigned_sessions older
    WHERE older.state IN ('collecting', 'awaiting_resolution')
      AND EXISTS (
          SELECT 1
          FROM unassigned_sessions newer
          WHERE newer.workspace_id = older.workspace_id
            AND newer.state IN ('collecting', 'awaiting_resolution')
            AND (
                newer.work_date > older.work_date
                OR (newer.work_date = older.work_date AND newer.first_started_at > older.first_started_at)
                OR (newer.work_date = older.work_date AND newer.first_started_at = older.first_started_at AND newer.id > older.id)
            )
      )
);

UPDATE unassigned_sessions
SET state = 'discarded',
    resolution_type = 'discard',
    generated_entry_id = NULL,
    duration_seconds = 0,
    prompted_at = NULL,
    last_ended_at = MAX(first_started_at, COALESCE(last_ended_at, updated_at, first_started_at)),
    resolved_at = MAX(first_started_at, COALESCE(last_ended_at, updated_at, first_started_at)),
    migration_state = 'superseded',
    version = version + 1
WHERE state IN ('collecting', 'awaiting_resolution')
  AND EXISTS (
      SELECT 1
      FROM unassigned_sessions newer
      WHERE newer.workspace_id = unassigned_sessions.workspace_id
        AND newer.state IN ('collecting', 'awaiting_resolution')
        AND (
            newer.work_date > unassigned_sessions.work_date
            OR (newer.work_date = unassigned_sessions.work_date AND newer.first_started_at > unassigned_sessions.first_started_at)
            OR (newer.work_date = unassigned_sessions.work_date AND newer.first_started_at = unassigned_sessions.first_started_at AND newer.id > unassigned_sessions.id)
        )
  );

DELETE FROM sync_outbox
WHERE entity_type = 'unassigned_session'
  AND state IN ('pending', 'failed', 'conflict')
  AND entity_id IN (
      SELECT id FROM unassigned_sessions WHERE migration_state = 'superseded'
  );
