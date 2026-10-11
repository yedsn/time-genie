ALTER TABLE report_templates ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE report_templates ADD COLUMN description TEXT;
ALTER TABLE report_templates ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1));
ALTER TABLE report_templates ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

DROP INDEX IF EXISTS uq_report_templates_global;
DROP INDEX IF EXISTS uq_report_templates_subject;

-- Give every legacy template a stable, user-facing name before removing subject scope.
UPDATE report_templates
SET name = CASE report_type
    WHEN 'daily' THEN '标准日报'
    WHEN 'weekly' THEN '标准周报'
    ELSE '标准月报'
  END
WHERE subject_id IS NULL;

UPDATE report_templates AS template
SET name = (
      SELECT trim(subject.name) || ' - ' || CASE template.report_type
        WHEN 'daily' THEN '日报'
        WHEN 'weekly' THEN '周报'
        ELSE '月报'
      END
      FROM subjects subject
      WHERE subject.id = template.subject_id
    ) || CASE WHEN (
      SELECT COUNT(*)
      FROM report_templates sibling
      JOIN subjects sibling_subject ON sibling_subject.id = sibling.subject_id
      WHERE sibling.workspace_id = template.workspace_id
        AND sibling.report_type = template.report_type
        AND sibling.subject_id IS NOT NULL
        AND lower(trim(sibling_subject.name)) = lower(trim((SELECT name FROM subjects WHERE id = template.subject_id)))
        AND sibling.id <= template.id
    ) > 1 THEN ' (' || (
      SELECT COUNT(*)
      FROM report_templates sibling
      JOIN subjects sibling_subject ON sibling_subject.id = sibling.subject_id
      WHERE sibling.workspace_id = template.workspace_id
        AND sibling.report_type = template.report_type
        AND sibling.subject_id IS NOT NULL
        AND lower(trim(sibling_subject.name)) = lower(trim((SELECT name FROM subjects WHERE id = template.subject_id)))
        AND sibling.id <= template.id
    ) || ')' ELSE '' END
WHERE subject_id IS NOT NULL;

UPDATE report_templates
SET name = CASE report_type
    WHEN 'daily' THEN '迁移日报'
    WHEN 'weekly' THEN '迁移周报'
    ELSE '迁移月报'
  END || ' - ' || substr(id, 1, 8)
WHERE trim(name) = '';

UPDATE report_templates SET subject_id = NULL;

-- Older workspaces may be missing one of the built-in report types.
INSERT INTO report_templates(
  id, workspace_id, report_type, subject_id, content, is_builtin, created_at, updated_at, version,
  name, description, is_default, sort_order
)
SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
       workspace.id, type.report_type, NULL, type.content, 1, workspace.updated_at, workspace.updated_at, 1,
       type.name, '系统内置模板', 0, 10
FROM workspaces workspace
JOIN (
  SELECT 'daily' AS report_type, '标准日报' AS name, '【{{姓名}}】{{日期}} {{星期}} {{工作时段}} {{总工时}}

{{今日事项}}

## 明日计划：

{{明日计划}}
' AS content
  UNION ALL SELECT 'weekly', '标准周报', '# {{日期范围}}

{{统计信息}}

## 每日情况

{{每日情况}}

## 总结

{{总结}}
'
  UNION ALL SELECT 'monthly', '标准月报', '# {{日期范围}}

{{统计信息}}

## 本月事项

{{每日情况}}

## 总结

{{总结}}
'
) type
WHERE workspace.deleted_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM report_templates existing
    WHERE existing.workspace_id = workspace.id
      AND existing.report_type = type.report_type
      AND existing.deleted_at IS NULL
  );

UPDATE report_templates AS template
SET is_default = CASE WHEN template.id = (
      SELECT candidate.id
      FROM report_templates candidate
      WHERE candidate.workspace_id = template.workspace_id
        AND candidate.report_type = template.report_type
        AND candidate.deleted_at IS NULL
      ORDER BY CASE WHEN candidate.is_builtin = 1 THEN 0 ELSE 1 END,
               candidate.created_at, candidate.id
      LIMIT 1
    ) THEN 1 ELSE 0 END,
    sort_order = CASE WHEN sort_order = 0 THEN (
      SELECT COUNT(*) * 10
      FROM report_templates candidate
      WHERE candidate.workspace_id = template.workspace_id
        AND candidate.report_type = template.report_type
        AND candidate.deleted_at IS NULL
        AND candidate.id <= template.id
    ) ELSE sort_order END;

CREATE UNIQUE INDEX uq_report_templates_active_name
  ON report_templates(workspace_id, report_type, lower(trim(name)))
  WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_report_templates_active_default
  ON report_templates(workspace_id, report_type)
  WHERE is_default = 1 AND deleted_at IS NULL;
CREATE INDEX idx_report_templates_active_order
  ON report_templates(workspace_id, report_type, sort_order, name)
  WHERE deleted_at IS NULL;
