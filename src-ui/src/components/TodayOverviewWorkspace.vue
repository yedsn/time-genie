<script setup lang="ts">
import { computed } from "vue";
import { AlertCircle, Clock3, ListTodo, Play, RefreshCw } from "lucide-vue-next";
import { useWorkdayStore } from "../store";
import type { TodayWorkOverviewDayRecord, TodayWorkOverviewTaskRecord, TodayWorkOverviewTimelineItemRecord } from "../services/tauri";

const emit = defineEmits<{
  startTimer: [taskId: string];
  openTimer: [];
  openPlan: [];
  resolveUnassigned: [];
}>();

const store = useWorkdayStore();

const overview = computed(() => store.todayOverview);
const summary = computed(() => overview.value?.summary ?? {
  actualMinutes: 0,
  estimatedMinutes: 0,
  completedCount: 0,
  activeCount: 0,
  notStartedCount: 0,
  unassignedMinutes: 0,
  completionRate: 0,
});
const subjectOptions = computed(() => [{ id: "", name: "全部主体" }, ...store.subjects.map((subject) => ({ id: subject.id, name: subject.name }))]);
const trendDays = computed(() => (overview.value?.days ?? []).slice(-14));
const heatmapDays = computed(() => overview.value?.days ?? []);
const timelineItems = computed(() => overview.value?.timeline ?? []);
const overviewTasks = computed(() => overview.value?.tasks ?? []);
const completionPercent = computed(() => Math.round(summary.value.completionRate * 100));
const allSubjectScope = computed(() => !store.todayOverviewSubjectId);
const maxTrendMinutes = computed(() => Math.max(1, ...trendDays.value.map((day) => day.actualMinutes)));
const trendPoints = computed(() => {
  const days = trendDays.value;
  if (!days.length) return "";
  const width = 280;
  const baseline = 91;
  const plotHeight = 68;
  const xStep = days.length > 1 ? width / (days.length - 1) : width;
  return days.map((day, index) => {
    const x = Math.round(index * xStep);
    const y = Math.round(baseline - (day.actualMinutes / maxTrendMinutes.value) * plotHeight);
    return `${x},${y}`;
  }).join(" ");
});
const hasOverviewData = computed(() => {
  const totals = summary.value.actualMinutes + summary.value.estimatedMinutes + summary.value.unassignedMinutes;
  return totals > 0 || overviewTasks.value.length > 0 || timelineItems.value.length > 0 || heatmapDays.value.some((day) => day.actualMinutes > 0);
});

function formatMinutes(minutes: number) {
  const value = Math.max(0, Math.round(minutes));
  if (value < 60) return `${value}min`;
  const hours = Math.floor(value / 60);
  const rest = value % 60;
  return rest ? `${hours}h${rest}min` : `${hours}h`;
}

function formatShortDate(date: string) {
  return date.slice(5).replace("-", "/");
}

function formatWeekday(date: string) {
  const parsed = new Date(`${date}T00:00:00`);
  return ["日", "一", "二", "三", "四", "五", "六"][parsed.getDay()] ?? "";
}

function formatTime(timestamp?: number) {
  if (!timestamp) return "--:--";
  const date = new Date(timestamp);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function isToday(date: string) {
  return overview.value?.scope.todayDate === date;
}

function heatLevel(day: TodayWorkOverviewDayRecord) {
  if (day.actualMinutes <= 0) return 0;
  if (day.actualMinutes <= 30) return 1;
  if (day.actualMinutes <= 120) return 2;
  if (day.actualMinutes <= 240) return 3;
  return 4;
}

function statusLabel(status: string) {
  if (status === "done") return "已完成";
  if (status === "active" || status === "running") return "进行中";
  if (status === "started") return "已开始";
  return "未开始";
}

function statusClass(status: string) {
  if (status === "done") return "done";
  if (status === "active" || status === "running" || status === "started") return "active";
  return "idle";
}

function timelineClass(item: TodayWorkOverviewTimelineItemRecord) {
  return [
    `kind-${item.itemType}`,
    item.state ? `state-${item.state}` : "",
  ];
}

function timelineLabel(item: TodayWorkOverviewTimelineItemRecord) {
  if (item.itemType === "break") return "休息";
  if (item.itemType === "unassigned") return "未归属";
  return item.taskTitle || "工作";
}

function timelineStateLabel(item: TodayWorkOverviewTimelineItemRecord) {
  if (item.state === "running" || item.state === "collecting") return "进行中";
  if (item.state === "paused") return "已暂停";
  return item.itemType === "unassigned" ? "待处理" : "已结束";
}

function timelineStyle(item: TodayWorkOverviewTimelineItemRecord) {
  const current = Date.now();
  const day = overview.value?.scope.todayDate ?? "";
  const dayStart = new Date(`${day}T00:00:00`).getTime();
  const dayEnd = dayStart + 24 * 60 * 60 * 1000;
  const startedAt = Math.min(Math.max(item.startedAt, dayStart), dayEnd);
  const endedAt = item.endedAt ?? (item.state === "running" || item.state === "collecting" ? current : item.startedAt + item.minutes * 60_000);
  const safeEndedAt = Math.min(Math.max(endedAt, startedAt + 60_000), dayEnd);
  const left = ((startedAt - dayStart) / (dayEnd - dayStart)) * 100;
  const width = Math.max(1.6, ((safeEndedAt - startedAt) / (dayEnd - dayStart)) * 100);
  return { left: `${left}%`, width: `${Math.min(width, 100 - left)}%` };
}

function taskEstimateLabel(task: TodayWorkOverviewTaskRecord) {
  if (task.todayEstimateMinutes) return `今日预计 ${formatMinutes(task.todayEstimateMinutes)}`;
  if (task.estimateMinutes) return `整体预计 ${formatMinutes(task.estimateMinutes)}`;
  return "未设置预计";
}

function taskTitle(task: TodayWorkOverviewTaskRecord) {
  if (!allSubjectScope.value) return task.pathLabel || task.title;
  return `${task.subjectName} / ${task.pathLabel || task.title}`;
}
</script>

<template>
  <section class="today-overview-workspace">
    <header class="today-overview-head">
      <div class="today-subject-tabs" role="tablist" aria-label="今日概览主体">
        <button
          v-for="subject in subjectOptions"
          :key="subject.id || 'all'"
          type="button"
          role="tab"
          :aria-selected="store.todayOverviewSubjectId === subject.id"
          :class="{ active: store.todayOverviewSubjectId === subject.id }"
          @click="store.selectTodayOverviewSubject(subject.id)"
        >
          {{ subject.name }}
        </button>
      </div>
      <div class="today-overview-actions">
        <button class="secondary-button compact" type="button" :disabled="store.todayOverviewLoading" @click="store.loadTodayOverview()"><RefreshCw :size="14" />刷新</button>
        <button class="secondary-button compact" type="button" @click="emit('openPlan')"><ListTodo :size="14" />待办</button>
        <button class="primary-button compact" type="button" @click="emit('openTimer')"><Clock3 :size="14" />计时</button>
      </div>
    </header>

    <div class="today-summary-grid">
      <div class="today-summary-item emphasis"><span>今日投入</span><strong>{{ formatMinutes(summary.actualMinutes) }}</strong><small>{{ overview?.scope.subjectName ?? '全部主体' }}</small></div>
      <div class="today-summary-item"><span>今日预计</span><strong>{{ formatMinutes(summary.estimatedMinutes) }}</strong><small>{{ summary.estimatedMinutes ? '按今日预计优先' : '暂无预计' }}</small></div>
      <div class="today-summary-item"><span>完成率</span><strong>{{ completionPercent }}%</strong><small>{{ summary.completedCount }} 完成 · {{ summary.activeCount }} 已开始 · {{ summary.notStartedCount }} 未开始</small></div>
      <button class="today-summary-item action" type="button" :disabled="summary.unassignedMinutes <= 0" @click="emit('resolveUnassigned')">
        <span>{{ allSubjectScope ? '未归属' : '未归属（全局）' }}</span><strong>{{ formatMinutes(summary.unassignedMinutes) }}</strong><small>{{ summary.unassignedMinutes ? '点击处理' : '无待处理时间' }}</small>
      </button>
    </div>

    <div v-if="!hasOverviewData && !store.todayOverviewLoading" class="today-empty-state">
      <AlertCircle :size="22" />
      <strong>今天还没有工作数据</strong>
      <span>可以先添加待办，或从计时开始。</span>
      <div><button class="secondary-button compact" type="button" @click="emit('openPlan')">进入待办</button><button class="primary-button compact" type="button" @click="emit('openTimer')">开始计时</button></div>
    </div>

    <div v-else class="today-visual-grid">
      <section class="today-panel timeline-panel">
        <div class="today-panel-head"><h2>今日时间分布</h2><span>{{ timelineItems.length }} 段</span></div>
        <div v-if="timelineItems.length" class="today-timeline">
          <div class="today-timeline-scale"><span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>24:00</span></div>
          <div v-for="item in timelineItems" :key="item.id" class="today-timeline-row" :class="timelineClass(item)">
            <div class="timeline-copy">
              <strong>{{ timelineLabel(item) }}</strong>
              <small>{{ formatTime(item.startedAt) }} - {{ item.endedAt ? formatTime(item.endedAt) : '现在' }} · {{ formatMinutes(item.minutes) }} · {{ timelineStateLabel(item) }}<template v-if="allSubjectScope && item.subjectName"> · {{ item.subjectName }}</template></small>
            </div>
            <div class="timeline-track"><i :style="timelineStyle(item)"></i></div>
          </div>
        </div>
        <p v-else class="today-muted-empty">今天还没有时间段。</p>
      </section>

      <section class="today-panel execution-panel">
        <div class="today-panel-head"><h2>执行摘要</h2><span>{{ overviewTasks.length }} 项</span></div>
        <div v-if="overviewTasks.length" class="today-task-summary-list">
          <article v-for="task in overviewTasks" :key="task.taskId" class="today-task-summary-row" :class="`status-${statusClass(task.status)}`">
            <div class="today-task-main">
              <strong>{{ taskTitle(task) }}</strong>
              <small>{{ taskEstimateLabel(task) }} · 实际 {{ formatMinutes(task.actualMinutes) }}<template v-if="task.lastEntryAt"> · 最近 {{ formatTime(task.lastEntryAt) }}</template></small>
            </div>
            <span class="today-task-state">{{ statusLabel(task.status) }}</span>
            <button class="secondary-button compact icon-button" type="button" :disabled="!task.selectable" :title="task.selectable ? '继续计时' : '父事项不可直接计时'" @click="emit('startTimer', task.taskId)"><Play :size="13" fill="currentColor" /></button>
          </article>
        </div>
        <p v-else class="today-muted-empty">今天没有相关事项。</p>
      </section>

      <section class="today-panel activity-panel">
        <div class="today-panel-head"><h2>近 30 天</h2><span>{{ heatmapDays.filter((day) => day.actualMinutes > 0).length }} 天有投入</span></div>
        <div class="today-heatmap" aria-label="近 30 天投入点阵">
          <span
            v-for="day in heatmapDays"
            :key="day.date"
            :class="[`level-${heatLevel(day)}`, { today: isToday(day.date) }]"
            :title="`${day.date} 实际 ${formatMinutes(day.actualMinutes)}，预计 ${formatMinutes(day.estimatedMinutes)}`"
          >
            <i></i><b>{{ formatShortDate(day.date) }}</b>
          </span>
        </div>
      </section>

      <section class="today-panel trend-panel">
        <div class="today-panel-head"><h2>近 14 天趋势</h2><span>最高 {{ formatMinutes(maxTrendMinutes) }}</span></div>
        <div class="today-trend-chart">
          <svg viewBox="0 0 280 100" role="img" aria-label="近 14 天实际用时趋势">
            <line x1="0" y1="91" x2="280" y2="91" />
            <polyline v-if="trendPoints" :points="trendPoints" />
            <circle v-for="(day, index) in trendDays" :key="day.date" :cx="trendDays.length > 1 ? index * (280 / (trendDays.length - 1)) : 280" :cy="91 - (day.actualMinutes / maxTrendMinutes) * 68" r="3.2"><title>{{ day.date }} {{ formatMinutes(day.actualMinutes) }}</title></circle>
          </svg>
          <div class="today-trend-labels"><span v-for="day in trendDays" :key="day.date">{{ formatWeekday(day.date) }}</span></div>
        </div>
      </section>
    </div>
  </section>
</template>
