CREATE TABLE subject_work_schedules (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    weekday INTEGER NOT NULL CHECK (weekday BETWEEN 0 AND 6),
    is_workday INTEGER NOT NULL CHECK (is_workday IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    UNIQUE (workspace_id, subject_id, weekday)
);
CREATE INDEX idx_subject_work_schedules_subject ON subject_work_schedules(workspace_id, subject_id, weekday);

CREATE TABLE subject_work_schedule_segments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    schedule_id TEXT NOT NULL REFERENCES subject_work_schedules(id) ON DELETE CASCADE,
    sequence_no INTEGER NOT NULL CHECK (sequence_no > 0),
    start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute INTEGER NOT NULL CHECK (end_minute BETWEEN 1 AND 1440),
    CHECK (start_minute < end_minute),
    UNIQUE (schedule_id, sequence_no)
);
CREATE INDEX idx_subject_work_schedule_segments_schedule ON subject_work_schedule_segments(workspace_id, schedule_id, sequence_no);

WITH RECURSIVE weekdays(value) AS (
    SELECT 0 UNION ALL SELECT value + 1 FROM weekdays WHERE value < 6
)
INSERT INTO subject_work_schedules(id, workspace_id, subject_id, weekday, is_workday, created_at, updated_at, version)
SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-7' || substr(lower(hex(randomblob(2))), 2) || '-a' || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
       subjects.workspace_id, subjects.id, weekdays.value, CASE WHEN weekdays.value < 5 THEN 1 ELSE 0 END,
       subjects.created_at, subjects.updated_at, 1
FROM subjects CROSS JOIN weekdays
WHERE subjects.deleted_at IS NULL;

INSERT INTO subject_work_schedule_segments(id, workspace_id, schedule_id, sequence_no, start_minute, end_minute)
SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-7' || substr(lower(hex(randomblob(2))), 2) || '-a' || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
       workspace_id, id, 1, 600, 720
FROM subject_work_schedules WHERE weekday < 5;
INSERT INTO subject_work_schedule_segments(id, workspace_id, schedule_id, sequence_no, start_minute, end_minute)
SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-7' || substr(lower(hex(randomblob(2))), 2) || '-a' || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
       workspace_id, id, 2, 810, 1050
FROM subject_work_schedules WHERE weekday < 5;
