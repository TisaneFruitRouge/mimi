/**
 * How each tool's arguments read on an approval card. Integrations add an entry for
 * their tools; anything without one falls back to a tidy list of its arguments.
 */
export type ArgRow = { label: string; value: string };

type Formatter = (args: Record<string, unknown>) => ArgRow[];

export const formatters: Record<string, Formatter> = {
  calendar_add_event: (a) => {
    const rows: ArgRow[] = [{ label: "Event", value: String(a.title ?? "") }];
    rows.push({ label: "When", value: when(String(a.start ?? ""), a.end ? String(a.end) : null) });
    if (a.calendar) rows.push({ label: "Calendar", value: String(a.calendar) });
    if (a.location) rows.push({ label: "Where", value: String(a.location) });
    if (a.notes) rows.push({ label: "Notes", value: String(a.notes) });
    return rows;
  },
};

/** "Friday 2 October, 10:00–10:45" from the tool's local-time strings. */
function when(start: string, end: string | null) {
  const allDay = start.length === 10;
  const s = new Date(allDay ? `${start}T00:00` : start);
  if (Number.isNaN(s.getTime())) return end ? `${start} – ${end}` : start;
  const day = s.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  if (allDay) return end && end !== start ? `${day} – ${new Date(`${end}T00:00`).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}` : `${day} (all day)`;
  const time = (d: Date) => d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const e = end ? new Date(end) : new Date(s.getTime() + 3600_000);
  return e.toDateString() === s.toDateString() ? `${day}, ${time(s)}–${time(e)}` : `${day} ${time(s)} – ${e.toLocaleString()}`;
}

export function describeArgs(tool: string, args: Record<string, unknown>): ArgRow[] {
  const format = formatters[tool];
  if (format) return format(args);
  return Object.entries(args ?? {})
    .filter(([, v]) => v !== null && v !== undefined && v !== "")
    .map(([k, v]) => ({
      label: humanize(k),
      value: typeof v === "string" ? v : JSON.stringify(v, null, 1),
    }));
}

function humanize(key: string) {
  const words = key.replace(/[_-]+/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
