CREATE INDEX IF NOT EXISTS idx_reports_list_subject_updated
ON reports(workspace_id, subject_id, updated_at DESC, id DESC)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_reports_list_subject_type_updated
ON reports(workspace_id, subject_id, report_type, updated_at DESC, id DESC)
WHERE deleted_at IS NULL;
