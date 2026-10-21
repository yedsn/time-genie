UPDATE report_templates
SET content = '{{统计信息}}

{{日报合集}}',
    description = '合并周期内已保存日报的最终内容',
    updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000,
    version = version + 1
WHERE is_builtin = 1
  AND report_type IN ('weekly', 'monthly')
  AND deleted_at IS NULL;
