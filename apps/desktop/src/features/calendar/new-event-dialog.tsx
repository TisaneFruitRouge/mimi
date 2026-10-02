import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Check, Loader2 } from "lucide-react";
import { toast } from "sonner";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { CalendarInfo } from "@/bindings/CalendarInfo";
import type { InvitationOffer } from "@/bindings/InvitationOffer";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { addDays, fromInputs, inputDate, inputTime, startOfDay } from "@/features/calendar/dates";
import { type GuestEntry, GuestsField, guestText } from "@/features/calendar/guests-field";
import { InvitationRow, toastOffers, useSendOffer } from "@/features/calendar/invitations";
import { optimistic } from "@/features/calendar/optimistic";
import { api, keys } from "@/lib/api";
import { openExternal } from "@/lib/transport";

/**
 * Adding an event by hand (`start` is where the user clicked, if they did), or changing
 * one (`event`). On Google calendars Google invites the guests; elsewhere they're saved
 * without anyone being emailed, and once added, the dialog offers to send the
 * invitations (once changed, a toast does: a change doesn't wait for the calendar).
 */
export function NewEventDialog({
  open,
  start,
  event,
  calendars,
  onClose,
}: {
  open: boolean;
  start: number | null;
  event?: CalendarEvent | null;
  calendars: CalendarInfo[];
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-5 bg-canvas sm:max-w-[480px]">
        {open && (
          <NewEventForm
            key={event?.id ?? start ?? "now"}
            start={start}
            event={event ?? null}
            calendars={calendars}
            onDone={onClose}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

/** The user themselves, among an event's attendees: not a guest to show or to invite. */
function guestsOf(e: CalendarEvent): GuestEntry[] {
  const organizer = e.organizer?.email;
  return e.attendees
    .filter((a) => a.email !== organizer)
    .map((a) => ({ name: a.person_name ?? a.name, email: a.email }));
}

/** The attendees after a change of guests: those who stay keep their answer. */
function withGuests(e: CalendarEvent, guests: GuestEntry[]) {
  const organizer = e.attendees.filter((a) => a.email === e.organizer?.email);
  return organizer.concat(
    guests.map(
      (g) =>
        e.attendees.find((a) => a.email.toLowerCase() === g.email.toLowerCase()) ?? {
          name: g.name,
          email: g.email,
          person_id: null,
          person_name: null,
          response: null,
        },
    ),
  );
}

function NewEventForm({
  start: clicked,
  event,
  calendars,
  onDone,
}: {
  start: number | null;
  event: CalendarEvent | null;
  calendars: CalendarInfo[];
  onDone: () => void;
}) {
  const qc = useQueryClient();
  const send = useSendOffer();
  const initial = event?.start ?? clicked ?? nextHour();
  const initialEnd = event
    ? event.all_day
      ? addDays(event.end, -1)
      : event.end
    : initial + 3_600_000;
  const [title, setTitle] = useState(event?.title ?? "");
  const [calendarId, setCalendarId] = useState(
    () => event?.calendar_id ?? (calendars.find((c) => c.writable) ?? calendars[0])?.id ?? "",
  );
  const [allDay, setAllDay] = useState(event?.all_day ?? false);
  const [date, setDate] = useState(inputDate(initial));
  const [time, setTime] = useState(inputTime(initial));
  const [endDate, setEndDate] = useState(inputDate(initialEnd));
  const [endTime, setEndTime] = useState(inputTime(initialEnd));
  const [location, setLocation] = useState(event?.location ?? "");
  const [notes, setNotes] = useState(event?.notes ?? "");
  const [guests, setGuests] = useState<GuestEntry[]>(() => (event ? guestsOf(event) : []));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Once saved: what the guests could be sent.
  const [offers, setOffers] = useState<InvitationOffer[] | null>(null);

  const calendar = calendars.find((c) => c.id === calendarId);
  const google = !!calendar && !calendar.writable;
  const editing = !!event;
  // Guests only where they can be invited, and only on the user's own events.
  const canInvite = !!calendar?.guests && (!event || event.mine);

  const submit = async () => {
    const startMs = allDay ? startOfDay(fromInputs(date)) : fromInputs(date, time);
    const endMs = allDay ? addDays(startOfDay(fromInputs(endDate)), 1) : fromInputs(endDate, endTime);
    if (!(endMs > startMs)) {
      setError("The event must end after it starts.");
      return;
    }
    if (event) {
      saveChange(event, { title, start: startMs, end: endMs, all_day: allDay, location, notes });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const created = await api.addEvent({
        calendar_id: calendarId,
        title,
        start: startMs,
        end: endMs,
        all_day: allDay,
        location: location || null,
        notes: notes || null,
        guests: canInvite ? guests.map(guestText) : [],
      });
      if (created.open_url) {
        await openExternal(created.open_url);
        toast.success("Google Calendar is open with your event. Press Save there.");
        onDone();
        return;
      }
      qc.invalidateQueries({ queryKey: keys.calendar });
      if (created.note) toast.info(created.note);
      if (created.invitations.length > 0) setOffers(created.invitations);
      else {
        toast.success(`Added to ${created.calendar}`);
        onDone();
      }
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  // A change shows in the calendar at once and the dialog closes; the calendar's answer
  // (a note, invitations to offer, or a refusal that puts the event back) comes as a toast.
  const saveChange = (
    event: CalendarEvent,
    fields: Pick<CalendarEvent, "title" | "start" | "end" | "all_day" | "location" | "notes">,
  ) => {
    const change = { ...fields, guests: canInvite ? guests.map(guestText) : null };
    const edited: CalendarEvent = {
      ...event,
      ...fields,
      location: fields.location || null,
      notes: fields.notes || null,
      attendees: canInvite ? withGuests(event, guests) : event.attendees,
    };
    const shown = toast.success("Saved");
    onDone();
    optimistic(
      qc,
      (events, from, to) =>
        events
          .filter((e) => e.id !== event.id)
          .concat(edited.start < to && edited.end > from ? [edited] : [])
          .sort((a, b) => a.start - b.start),
      () => api.changeEvent(event.id, change),
    )
      .then((changed) => {
        if (changed.invitations.length > 0) toastOffers("Saved", changed.invitations, send, shown);
        if (changed.note) toast.info(changed.note);
      })
      .catch((e) => toast.error(`Your change to “${event.title}” wasn't saved. ${(e as Error).message}`, { id: shown }));
  };

  // Moving the start keeps the length the same.
  const moveStart = (d: string, t: string) => {
    const before = fromInputs(date, time);
    const length = Math.max(fromInputs(endDate, endTime) - before, allDay ? 0 : 15 * 60_000);
    setDate(d);
    setTime(t);
    const after = fromInputs(d, t) + length;
    setEndDate(inputDate(after));
    setEndTime(inputTime(after));
  };

  if (offers)
    return (
      <div className="flex flex-col gap-5">
        <DialogHeader>
          <span className="flex size-9 items-center justify-center rounded-full bg-private-soft text-private">
            <Check className="size-[18px]" strokeWidth={2.6} />
          </span>
          <DialogTitle className="type-title">{`Added to ${calendar?.name ?? "your calendar"}`}</DialogTitle>
          <DialogDescription>Nothing has been emailed to your guests yet.</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 rounded-[14px] bg-background p-4 shadow-[var(--shadow-card)]">
          {offers.map((o) => (
            <InvitationRow key={o.id} offer={o} />
          ))}
        </div>
        <DialogFooter>
          <Button variant="secondary" onClick={onDone}>
            Done
          </Button>
        </DialogFooter>
      </div>
    );

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <DialogHeader>
        <DialogTitle className="type-title">{editing ? "Edit event" : "New event"}</DialogTitle>
        <DialogDescription>
          {editing
            ? event.repeats
              ? "Changes only this time. The rest of the series stays as it is."
              : `In ${event.calendar}.`
            : google
              ? "Google Calendar opens with this filled in, for you to save."
              : "Saved straight into your calendar."}
        </DialogDescription>
      </DialogHeader>

      <div className="flex flex-col gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="event-title">What</Label>
          <Input
            id="event-title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="Dinner with Sam"
            autoFocus
          />
        </div>

        {!editing && (
          <div className="flex flex-col gap-1.5">
            <Label>Calendar</Label>
            <Select value={calendarId} onValueChange={setCalendarId}>
              <SelectTrigger aria-label="Calendar">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {calendars.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    <span className="size-2.5 rounded-full" style={{ background: c.color }} aria-hidden />
                    {c.name}
                    {!c.writable && <span className="text-faint"> · opens Google Calendar</span>}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        <div className="grouped flex flex-col">
          <label className="flex h-11 items-center justify-between px-4 type-callout">
            All day
            <Switch checked={allDay} onCheckedChange={setAllDay} aria-label="All day" />
          </label>
          <div className="flex h-12 items-center gap-2 px-4 type-callout">
            <span className="w-14 shrink-0">Starts</span>
            <Input
              type="date"
              value={date}
              onChange={(e) => e.target.value && moveStart(e.target.value, time)}
              aria-label="Start date"
              className="h-8"
            />
            {!allDay && (
              <Input
                type="time"
                value={time}
                onChange={(e) => e.target.value && moveStart(date, e.target.value)}
                aria-label="Start time"
                className="h-8 w-[136px] shrink-0"
              />
            )}
          </div>
          <div className="flex h-12 items-center gap-2 px-4 type-callout">
            <span className="w-14 shrink-0">Ends</span>
            <Input
              type="date"
              value={endDate}
              onChange={(e) => e.target.value && setEndDate(e.target.value)}
              aria-label="End date"
              className="h-8"
            />
            {!allDay && (
              <Input
                type="time"
                value={endTime}
                onChange={(e) => e.target.value && setEndTime(e.target.value)}
                aria-label="End time"
                className="h-8 w-[136px] shrink-0"
              />
            )}
          </div>
        </div>

        <Input value={location} onChange={(e) => setLocation(e.target.value)} placeholder="Place (optional)" aria-label="Place" />

        {canInvite ? (
          <div className="flex flex-col gap-1.5">
            <Label>Guests</Label>
            <GuestsField value={guests} onChange={setGuests} />
            <p className="type-footnote text-faint">
              {calendar?.google && calendar.writable
                ? "Google emails them the invitation when you save, and it shows in their calendar."
                : "Nobody is emailed when you save. You can send the invitations next."}
            </p>
          </div>
        ) : (
          editing &&
          !event.mine &&
          event.organizer && (
            <p className="type-footnote text-faint">
              {event.organizer.person_name ?? event.organizer.name ?? event.organizer.email} organizes this
              event, so only they can change its guests.
            </p>
          )
        )}

        <Textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Notes (optional)"
          aria-label="Notes"
          rows={3}
        />
        {error && <p className="type-subhead text-destructive">{error}</p>}
      </div>

      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" disabled={busy || !title.trim() || !calendarId}>
          {busy && <Loader2 className="animate-spin" />}
          {editing ? "Save" : google ? "Open in Google Calendar" : "Add"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function nextHour() {
  const d = new Date(Date.now() + 3_600_000);
  d.setMinutes(0, 0, 0);
  return d.getTime();
}
