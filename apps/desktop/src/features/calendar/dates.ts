import type { CalendarEvent } from "@/bindings/CalendarEvent";
import { clock } from "@/features/reminders/time";

export const DAY = 86_400_000;

/** Local midnight of the day `ms` falls on. */
export function startOfDay(ms: number) {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Calendar days, not 24-hour steps, so weeks with a clock change stay aligned. */
export function addDays(ms: number, days: number) {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days).getTime();
}

/** Monday of the week `ms` falls in. */
export function startOfWeek(ms: number) {
  const day = (new Date(ms).getDay() + 6) % 7;
  return addDays(startOfDay(ms), -day);
}

export const sameDay = (a: number, b: number) => startOfDay(a) === startOfDay(b);

/** "October 2026", or "Sep – Oct 2026" for a week across two months. */
export function monthTitle(weekStart: number) {
  const a = new Date(weekStart);
  const b = new Date(addDays(weekStart, 6));
  if (a.getMonth() === b.getMonth())
    return a.toLocaleDateString([], { month: "long", year: "numeric" });
  const short = (d: Date) => d.toLocaleDateString([], { month: "short" });
  return a.getFullYear() === b.getFullYear()
    ? `${short(a)} – ${short(b)} ${b.getFullYear()}`
    : `${short(a)} ${a.getFullYear()} – ${short(b)} ${b.getFullYear()}`;
}

/** "Today", "Tomorrow", else "Wednesday 7 October". */
export function dayTitle(ms: number) {
  const days = Math.round((startOfDay(ms) - startOfDay(Date.now())) / DAY);
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  if (days === -1) return "Yesterday";
  return new Date(ms).toLocaleDateString([], {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: new Date(ms).getFullYear() === new Date().getFullYear() ? undefined : "numeric",
  });
}

/** "9:00 – 9:30", "All day", or across days "Mon 9:00 – Tue 11:00". */
export function timeRange(e: CalendarEvent) {
  if (e.all_day) {
    const days = Math.round((startOfDay(e.end) - startOfDay(e.start)) / DAY);
    return days > 1 ? `All day, ${days} days` : "All day";
  }
  if (sameDay(e.start, e.end - 1)) return `${clock(e.start)} – ${clock(e.end)}`;
  const day = (ms: number) => new Date(ms).toLocaleDateString([], { weekday: "short" });
  return `${day(e.start)} ${clock(e.start)} – ${day(e.end)} ${clock(e.end)}`;
}

/** "Wednesday 7 October · 11:00 – 12:00" */
export function whenLong(e: CalendarEvent) {
  return `${dayTitle(e.start)} · ${timeRange(e)}`;
}

/** Whether an event shows on the day starting at `day` (local midnight). */
export function onDay(e: CalendarEvent, day: number) {
  const next = addDays(day, 1);
  // All-day events end at midnight after their last day.
  return e.start < next && (e.end > day || (e.end === e.start && e.start >= day));
}

/** The shortest an event is drawn, so its title fits on one line. */
const MIN_MINUTES = 28;

export interface Placed {
  event: CalendarEvent;
  /** Minutes from the day's midnight, clipped to the day. */
  top: number;
  bottom: number;
  column: number;
  columns: number;
}

/**
 * Timed events of one day, side by side where they overlap: each cluster of
 * overlapping events is split into as many columns as it needs, like Calendar.app.
 */
export function layoutDay(events: CalendarEvent[], day: number): Placed[] {
  const end = addDays(day, 1);
  const minutes = (ms: number) => (ms - day) / 60_000;
  const items = events
    .filter((e) => !e.all_day && e.start < end && e.end > day)
    .map((event) => {
      const top = Math.max(0, minutes(event.start));
      // Very short events still get room for a line of text.
      const bottom = Math.max(top + MIN_MINUTES, Math.min(minutes(end), minutes(event.end)));
      return { event, top, bottom, column: 0, columns: 1 };
    })
    .sort((a, b) => a.top - b.top || b.bottom - a.bottom);

  let cluster: Placed[] = [];
  let clusterEnd = -1;
  const columnEnds: number[] = [];
  const close = () => {
    const n = Math.max(1, ...cluster.map((p) => p.column + 1));
    for (const p of cluster) p.columns = n;
    cluster = [];
    columnEnds.length = 0;
  };
  for (const p of items) {
    if (p.top >= clusterEnd && cluster.length) close();
    let col = columnEnds.findIndex((e) => e <= p.top);
    if (col === -1) col = columnEnds.length;
    columnEnds[col] = p.bottom;
    p.column = col;
    cluster.push(p);
    clusterEnd = Math.max(clusterEnd, p.bottom);
  }
  if (cluster.length) close();
  return items;
}

/** "HH:MM" and "YYYY-MM-DD" for form inputs, in local time. */
const pad = (n: number) => String(n).padStart(2, "0");
export const inputDate = (ms: number) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};
export const inputTime = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
export const fromInputs = (date: string, time = "00:00") => new Date(`${date}T${time}`).getTime();
