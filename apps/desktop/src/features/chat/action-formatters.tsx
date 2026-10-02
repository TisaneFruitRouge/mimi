/**
 * How each tool's arguments read on an approval card. Integrations add an entry for
 * their tools; anything without one falls back to a tidy list of its arguments.
 */
export type ArgRow = {
  label: string;
  value: string;
  /** The value is Markdown to show formatted, as the recipient will see it. */
  markdown?: boolean;
};

type Formatter = {
  /** Arguments the rows cover (or deliberately leave out). Any other is listed after them. */
  keys: string[];
  rows: (args: Record<string, unknown>) => ArgRow[];
};

export const formatters: Record<string, Formatter> = {
  // Guests are listed one per line: an event with them reaches each of them.
  calendar_add_event: {
    keys: ["title", "start", "end", "calendar", "location", "notes", "guests", "guests_note", "email_note"],
    rows: (a) => {
      const rows: ArgRow[] = [{ label: "Event", value: text(a.title) }];
      rows.push({ label: "When", value: when(text(a.start), present(a.end) ? text(a.end) : null) });
      if (present(a.calendar)) rows.push({ label: "Calendar", value: text(a.calendar) });
      if (present(a.location)) rows.push({ label: "Where", value: text(a.location) });
      if (present(a.guests)) rows.push({ label: "Guests", value: lines(a.guests) });
      if (present(a.guests_note)) rows.push({ label: "Guests", value: text(a.guests_note) });
      if (present(a.notes)) rows.push({ label: "Notes", value: text(a.notes) });
      // The daemon says how the guests hear of it (Google invites them, or nobody does).
      if (present(a.guests))
        rows.push({
          label: "Email",
          value: present(a.email_note) ? text(a.email_note) : "Nobody is emailed. You can send the invitations after.",
        });
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
      "add_guests",
      "remove_guests",
      "guests",
      "email_note",
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
      if (present(a.add_guests)) rows.push({ label: "Invite", value: lines(a.add_guests) });
      if (present(a.remove_guests)) rows.push({ label: "Take off", value: lines(a.remove_guests) });
      if (present(a.guests)) rows.push({ label: "Guests after", value: lines(a.guests) });
      if (present(a.email_note)) rows.push({ label: "Email", value: text(a.email_note) });
      else if (present(a.add_guests) || present(a.remove_guests))
        rows.push({ label: "Email", value: "Nobody is emailed. You can tell them after." });
      return rows;
    },
  },
  // Who it goes to, from which address, and about which event. `offer` and `event` only
  // point at it; `guests` is how the request narrowed the recipients.
  calendar_send_invitations: {
    keys: ["event", "offer", "kind", "event_title", "event_when", "recipients", "from", "from_note", "guests"],
    rows: (a) => {
      const what: Record<string, string> = {
        invite: "The invitation",
        update: "The new details",
        cancel: "It's cancelled",
        uninvite: "They're no longer invited",
      };
      const rows: ArgRow[] = [
        { label: "Event", value: text(a.event_title) },
        { label: "When", value: upperFirst(text(a.event_when)) },
        { label: "To", value: lines(a.recipients) },
        { label: "Message", value: what[text(a.kind)] ?? "The invitation" },
      ];
      if (present(a.from)) rows.push({ label: "From", value: text(a.from) });
      if (present(a.from_note)) rows.push({ label: "Note", value: text(a.from_note) });
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
  // Everyone it reaches by name with their exact address (`to` holds just the addresses),
  // the public groups it joins to post, the assistant's account, and the whole message.
  matrix_send: {
    keys: ["to", "recipients", "from", "joins", "text"],
    rows: (a) => {
      const rows: ArgRow[] = [{ label: "To", value: present(a.recipients) ? lines(a.recipients) : lines(a.to) }];
      if (present(a.joins))
        rows.push({ label: "Joins", value: `${lines(a.joins)}\nA public group: your assistant joins it to post this.` });
      if (present(a.from)) rows.push({ label: "From", value: `Your assistant, ${text(a.from)}` });
      rows.push({ label: "Message", value: text(a.text), markdown: true });
      return rows;
    },
  },
  // The whole message, exactly as it will be sent: blind copies said to be hidden, and
  // every file with its size and where it comes from (looked up by the daemon).
  // `thread_id` only threads the reply.
  mail_send: {
    keys: ["to", "cc", "bcc", "subject", "body", "thread_id", "attachments"],
    rows: (a) => {
      const rows: ArgRow[] = [{ label: "To", value: text(a.to) }];
      if (present(a.cc)) rows.push({ label: "Cc", value: text(a.cc) });
      if (present(a.bcc))
        rows.push({ label: "Bcc", value: `${lines(a.bcc)}\nA hidden copy: the others won't see this.` });
      rows.push({ label: "Subject", value: text(a.subject) || "(no subject)" });
      rows.push({ label: "Message", value: text(a.body) });
      if (Array.isArray(a.attachments) && a.attachments.length > 0)
        rows.push({ label: a.attachments.length === 1 ? "File" : "Files", value: a.attachments.map(fileLine).join("\n") });
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

/** A list with one entry per line: each guest or recipient on their own. */
function lines(v: unknown): string {
  return Array.isArray(v) ? v.map(text).join("\n") : text(v);
}

/** "Invoice.pdf · 340 KB · From the email “March” from Sam": a file an email carries. */
function fileLine(f: unknown): string {
  if (f === null || typeof f !== "object") return text(f);
  const { name, size, from } = f as { name?: unknown; size?: unknown; from?: unknown };
  const parts = [text(name) || "A file"];
  if (typeof size === "number" && size > 0)
    parts.push(size < 1e6 ? `${Math.max(1, Math.round(size / 1e3))} KB` : `${(size / 1e6).toFixed(1)} MB`);
  if (present(from)) parts.push(text(from));
  return parts.join(" · ");
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
