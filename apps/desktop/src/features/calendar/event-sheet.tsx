import { Bell, Clock, MapPin, Sparkles, X } from "lucide-react";
import { toast } from "sonner";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { EventPerson } from "@/bindings/EventPerson";
import type { ScheduleItem } from "@/bindings/ScheduleItem";
import { PersonAvatar, initials } from "@/components/people";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { whenLong } from "@/features/calendar/dates";
import { useScheduleItems } from "@/features/reminders/reminders";
import { api } from "@/lib/api";
import { type Draft, mentionDraft } from "@/lib/draft";
import { useAssistantName } from "@/lib/queries";

const before: { minutes: number; label: string }[] = [
  { minutes: 10, label: "10 minutes before" },
  { minutes: 30, label: "30 minutes before" },
  { minutes: 60, label: "1 hour before" },
  { minutes: 24 * 60, label: "1 day before" },
];

export const beforeLabel = (minutes: number) =>
  before.find((b) => b.minutes === minutes)?.label ??
  (minutes % 60 === 0 ? `${minutes / 60} h before` : `${minutes} min before`);

/** Reminders tied to this occurrence of an event. */
export function remindersFor(items: ScheduleItem[], eventId: string) {
  return items.filter(
    (i) => i.schedule.type === "before_event" && i.schedule.event_id === eventId && !i.ended,
  );
}

/**
 * One event: when, where, who, the invitation's notes, reminders and "Ask Mimi".
 * Everything shown comes from the calendar and may be written by someone else, so it
 * is plain text: no links, no formatting.
 */
export function EventSheet({
  event,
  color,
  onClose,
  onAsk,
  onOpenPerson,
}: {
  event: CalendarEvent | null;
  color: string;
  onClose: () => void;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
}) {
  return (
    <Dialog open={!!event} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] gap-0 overflow-hidden bg-canvas p-0 sm:max-w-[460px]">
        {event && (
          <EventDetails event={event} color={color} onClose={onClose} onAsk={onAsk} onOpenPerson={onOpenPerson} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function EventDetails({
  event: e,
  color,
  onClose,
  onAsk,
  onOpenPerson,
}: {
  event: CalendarEvent;
  color: string;
  onClose: () => void;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
}) {
  const reminders = remindersFor(useScheduleItems().data ?? [], e.id);
  const people = guests(e);
  const assistant = useAssistantName();

  const remind = (minutes: number) =>
    api
      .addSchedule({
        kind: "reminder",
        title: e.title,
        instruction: null,
        schedule: { type: "before_event", event_id: e.id, event_title: e.title, minutes_before: minutes },
      })
      .then(() => toast.success(`You'll be reminded ${beforeLabel(minutes)}`))
      .catch((err) => toast.error(err.message));

  return (
    <div className="flex max-h-[85vh] flex-col">
      <div className="flex flex-col gap-1.5 px-6 pt-6 pb-4">
        <span className="flex items-center gap-2 type-subhead text-muted-foreground">
          <span className="size-2.5 shrink-0 rounded-full" style={{ background: color }} aria-hidden />
          {e.calendar}
        </span>
        <DialogTitle className="pr-8 type-title break-words">{e.title}</DialogTitle>
        <DialogDescription className="flex items-center gap-1.5 type-callout text-foreground">
          <Clock className="size-3.5 text-muted-foreground" />
          {whenLong(e)}
        </DialogDescription>
        {e.location && (
          <p className="flex items-start gap-1.5 type-callout break-words">
            <MapPin className="mt-[3px] size-3.5 shrink-0 text-muted-foreground" />
            {e.location}
          </p>
        )}
      </div>

      <div className="flex flex-col gap-5 overflow-y-auto px-6 pb-4">
        {people.length > 0 && (
          <section className="flex flex-col gap-2">
            <h3 className="section-label">
              {people.length === 1 ? "1 person" : `${people.length} people`}
            </h3>
            <div className="grouped flex flex-col">
              {people.map(({ person: p, organizer }) => {
                const name = p.person_name ?? p.name ?? p.email;
                const body = (
                  <>
                    {p.person_id ? (
                      <PersonAvatar id={p.person_id} name={name} size="sm" />
                    ) : (
                      <span className="inline-flex size-6 shrink-0 items-center justify-center rounded-full bg-fill text-[10.5px] font-medium text-muted-foreground">
                        {initials(name)}
                      </span>
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate type-callout">{name}</span>
                      {name !== p.email && (
                        <span className="block truncate type-footnote text-muted-foreground">{p.email}</span>
                      )}
                    </span>
                    {organizer && <span className="type-footnote text-faint">Organizer</span>}
                  </>
                );
                return p.person_id ? (
                  <button
                    key={p.email}
                    onClick={() => {
                      onClose();
                      onOpenPerson(p.person_id!);
                    }}
                    className="flex min-h-[48px] items-center gap-3 px-4 py-2 text-left transition-colors hover:bg-[rgb(118_118_128/0.06)]"
                  >
                    {body}
                  </button>
                ) : (
                  <div key={p.email} className="flex min-h-[48px] items-center gap-3 px-4 py-2">
                    {body}
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {e.notes && (
          <section className="flex flex-col gap-2">
            <h3 className="section-label">Notes</h3>
            <p className="max-h-48 overflow-y-auto rounded-[14px] bg-background px-4 py-3 type-callout break-words whitespace-pre-wrap text-foreground/90 shadow-[var(--shadow-card)]">
              {e.notes}
            </p>
          </section>
        )}

        {reminders.length > 0 && (
          <section className="flex flex-col gap-2">
            <h3 className="section-label">Reminders</h3>
            <div className="flex flex-wrap gap-1.5">
              {reminders.map((r) => (
                <span
                  key={r.id}
                  className="inline-flex h-7 items-center gap-1.5 rounded-full bg-background pr-1 pl-2.5 type-subhead shadow-[var(--shadow-card)]"
                >
                  <Bell className="size-3 text-muted-foreground" />
                  {r.schedule.type === "before_event" ? beforeLabel(r.schedule.minutes_before) : r.description}
                  <button
                    aria-label="Remove this reminder"
                    onClick={() => api.deleteSchedule(r.id).catch((err) => toast.error(err.message))}
                    className="flex size-5 items-center justify-center rounded-full text-faint hover:bg-fill hover:text-foreground"
                  >
                    <X className="size-3" />
                  </button>
                </span>
              ))}
            </div>
          </section>
        )}
      </div>

      <div className="flex items-center gap-2 px-6 pt-2 pb-6">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="secondary">
              <Bell /> Remind me
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuLabel className="type-footnote text-muted-foreground">
              Follows the event if it moves
            </DropdownMenuLabel>
            {before.map((b) => (
              <DropdownMenuItem key={b.minutes} onSelect={() => remind(b.minutes)}>
                {b.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <div className="flex-1" />
        <Button
          onClick={() => {
            onClose();
            onAsk(mentionDraft("event", e.id, e.title));
          }}
        >
          <Sparkles /> Ask {assistant} about this
        </Button>
      </div>
    </div>
  );
}

/** Organizer first, then guests, each address once. */
function guests(e: CalendarEvent): { person: EventPerson; organizer: boolean }[] {
  const seen = new Set<string>();
  const out: { person: EventPerson; organizer: boolean }[] = [];
  for (const [p, organizer] of [
    ...(e.organizer ? [[e.organizer, true] as const] : []),
    ...e.attendees.map((a) => [a, false] as const),
  ]) {
    if (seen.has(p.email)) continue;
    seen.add(p.email);
    out.push({ person: p, organizer });
  }
  return out;
}
