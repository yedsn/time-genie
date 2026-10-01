<script setup lang="ts">
import { computed, ref } from "vue";
import { CalendarRange, CheckCircle2, ChevronDown, ChevronRight, Clock3, ListChecks, TrendingUp } from "lucide-vue-next";
import { useWorkdayStore } from "../store";

type WeekDayState = "settled" | "active" | "planned" | "rest";

type WeekDayItem = {
  title: string;
  state: "done" | "active" | "planned";
  minutes?: number;
};

type WeekDay = {
  id: string;
  weekday: string;
  date: string;
  fullDate: string;
  state: WeekDayState;
  minutes: number;
  completed: number;
  total: number;
  summary: string;
  items: WeekDayItem[];
};

const store = useWorkdayStore();
const expandedDayId = ref("tue");

const weekDays = computed<WeekDay[]>(() => [
  {
    id: "mon",
    weekday: "周一",
    date: "09.14",
    fullDate: "2026-09-14",
    state: "settled",
    minutes: 370,
    completed: 4,
    total: 5,
    summary: "完成环境检查、备份异常清理和同步方案梳理",
    items: [
      { title: "检查运维环境与同步任务状态", state: "done", minutes: 45 },
      { title: "清理 NAS01 异常导致的备份问题", state: "done", minutes: 105 },
      { title: "整理江湖数据同步 v3 方案", state: "done", minutes: 130 },
      { title: "核对虚拟机备份结果", state: "done", minutes: 90 }
    ]
  },
  {
    id: "tue",
    weekday: "周二",
    date: "09.15",
    fullDate: "2026-09-15",
    state: "active",
    minutes: store.todayMinutes,
    completed: store.doneCount,
    total: store.tasks.length,
    summary: "推进数据同步 v3 开发，并处理运维相关事项",
    items: store.entries.filter((entry) => entry.kind !== "break").map((entry) => ({
      title: store.timeEntryLabel(entry),
      state: entry.state === "ended" ? "done" : "active",
      minutes: Math.ceil(store.entryDurationSeconds(entry) / 60)
    }))
  },
  {
    id: "wed",
    weekday: "周三",
    date: "09.16",
    fullDate: "2026-09-16",
    state: "planned",
    minutes: 0,
    completed: 0,
    total: 3,
    summary: "计划继续开发同步功能并整理接口设计",
    items: [
      { title: "开发江湖数据同步 v3 功能", state: "planned" },
      { title: "整理接口与数据结构设计", state: "planned" },
      { title: "处理运维异常问题", state: "planned" }
    ]
  },
  {
    id: "thu",
    weekday: "周四",
    date: "09.17",
    fullDate: "2026-09-17",
    state: "planned",
    minutes: 0,
    completed: 0,
    total: 3,
    summary: "计划推进虚拟机备份和模板镜像整理",
    items: [
      { title: "重新分配 PVE 虚拟机 ID", state: "planned" },
      { title: "开启虚拟机备份", state: "planned" },
      { title: "制作带版本号的模板虚拟机", state: "planned" }
    ]
  },
  {
    id: "fri",
    weekday: "周五",
    date: "09.18",
    fullDate: "2026-09-18",
    state: "planned",
    minutes: 0,
    completed: 0,
    total: 3,
    summary: "计划整理工作记录并完成自动化运维调研",
    items: [
      { title: "整理工作记录文档到 Teable", state: "planned" },
      { title: "梳理数据分析相关内容", state: "planned" },
      { title: "调研 Jpom、Openocta", state: "planned" }
    ]
  },
  {
    id: "sat",
    weekday: "周六",
    date: "09.19",
    fullDate: "2026-09-19",
    state: "rest",
    minutes: 0,
    completed: 0,
    total: 0,
    summary: "暂无工作安排",
    items: []
  },
  {
    id: "sun",
    weekday: "周日",
    date: "09.20",
    fullDate: "2026-09-20",
    state: "rest",
    minutes: 0,
    completed: 0,
    total: 0,
    summary: "暂无工作安排",
    items: []
  }
]);

const totalMinutes = computed(() => weekDays.value.reduce((sum, day) => sum + day.minutes, 0));
const completedCount = computed(() => weekDays.value.reduce((sum, day) => sum + day.completed, 0));
const activeDayCount = computed(() => weekDays.value.filter((day) => day.minutes > 0).length);
const averageMinutes = computed(() => activeDayCount.value ? Math.round(totalMinutes.value / activeDayCount.value) : 0);
const maxDayMinutes = computed(() => Math.max(480, ...weekDays.value.map((day) => day.minutes)));

function formatMinutes(minutes: number) {
  if (minutes <= 0) return "0h";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (!hours) return `${rest}min`;
  return rest ? `${hours}h ${rest}min` : `${hours}h`;
}

function stateLabel(state: WeekDayState) {
  if (state === "settled") return "已完成";
  if (state === "active") return "进行中";
  if (state === "planned") return "待开始";
  return "休息日";
}

function itemStateLabel(state: WeekDayItem["state"]) {
  if (state === "done") return "已完成";
  if (state === "active") return "进行中";
  return "待开始";
}

function toggleDay(day: WeekDay) {
  if (!day.items.length) return;
  expandedDayId.value = expandedDayId.value === day.id ? "" : day.id;
}
</script>

<template>
  <section class="weekly-workspace">
    <header class="weekly-overview-head">
      <div>
        <span class="weekly-kicker"><CalendarRange :size="13" />第 38 周</span>
        <h2>2026-09-14 至 2026-09-20</h2>
      </div>
      <span class="weekly-progress-label">本周第 2 个工作日</span>
    </header>

    <section class="weekly-stat-grid" aria-label="本周统计">
      <article class="weekly-stat primary">
        <span><Clock3 :size="15" />本周工时</span>
        <strong>{{ formatMinutes(totalMinutes) }}</strong>
        <small>已记录 {{ activeDayCount }} 个工作日</small>
      </article>
      <article class="weekly-stat">
        <span><CheckCircle2 :size="15" />完成事项</span>
        <strong>{{ completedCount }}</strong>
        <small>按每日完成记录汇总</small>
      </article>
      <article class="weekly-stat">
        <span><ListChecks :size="15" />计划事项</span>
        <strong>{{ weekDays.reduce((sum, day) => sum + day.total, 0) }}</strong>
        <small>含本周后续安排</small>
      </article>
      <article class="weekly-stat">
        <span><TrendingUp :size="15" />日均用时</span>
        <strong>{{ formatMinutes(averageMinutes) }}</strong>
        <small>按已记录工作日计算</small>
      </article>
    </section>

    <section class="weekly-days-section">
      <div class="weekly-days-head">
        <div><h2>每日情况</h2><span>点击有事项的日期查看当天明细</span></div>
        <div class="weekly-legend" aria-label="状态说明">
          <span class="done">已完成</span>
          <span class="active">进行中</span>
          <span class="planned">待开始</span>
        </div>
      </div>

      <div class="weekly-day-list">
        <article
          v-for="day in weekDays"
          :key="day.id"
          class="weekly-day-row"
          :class="[day.state, { expanded: expandedDayId === day.id, interactive: day.items.length }]"
        >
          <button class="weekly-day-summary" type="button" :disabled="!day.items.length" @click="toggleDay(day)">
            <span class="weekly-day-date">
              <strong>{{ day.weekday }}</strong>
              <time :datetime="day.fullDate">{{ day.date }}</time>
            </span>
            <span class="weekly-day-status" :class="day.state">{{ stateLabel(day.state) }}</span>
            <span class="weekly-day-copy">
              <strong>{{ day.summary }}</strong>
              <span v-if="day.total">完成 {{ day.completed }} / {{ day.total }} 项</span>
              <span v-else>没有安排事项</span>
            </span>
            <span class="weekly-day-bar" aria-hidden="true">
              <i :style="{ width: `${Math.max(day.minutes ? 4 : 0, day.minutes / maxDayMinutes * 100)}%` }"></i>
            </span>
            <span class="weekly-day-time">{{ formatMinutes(day.minutes) }}</span>
            <ChevronDown v-if="day.items.length && expandedDayId === day.id" :size="16" />
            <ChevronRight v-else-if="day.items.length" :size="16" />
            <span v-else class="weekly-day-chevron-space"></span>
          </button>

          <div v-if="day.items.length && expandedDayId === day.id" class="weekly-day-details">
            <div v-for="item in day.items" :key="item.title" class="weekly-day-item">
              <span class="weekly-item-state" :class="item.state" :title="itemStateLabel(item.state)"></span>
              <span>{{ item.title }}</span>
              <time v-if="item.minutes">{{ formatMinutes(item.minutes) }}</time>
              <small v-else>{{ itemStateLabel(item.state) }}</small>
            </div>
          </div>
        </article>
      </div>
    </section>
  </section>
</template>
