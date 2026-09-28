/**
 * How each tool's arguments read on an approval card. Integrations add an entry for
 * their tools; anything without one falls back to a tidy list of its arguments.
 */
export type ArgRow = { label: string; value: string };

type Formatter = {
  /** Arguments the rows cover (or deliberately leave out). Any other is listed after them. */
  keys: string[];
  rows: (args: Record<string, unknown>) => ArgRow[];
};

export const formatters: Record<string, Formatter> = {
  calendar_add_event: {
    keys: ["title", "start", "end", "calendar", "location", "notes"],
    rows: (a) => {
      const rows: ArgRow[] = [{ label: "Event", value: text(a.title) }];
      rows.push({ label: "When", value: when(text(a.start), present(a.end) ? text(a.end) : null) });
      if (present(a.calendar)) rows.push({ label: "Calendar", value: text(a.calendar) });
      if (present(a.location)) rows.push({ label: "Where", value: text(a.location) });
      if (present(a.notes)) rows.push({ label: "Notes", value: text(a.notes) });
      return rows;
    },
  },
  // The event as the calendar has it (looked up by the daemon), then what changes.
  // `event` and `calendar_id` only point at it; `repeats` and `which` read as the scope.
  calendar_change_event: {
    keys: [
      "event",
      "event_title",
      "event_when",
      "calendar",
      "calendar_id",
      "repeats",
      "which",
      "new_time",
      "title",
      "start",
      "end",
      "location",
      "notes",
    ],
    rows: (a) => {
      const rows: ArgRow[] = [
        { label: "Event", value: text(a.event_title) },
        { label: "Now", value: upperFirst(text(a.event_when)) },
        { label: "Calendar", value: text(a.calendar) },
      ];
      if (a.repeats === true)
        rows.push({ label: "Repeats", value: a.which === "all" ? "Change every time" : "Change only this time" });
      if (present(a.new_time)) rows.push({ label: "New time", value: upperFirst(text(a.new_time)) });
      if (present(a.title)) rows.push({ label: "New name", value: text(a.title) });
      if (typeof a.location === "string")
        rows.push({ label: "Where", value: a.location.trim() ? a.location : "(removed)" });
      if (typeof a.notes === "string") rows.push({ label: "Notes", value: a.notes.trim() ? a.notes : "(removed)" });
      return rows;
    },
  },
  calendar_delete_event: {
    keys: ["event", "event_title", "event_when", "calendar", "calendar_id", "repeats", "which"],
    rows: (a) => {
      const rows: ArgRow[] = [
        { label: "Event", value: text(a.event_title) },
        { label: "When", value: upperFirst(text(a.event_when)) },
        { label: "Calendar", value: text(a.calendar) },
      ];
      if (a.repeats === true)
        rows.push({ label: "Repeats", value: a.which === "all" ? "Remove every time" : "Remove only this time" });
      return rows;
    },
  },
  // The whole message, exactly as it will be sent. `thread_id` only threads the reply.
  mail_send: {
    keys: ["to", "cc", "subject", "body", "thread_id"],
    rows: (a) => {
      const rows: ArgRow[] = [{ label: "To", value: text(a.to) }];
      if (present(a.cc)) rows.push({ label: "Cc", value: text(a.cc) });
      rows.push({ label: "Subject", value: text(a.subject) || "(no subject)" });
      rows.push({ label: "Message", value: text(a.body) });
      return rows;
    },
  },
};

/** Any value as the card shows it, whatever its type: lists joined, nothing hidden. */
function text(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.map(text).join(", ");
  if (typeof v === "object") return JSON.stringify(v, null, 1);
  return String(v);
}

function present(v: unknown) {
  return text(v).trim() !== "";
}

/** "On Friday 2 Oct, 10:00–10:45" from the daemon's "on Friday…". */
function upperFirst(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

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

/** The card's rows: the tool's formatter, then every argument it doesn't cover. */
export function describeArgs(tool: string, args: unknown): ArgRow[] {
  const all: Record<string, unknown> =
    args !== null && typeof args === "object" && !Array.isArray(args) ? (args as Record<string, unknown>) : { arguments: args };
  const format = formatters[tool];
  const rows = format ? format.rows(all) : [];
  const covered = new Set(format?.keys ?? []);
  for (const [k, v] of Object.entries(all)) {
    if (!covered.has(k) && present(v)) rows.push({ label: humanize(k), value: text(v) });
  }
  return rows;
}

function humanize(key: string) {
  const words = key.replace(/[_-]+/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
