import { reactive } from "vue";
import { getSubjectWorkSchedule as getSubjectWorkScheduleRecord, saveSubjectWorkSchedule as saveSubjectWorkScheduleRecord, type SubjectWorkScheduleDay } from "./tauri";

export type WorkScheduleSegment = {
  start: string;
  end: string;
};

export type WorkScheduleDay = {
  id: string;
  weekday: number;
  enabled: boolean;
  segments: WorkScheduleSegment[];
  version: number;
};

const DEFAULT_SEGMENTS: WorkScheduleSegment[] = [
  { start: "10:00", end: "12:00" },
  { start: "13:30", end: "17:30" },
];

const subjectSchedules = reactive(new Map<string, WorkScheduleDay[]>());

export function createDefaultWorkSchedule(): WorkScheduleDay[] {
  return Array.from({ length: 7 }, (_, weekday) => ({
    id: "",
    weekday,
    enabled: weekday < 5,
    segments: weekday < 5 ? DEFAULT_SEGMENTS.map((segment) => ({ ...segment })) : [],
    version: 0,
  }));
}

export function cloneWorkSchedule(schedule: WorkScheduleDay[]): WorkScheduleDay[] {
  return schedule.map((day) => ({
    ...day,
    segments: day.segments.map((segment) => ({ ...segment })),
  }));
}

export function getSubjectWorkSchedule(subjectId: string): WorkScheduleDay[] {
  const schedule = subjectSchedules.get(subjectId);
  return cloneWorkSchedule(schedule ?? createDefaultWorkSchedule());
}

export async function loadSubjectWorkSchedule(subjectId: string): Promise<WorkScheduleDay[]> {
  const schedule = await getSubjectWorkScheduleRecord(subjectId);
  subjectSchedules.set(subjectId, cloneWorkSchedule(schedule));
  return cloneWorkSchedule(schedule);
}

export async function saveSubjectWorkSchedule(subjectId: string, schedule: WorkScheduleDay[]): Promise<WorkScheduleDay[]> {
  const saved = await saveSubjectWorkScheduleRecord(subjectId, schedule as SubjectWorkScheduleDay[]);
  subjectSchedules.set(subjectId, cloneWorkSchedule(saved));
  return cloneWorkSchedule(saved);
}

export function workPeriodTextForDate(subjectId: string, date: Date): string {
  const schedule = subjectSchedules.get(subjectId) ?? createDefaultWorkSchedule();
  const weekday = (date.getDay() + 6) % 7;
  const day = schedule.find((item) => item.weekday === weekday);
  if (!day?.enabled || !day.segments.length) return "休息";
  return day.segments
    .slice()
    .sort((left, right) => left.start.localeCompare(right.start))
    .map((segment) => `${segment.start}-${segment.end}`)
    .join(" ");
}
