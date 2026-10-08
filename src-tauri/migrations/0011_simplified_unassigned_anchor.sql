-- Active unassigned time is now a single anchor. Historical terminal rows and
-- already generated entries are intentionally preserved.
DELETE FROM unassigned_segments
WHERE session_id IN (
    SELECT id FROM unassigned_sessions
    WHERE state IN ('collecting', 'awaiting_resolution')
)
AND EXISTS (
    SELECT 1
    FROM unassigned_segments active_anchor
    WHERE active_anchor.session_id = unassigned_segments.session_id
      AND active_anchor.ended_at IS NULL
)
AND id NOT IN (
    SELECT open_segment.id
    FROM unassigned_segments open_segment
    JOIN unassigned_sessions session ON session.id = open_segment.session_id
    WHERE session.state IN ('collecting', 'awaiting_resolution')
      AND open_segment.ended_at IS NULL
);

UPDATE unassigned_segments
SET sequence_no = 1, duration_seconds = 0
WHERE ended_at IS NULL
  AND session_id IN (
      SELECT id FROM unassigned_sessions
      WHERE state IN ('collecting', 'awaiting_resolution')
  );

UPDATE unassigned_sessions
SET first_started_at = (
        SELECT started_at FROM unassigned_segments
        WHERE session_id = unassigned_sessions.id AND ended_at IS NULL
        LIMIT 1
    ),
    duration_seconds = 0,
    last_ended_at = NULL,
    prompted_at = NULL,
    state = 'collecting',
    updated_at = COALESCE((
        SELECT started_at FROM unassigned_segments
        WHERE session_id = unassigned_sessions.id AND ended_at IS NULL
        LIMIT 1
    ), updated_at),
    last_continuous_at = COALESCE((
        SELECT started_at FROM unassigned_segments
        WHERE session_id = unassigned_sessions.id AND ended_at IS NULL
        LIMIT 1
    ), last_continuous_at)
WHERE state IN ('collecting', 'awaiting_resolution')
  AND EXISTS (
      SELECT 1 FROM unassigned_segments
      WHERE session_id = unassigned_sessions.id AND ended_at IS NULL
  );

UPDATE unassigned_sessions
SET state = 'discarded',
    resolution_type = 'discard',
    resolved_at = COALESCE(last_ended_at, updated_at, first_started_at),
    duration_seconds = 0,
    prompted_at = NULL,
    migration_state = 'superseded',
    version = version + 1
WHERE state IN ('collecting', 'awaiting_resolution')
  AND EXISTS (
      SELECT 1
      FROM time_entries timer
      WHERE timer.workspace_id = unassigned_sessions.workspace_id
        AND timer.work_date = unassigned_sessions.work_date
        AND timer.source_type = 'timer'
        AND timer.state IN ('running', 'paused')
        AND timer.deleted_at IS NULL
  )
  AND NOT EXISTS (
      SELECT 1 FROM unassigned_segments
      WHERE session_id = unassigned_sessions.id AND ended_at IS NULL
  );

-- Lifecycle images from the old pause/resume model must never replay after the
-- anchor migration. Rows already being sent are left for normal reconciliation.
DELETE FROM sync_outbox
WHERE entity_type = 'unassigned_session'
  AND operation_type IN (
      'unassigned_session_pause',
      'unassigned_session_resume',
      'unassigned_session_awaiting_resolution'
  )
  AND state IN ('pending', 'failed', 'conflict');
