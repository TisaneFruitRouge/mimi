import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";

import type { CalendarInfo } from "@/bindings/CalendarInfo";
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
import { api, keys } from "@/lib/api";
import { openExternal } from "@/lib/transport";

/** Adding an event by hand. `start` is where the user clicked, if they did. */
export function NewEventDialog({
  open,
  start,
  calendars,
  onClose,
}: {
  open: boolean;
  start: number | null;
  calendars: CalendarInfo[];
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-5 bg-canvas sm:max-w-[480px]">
        {open && <NewEventForm key={start ?? "now"} start={start} calendars={calendars} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function NewEventForm({
  start: clicked,
  calendars,
  onDone,
}: {
  start: number | null;
  calendars: CalendarInfo[];
  onDone: () => void;
}) {
  const qc = useQueryClient();
  const initial = clicked ?? nextHour();
  const [title, setTitle] = useState("");
  const [calendarId, setCalendarId] = useState(
    () => (calendars.find((c) => c.writable) ?? calendars[0])?.id ?? "",
  );
  const [allDay, setAllDay] = useState(false);
  const [date, setDate] = useState(inputDate(initial));
  const [time, setTime] = useState(inputTime(initial));
  const [endDate, setEndDate] = useState(inputDate(initial + 3_600_000));
  const [endTime, setEndTime] = useState(inputTime(initial + 3_600_000));
  const [location, setLocation] = useState("");
  const [notes, setNotes] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const calendar = calendars.find((c) => c.id === calendarId);
  const google = !!calendar && !calendar.writable;

  const submit = async () => {
    const startMs = allDay ? startOfDay(fromInputs(date)) : fromInputs(date, time);
    const endMs = allDay ? addDays(startOfDay(fromInputs(endDate)), 1) : fromInputs(endDate, endTime);
    if (!(endMs > startMs)) {
      setError("The event must end after it starts.");
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
      });
      if (created.open_url) {
        await openExternal(created.open_url);
        toast.success("Google Calendar is open with your event. Press Save there.");
      } else {
        toast.success(`Added to ${created.calendar}`);
        qc.invalidateQueries({ queryKey: keys.calendar });
      }
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
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

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <DialogHeader>
        <DialogTitle className="type-title">New event</DialogTitle>
        <DialogDescription>
          {google
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
          {google ? "Open in Google Calendar" : "Add"}
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
