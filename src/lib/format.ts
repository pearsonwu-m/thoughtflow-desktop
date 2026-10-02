// Dates and labels for the notebook-style UI.

import type { Mode, TaskEffort, Thought } from "../types";

const DAY_MS = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Notebook-style 24-hour time, e.g. "21:14". */
export function formatTime(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** Section heading for the archive: TODAY, YESTERDAY, a weekday, or a date. */
export function dayLabel(ms: number, now: number = Date.now()): string {
  const diff = Math.round((startOfDay(now) - startOfDay(ms)) / DAY_MS);
  if (diff <= 0) return "Today";
  if (diff === 1) return "Yesterday";
  const date = new Date(ms);
  if (diff < 7) return date.toLocaleDateString(undefined, { weekday: "long" });
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleDateString(undefined, {
    month: "long",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

export interface DayGroup<T> {
  label: string;
  items: T[];
}

/** Groups thoughts (already sorted newest-first) under day headings. */
export function groupByDay<T extends Pick<Thought, "updatedAt">>(items: T[], now: number = Date.now()): DayGroup<T>[] {
  const groups: DayGroup<T>[] = [];
  for (const item of items) {
    const label = dayLabel(item.updatedAt, now);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(item);
    else groups.push({ label, items: [item] });
  }
  return groups;
}

/** The user's local date, sent so Claude can resolve "Friday" or "next week". */
export function localDateLabel(now: Date = new Date()): string {
  return now.toLocaleDateString("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" });
}

/** ISO date (YYYY-MM-DD) in local time. */
export function isoDate(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** "Oct 3" for a YYYY-MM-DD due date, with "today"/"tomorrow"/"overdue" hints. */
export function formatDue(date: string | null, now: Date = new Date()): string | null {
  if (!date) return null;
  const [y, m, d] = date.split("-").map(Number);
  if (!y || !m || !d) return date;
  const due = new Date(y, m - 1, d).getTime();
  const diff = Math.round((due - startOfDay(now.getTime())) / DAY_MS);
  if (diff === 0) return "today";
  if (diff === 1) return "tomorrow";
  if (diff < 0) return `overdue · ${new Date(due).toLocaleDateString(undefined, { month: "short", day: "numeric" })}`;
  return new Date(due).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export const MODE_LABELS: Record<Mode, string> = {
  think: "Think",
  plan: "Plan",
  task: "Tasks",
  reflect: "Reflect",
  prompt: "Prompt",
};

export const EFFORT_LABELS: Record<TaskEffort, string> = {
  quick: "< 30 min",
  short: "~1 hr",
  medium: "half day",
  long: "1+ days",
};

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Friendly model name for display, e.g. "claude-opus-5-5" → "Claude Opus 5.5". */
export function modelLabel(id: string): string {
  const match = /^claude-(opus|sonnet|haiku|fable|mythos)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?$/.exec(id);
  if (!match) return id;
  const [, family = "", major, minor] = match;
  return `Claude ${family.charAt(0).toUpperCase()}${family.slice(1)} ${major}${minor ? `.${minor}` : ""}`;
}
