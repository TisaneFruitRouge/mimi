import { useState } from "react";
import { motion } from "motion/react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Schedule } from "@/bindings/Schedule";
import type { ScheduleItem } from "@/bindings/ScheduleItem";
import type { ScheduleKind } from "@/bindings/ScheduleKind";
import type { Weekday } from "@/bindings/Weekday";
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
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

type Repeat = Schedule["type"];

const repeats: { id: Repeat; label: string }[] = [
  { id: "once", label: "Once" },
  { id: "daily", label: "Every day" },
  { id: "weekdays", label: "Every weekday" },
  { id: "weekly", label: "Every week" },
  { id: "monthly", label: "Every month" },
  { id: "yearly", label: "Every year" },
  { id: "interval", label: "Every few minutes or hours" },
];

const weekdays: { id: Weekday; label: string }[] = [
  { id: "mon", label: "Mon" },
  { id: "tue", label: "Tue" },
  { id: "wed", label: "Wed" },
  { id: "thu", label: "Thu" },
  { id: "fri", label: "Fri" },
  { id: "sat", label: "Sat" },
  { id: "sun", label: "Sun" },
];

/** The "when" part of the form, flat so switching between kinds of repeat keeps values. */
type When = {
  repeat: Repeat;
  date: string;
  time: string;
  days: Weekday[];
  dayOfMonth: number;
  every: number;
  unit: "minutes" | "hours";
  /** Only for reminders tied to a calendar event, which are made from a chat. */
  event: { id: string; title: string; minutesBefore: number } | null;
};

const pad = (n: number) => String(n).padStart(2, "0");
const localDate = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

function fresh(): When {
  const next = new Date(Date.now() + 60 * 60_000);
  return {
    repeat: "once",
    date: localDate(next),
    time: `${pad(next.getHours())}:00`,
    days: ["mon"],
    dayOfMonth: new Date().getDate(),
    every: 30,
    unit: "minutes",
    event: null,
  };
}

function fromSchedule(s: Schedule): When {
  const w = fresh();
  switch (s.type) {
    case "once":
      return { ...w, repeat: "once", date: s.at.slice(0, 10), time: s.at.slice(11, 16) };
    case "daily":
    case "weekdays":
      return { ...w, repeat: s.type, time: s.time };
    case "weekly":
      return { ...w, repeat: "weekly", days: s.days, time: s.time };
    case "monthly":
      return { ...w, repeat: "monthly", dayOfMonth: s.day, time: s.time };
    case "yearly":
      return { ...w, repeat: "yearly", date: `${new Date().getFullYear()}-${pad(s.month)}-${pad(s.day)}`, time: s.time };
    case "interval":
      return s.minutes % 60 === 0
        ? { ...w, repeat: "interval", every: s.minutes / 60, unit: "hours" }
        : { ...w, repeat: "interval", every: s.minutes, unit: "minutes" };
    case "before_event":
      return {
        ...w,
        repeat: "before_event",
        event: { id: s.event_id, title: s.event_title, minutesBefore: s.minutes_before },
      };
  }
}

function toSchedule(w: When): Schedule {
  switch (w.repeat) {
    case "once":
      return { type: "once", at: `${w.date}T${w.time}` };
    case "daily":
    case "weekdays":
      return { type: w.repeat, time: w.time };
    case "weekly":
      return { type: "weekly", days: w.days, time: w.time };
    case "monthly":
      return { type: "monthly", day: w.dayOfMonth, time: w.time };
    case "yearly":
      return { type: "yearly", month: Number(w.date.slice(5, 7)), day: Number(w.date.slice(8, 10)), time: w.time };
    case "interval":
      return { type: "interval", minutes: w.unit === "hours" ? w.every * 60 : w.every };
    case "before_event":
      return {
        type: "before_event",
        event_id: w.event!.id,
        event_title: w.event!.title,
        minutes_before: w.event!.minutesBefore,
      };
  }
}

/** Adding a reminder or routine by hand, or changing one. */
export function ScheduleDialog({
  item,
  open,
  onClose,
  title,
}: {
  item: ScheduleItem | null;
  open: boolean;
  onClose: () => void;
  /** For a new reminder: what it's about to start with, e.g. "Call Sam". */
  title?: string;
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-5 sm:max-w-[480px]">
        {open && <ScheduleForm key={item?.id ?? "new"} item={item} initialTitle={title} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function ScheduleForm({
  item,
  initialTitle,
  onDone,
}: {
  item: ScheduleItem | null;
  initialTitle?: string;
  onDone: () => void;
}) {
  const [kind, setKind] = useState<ScheduleKind>(item?.kind ?? "reminder");
  const [title, setTitle] = useState(item?.title ?? initialTitle ?? "");
  const [instruction, setInstruction] = useState(item?.instruction ?? "");
  const [when, setWhen] = useState<When>(() => (item ? fromSchedule(item.schedule) : fresh()));
  const [busy, setBusy] = useState(false);
  const routine = kind === "routine";
  const set = (patch: Partial<When>) => setWhen((w) => ({ ...w, ...patch }));

  const pickKind = (k: ScheduleKind) => {
    setKind(k);
    // A routine usually repeats: new ones start on "every day".
    if (k === "routine" && when.repeat === "once") set({ repeat: "daily", time: "07:00" });
  };

  const ready =
    title.trim() !== "" &&
    (!routine || instruction.trim() !== "") &&
    (when.repeat !== "weekly" || when.days.length > 0) &&
    (when.repeat !== "interval" || when.every > 0);

  const save = async () => {
    setBusy(true);
    try {
      const schedule = toSchedule(when);
      if (item) {
        await api.updateSchedule(item.id, {
          title: title.trim(),
          instruction: routine ? instruction.trim() : null,
          schedule,
        });
      } else {
        await api.addSchedule({
          kind,
          title: title.trim(),
          instruction: routine ? instruction.trim() : null,
          schedule,
        });
      }
      onDone();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) save();
      }}
    >
      <DialogHeader>
        <DialogTitle>{item ? `Change ${routine ? "routine" : "reminder"}` : "New"}</DialogTitle>
        <DialogDescription>
          {routine
            ? "Your assistant does this on schedule and sends you the result. Anything it would send or change still waits for your OK."
            : "You'll get it here, on this computer, and in your messaging apps if they're connected."}
        </DialogDescription>
      </DialogHeader>

      {!item && (
        <div role="radiogroup" aria-label="Kind" className="flex rounded-[10px] bg-fill p-[3px]">
          {(["reminder", "routine"] as const).map((k) => (
            <button
              key={k}
              type="button"
              role="radio"
              aria-checked={kind === k}
              onClick={() => pickKind(k)}
              className={cn(
                "relative h-[28px] flex-1 rounded-[7px] text-[13px] font-medium transition-colors",
                kind === k ? "text-foreground" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {kind === k && (
                <motion.span
                  layoutId="schedule-kind"
                  className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                  transition={{ type: "spring", stiffness: 520, damping: 38 }}
                />
              )}
              <span className="relative">{k === "reminder" ? "Reminder" : "Routine"}</span>
            </button>
          ))}
        </div>
      )}

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="schedule-title">{routine ? "Name" : "Remind me to"}</Label>
        <Input
          id="schedule-title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder={routine ? "Morning briefing" : "Call Léa"}
          autoFocus
        />
      </div>

      {routine && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="schedule-instruction">What your assistant should do</Label>
          <Textarea
            id="schedule-instruction"
            value={instruction}
            onChange={(e) => setInstruction(e.target.value)}
            placeholder="Look at my calendar for today and tell me what's coming up."
            className="min-h-20"
          />
        </div>
      )}

      <WhenFields when={when} set={set} />

      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" disabled={!ready || busy}>
          {item ? "Save" : routine ? "Set up" : "Remind me"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function WhenFields({ when: w, set }: { when: When; set: (patch: Partial<When>) => void }) {
  if (w.repeat === "before_event" && w.event) {
    return (
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="schedule-before">Minutes before “{w.event.title}”</Label>
        <Input
          id="schedule-before"
          type="number"
          min={0}
          value={w.event.minutesBefore}
          onChange={(e) => set({ event: { ...w.event!, minutesBefore: Math.max(0, Number(e.target.value)) } })}
          className="w-28"
        />
        <p className="type-footnote text-muted-foreground">It follows the event if it moves.</p>
      </div>
    );
  }
  const time = (
    <Input
      aria-label="Time"
      type="time"
      value={w.time}
      onChange={(e) => e.target.value && set({ time: e.target.value })}
      className="w-32"
    />
  );
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1.5">
        <Label>When</Label>
        <Select value={w.repeat} onValueChange={(v) => set({ repeat: v as Repeat })}>
          <SelectTrigger className="w-full" aria-label="Repeat">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {repeats.map((r) => (
              <SelectItem key={r.id} value={r.id}>
                {r.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {w.repeat === "once" && (
        <div className="flex gap-2">
          <Input
            aria-label="Date"
            type="date"
            value={w.date}
            min={localDate(new Date())}
            onChange={(e) => e.target.value && set({ date: e.target.value })}
            className="w-44"
          />
          {time}
        </div>
      )}
      {(w.repeat === "daily" || w.repeat === "weekdays") && <div className="flex">{time}</div>}
      {w.repeat === "weekly" && (
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1" role="group" aria-label="Days">
            {weekdays.map((d) => {
              const on = w.days.includes(d.id);
              return (
                <button
                  key={d.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() =>
                    set({
                      days: on
                        ? w.days.filter((x) => x !== d.id)
                        : weekdays.map((x) => x.id).filter((x) => x === d.id || w.days.includes(x)),
                    })
                  }
                  className={cn(
                    "pressable h-8 w-10 rounded-[8px] text-[13px] font-medium transition-colors",
                    on ? "bg-foreground text-background" : "bg-fill text-muted-foreground hover:text-foreground",
                  )}
                >
                  {d.label}
                </button>
              );
            })}
          </div>
          {time}
        </div>
      )}
      {w.repeat === "monthly" && (
        <div className="flex items-center gap-2 type-callout text-muted-foreground">
          On day
          <Input
            aria-label="Day of the month"
            type="number"
            min={1}
            max={31}
            value={w.dayOfMonth}
            onChange={(e) => set({ dayOfMonth: Math.min(31, Math.max(1, Number(e.target.value))) })}
            className="w-20"
          />
          at {time}
        </div>
      )}
      {w.repeat === "yearly" && (
        <div className="flex gap-2">
          <Input
            aria-label="Date"
            type="date"
            value={w.date}
            onChange={(e) => e.target.value && set({ date: e.target.value })}
            className="w-44"
          />
          {time}
        </div>
      )}
      {w.repeat === "interval" && (
        <div className="flex items-center gap-2 type-callout text-muted-foreground">
          Every
          <Input
            aria-label="How often"
            type="number"
            min={1}
            value={w.every}
            onChange={(e) => set({ every: Math.max(1, Number(e.target.value)) })}
            className="w-20"
          />
          <Select value={w.unit} onValueChange={(v) => set({ unit: v as When["unit"] })}>
            <SelectTrigger className="w-32" aria-label="Unit">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="minutes">minutes</SelectItem>
              <SelectItem value="hours">hours</SelectItem>
            </SelectContent>
          </Select>
        </div>
      )}
      {w.repeat === "monthly" && w.dayOfMonth > 28 && (
        <p className="type-footnote text-muted-foreground">Shorter months use their last day.</p>
      )}
    </div>
  );
}
