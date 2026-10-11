export type ReportTemplateType = "daily" | "weekly" | "monthly";

export type ReportTemplatePlaceholderDefinition = {
  token: string;
  label: string;
  description: string;
  example: string;
  types: ReportTemplateType[];
};

export type ReportTemplateListItem = {
  id: string;
  name: string;
  isDefault: boolean;
};

export const reportTemplatePlaceholders: ReportTemplatePlaceholderDefinition[] = [
  { token: "{{姓名}}", label: "姓名", description: "设置中的用户姓名", example: "晓健", types: ["daily", "weekly", "monthly"] },
  { token: "{{日期}}", label: "日期", description: "日报日期", example: "2026-10-11", types: ["daily"] },
  { token: "{{星期}}", label: "星期", description: "日报对应星期", example: "周日", types: ["daily"] },
  { token: "{{日期范围}}", label: "日期范围", description: "周报或月报周期", example: "2026-10-05 至 2026-10-11", types: ["weekly", "monthly"] },
  { token: "{{工作时段}}", label: "工作时段", description: "默认工作时段", example: "10:00-12:00 13:30-17:30", types: ["daily"] },
  { token: "{{总工时}}", label: "总工时", description: "所选事项累计用时", example: "6h", types: ["daily", "weekly", "monthly"] },
  { token: "{{今日事项}}", label: "今日事项", description: "日报事项明细", example: "1. 已完成事项", types: ["daily"] },
  { token: "{{明日计划}}", label: "明日计划", description: "下一日待办", example: "1. 明日重点事项", types: ["daily"] },
  { token: "{{统计信息}}", label: "统计信息", description: "周期统计", example: "- 累计用时：6h", types: ["weekly", "monthly"] },
  { token: "{{每日情况}}", label: "每日情况", description: "周期内逐日汇总", example: "- 周一：6h", types: ["weekly", "monthly"] },
  { token: "{{总结}}", label: "总结", description: "周期总结", example: "- 本期总结", types: ["weekly", "monthly"] },
];

export function placeholdersForReportType(type: ReportTemplateType) {
  return reportTemplatePlaceholders.filter((placeholder) => placeholder.types.includes(type));
}

export function validateReportTemplateDraft(input: {
  type: ReportTemplateType;
  name: string;
  content: string;
  templates: ReportTemplateListItem[];
  currentId?: string;
}) {
  const name = input.name.trim();
  if (!name) return "请输入模板名称";
  if (!input.content.trim()) return "报告模板不能为空";
  if (input.templates.some((template) => template.id !== input.currentId && template.name.trim().toLocaleLowerCase() === name.toLocaleLowerCase())) {
    return "同类型模板名称已存在";
  }

  const allowed = new Set(placeholdersForReportType(input.type).map((placeholder) => placeholder.token));
  const known = new Set(reportTemplatePlaceholders.map((placeholder) => placeholder.token));
  const tokens = input.content.match(/{{[^{}]+}}/g) ?? [];
  const remainder = input.content.replace(/{{[^{}]+}}/g, "");
  if (remainder.includes("{{") || remainder.includes("}}")) return "报告模板包含未闭合占位符";
  for (const token of tokens) {
    if (!known.has(token)) return `未知占位符：${token}`;
    if (!allowed.has(token)) return `占位符 ${token} 不适用于当前报告类型`;
  }
  return undefined;
}

export function insertTemplateToken(content: string, start: number, end: number, token: string) {
  const safeStart = Math.max(0, Math.min(start, content.length));
  const safeEnd = Math.max(safeStart, Math.min(end, content.length));
  const nextContent = `${content.slice(0, safeStart)}${token}${content.slice(safeEnd)}`;
  const cursor = safeStart + token.length;
  return { content: nextContent, selectionStart: cursor, selectionEnd: cursor };
}

export function renderTemplatePreview(content: string, replacements: Record<string, string>) {
  return Object.entries(replacements).reduce(
    (preview, [token, value]) => preview.split(token).join(value),
    content,
  );
}

export function selectReportTemplate<T extends ReportTemplateListItem>(
  templates: T[],
  options: { preferredId?: string; currentId?: string; selectDefault: boolean },
) {
  return templates.find((item) => item.id === options.preferredId)
    ?? (!options.selectDefault ? templates.find((item) => item.id === options.currentId) : undefined)
    ?? templates.find((item) => item.isDefault)
    ?? templates[0];
}

export function planTemplateDeletion<T extends ReportTemplateListItem>(target: T, templates: T[]) {
  const alternatives = templates.filter((item) => item.id !== target.id);
  if (!alternatives.length) return { error: "每种报告类型至少需要保留一份模板" } as const;
  return { replacement: target.isDefault ? alternatives.find((item) => item.isDefault) ?? alternatives[0] : undefined } as const;
}

export function reorderTemplateIds(templates: ReportTemplateListItem[], targetId: string, offset: -1 | 1) {
  const index = templates.findIndex((item) => item.id === targetId);
  const nextIndex = index + offset;
  if (index < 0 || nextIndex < 0 || nextIndex >= templates.length) return undefined;
  const ordered = [...templates];
  [ordered[index], ordered[nextIndex]] = [ordered[nextIndex], ordered[index]];
  return ordered.map((item) => item.id);
}
