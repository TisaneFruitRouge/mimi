import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import {
  Bell,
  BellOff,
  CalendarDays,
  CalendarPlus,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clock,
  Copy,
  Eye,
  EyeOff,
  Loader2,
  MapPin,
  PanelRightOpen,
  Plus,
  Sparkles,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { CalendarInfo } from "@/bindings/CalendarInfo";
import type { ScheduleItem } from "@/bindings/ScheduleItem";
import type { ScheduleOccurrence } from "@/bindings/ScheduleOccurrence";
import { Grouped, IconTile } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  DAY,
  type Placed,
  addDays,
  dayTitle,
  layoutDay,
  monthTitle,
  onDay,
  sameDay,
  startOfDay,
  startOfWeek,
  timeRange,
  whenLong,
} from "@/features/calendar/dates";
import { EventSheet, before, remindBefore, remindersFor } from "@/features/calendar/event-sheet";
import { NewEventDialog } from "@/features/calendar/new-event-dialog";
import {
  DeleteItemDialog,
  ItemMenu,
  RemindersPanel,
  finished,
  statusText,
  useScheduleItems,
} from "@/features/reminders/reminders";
import { clock } from "@/features/reminders/time";
import type { Section } from "@/features/shell/top-bar";
import { api, keys } from "@/lib/api";
import { type Draft, mentionDraft } from "@/lib/draft";
import { copyText } from "@/components/app-context-menu";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useAssistantName } from "@/lib/queries";
import { useScrollEdge } from "@/lib/scroll-edge";

/** Height of one hour in the week grid. */
const HOUR = 44;
/** Height of a reminder chip in the week grid. */
const CHIP = 20;
const AGENDA_DAYS = 28;
const GREY = "#8e8e93";

type View = "week" | "agenda";

/** Per-viewer conveniences; the page works the same without them. */
function stored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}
function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Private windows and blocked storage: nothing to remember then.
  }
}

/** Minutes since midnight, refreshed every minute, for the "now" line. */
function useNow() {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(t);
  }, []);
  return now;
}

/**
 * The half hour under a right-click on an empty part of the week grid, or `null`
 * elsewhere (an event, the agenda, a header).
 */
function slotAt(target: EventTarget | null, clientY: number): number | null {
  if (!(target instanceof HTMLElement) || !target.dataset.day) return null;
  const y = clientY - target.getBoundingClientRect().top;
  return Number(target.dataset.day) + Math.floor((y / HOUR) * 2) * 30 * 60_000;
}

/** "on Tue 14:30" for a slot's New event. */
function slotLabel(at: number) {
  return `on ${new Date(at).toLocaleDateString([], { weekday: "short" })} ${clock(at)}`;
}

/** A reminder or routine at one of its times. */
interface Timed {
  item: ScheduleItem;
  o: ScheduleOccurrence;
}

const upcoming = (item_id: string, at: number): ScheduleOccurrence => ({
  item_id,
  at,
  until: at,
  count: 1,
  status: null,
  snoozed: false,
});

/** "7:00", or "9:00–23:30" for a day of something that repeats every few minutes. */
const timedClock = ({ o }: Timed) => (o.count > 1 ? `${clock(o.at)}–${clock(o.until)}` : clock(o.at));

/** What happened to it, or will: "Sent", "Missed", "Snoozed", nothing for one to come. */
function timedState({ o }: Timed) {
  if (o.snoozed) return "Snoozed";
  return o.status ? statusText[o.status] : null;
}

/** The tooltip: "Water the plants · 18:00 · Sent", "Drink water · 9:00–23:30 · Every 30 minutes". */
function timedLabel(t: Timed) {
  const repeats = t.o.count > 1 ? t.item.description : null;
  return [t.item.title, timedClock(t), repeats, timedState(t)].filter(Boolean).join(" · ");
}

/** Didn't happen: missed while Mimi wasn't running, or a routine run that was skipped. */
const notHappened = ({ o }: Timed) => o.status === "missed" || o.status === "skipped";

function TimedIcon({ t, className }: { t: Timed; className: string }) {
  const past = t.o.status !== null;
  if (t.o.snoozed) return <Clock className={cn(className, "text-muted-foreground")} />;
  if (t.item.kind === "routine")
    return <Sparkles className={cn(className, past ? "text-muted-foreground" : "text-lime-deep")} />;
  return <Bell className={cn(className, "text-muted-foreground")} />;
}

/** What the right-click menus in every view can do, provided by the panel. */
interface CalendarActions {
  ask: (e: CalendarEvent) => void;
  hide: (e: CalendarEvent) => void;
  editItem: (i: ScheduleItem) => void;
  deleteItem: (i: ScheduleItem) => void;
  openConversation: (id: string) => void;
}
const CalendarActionsContext = createContext<CalendarActions | null>(null);

/** An event's details as plain text, for Copy. */
function eventText(e: CalendarEvent) {
  return [e.title, whenLong(e), e.location].filter(Boolean).join("\n");
}

/** Right-click on an event: open it, be reminded, ask about it, copy it, hide its calendar. */
function EventMenu({
  event: e,
  onOpen,
  children,
}: {
  event: CalendarEvent;
  onOpen: (e: CalendarEvent) => void;
  children: React.ReactElement;
}) {
  const actions = useContext(CalendarActionsContext);
  const assistant = useAssistantName();
  const reminders = remindersFor(useScheduleItems().data ?? [], e.id);
  const removeReminders = () =>
    Promise.all(reminders.map((r) => api.deleteSchedule(r.id)))
      .then(() => toast.success(reminders.length > 1 ? "Reminders removed" : "Reminder removed"))
      .catch((err) => toast.error((err as Error).message));
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-[230px]">
        <ContextMenuItem onSelect={() => onOpen(e)}>
          <PanelRightOpen /> Open
        </ContextMenuItem>
        {/* Only before it starts: a reminder for something under way is refused. */}
        {e.start > Date.now() && (
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <Bell /> Remind me
            </ContextMenuSubTrigger>
            <ContextMenuSubContent className="w-[210px]">
              <ContextMenuLabel className="type-footnote font-normal text-muted-foreground">
                Follows the event if it moves
              </ContextMenuLabel>
              {before.map((b) => (
                <ContextMenuItem key={b.minutes} onSelect={() => remindBefore(e, b.minutes)}>
                  {b.label}
                </ContextMenuItem>
              ))}
            </ContextMenuSubContent>
          </ContextMenuSub>
        )}
        {reminders.length > 0 && (
          <ContextMenuItem onSelect={removeReminders}>
            <BellOff /> {reminders.length > 1 ? "Remove its reminders" : "Remove its reminder"}
          </ContextMenuItem>
        )}
        {actions && (
          <ContextMenuItem onSelect={() => actions.ask(e)}>
            <Sparkles /> Ask {assistant} about this
          </ContextMenuItem>
        )}
        <ContextMenuItem onSelect={() => copyText(eventText(e))}>
          <Copy /> Copy details
        </ContextMenuItem>
        {actions && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => actions.hide(e)}>
              <EyeOff /> Hide “{e.calendar}”
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** Right-click on a reminder or routine shown in the calendar. */
function CalendarItemMenu({ item, children }: { item: ScheduleItem; children: React.ReactElement }) {
  const actions = useContext(CalendarActionsContext);
  if (!actions) return children;
  return (
    <ItemMenu
      item={item}
      onEdit={() => actions.editItem(item)}
      onDelete={() => actions.deleteItem(item)}
      onOpenConversation={actions.openConversation}
    >
      {children}
    </ItemMenu>
  );
}

/** Every calendar merged into one week or agenda, with reminders beside and inside it. */
export function CalendarView({
  onAsk,
  onOpenPerson,
  onOpenConversation,
  onSection,
}: {
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
  onOpenConversation: (id: string) => void;
  onSection: (s: Section) => void;
}) {
  const now = useNow();
  const [weekStart, setWeekStart] = useState(() => startOfWeek(Date.now()));
  const [view, setView] = useState<View>(() => stored<View>("mimi.calendar.view", "week"));
  const [hidden, setHidden] = useState<string[]>(() => stored<string[]>("mimi.calendar.hidden", []));
  const [open, setOpen] = useState<CalendarEvent | null>(null);
  const [creating, setCreating] = useState<{ start: number | null } | null>(null);
  const [editing, setEditing] = useState<ScheduleItem | "new" | null>(null);
  const [deletingItem, setDeletingItem] = useState<ScheduleItem | null>(null);
  // Where the calendar was right-clicked: a time in the week grid, if on an empty slot.
  const [slot, setSlot] = useState<number | null>(null);
  const [sideOpen, setSideOpen] = useState(false);

  const thisWeek = startOfWeek(now);
  const from = view === "week" ? weekStart : weekStart === thisWeek ? startOfDay(now) : weekStart;
  const to = view === "week" ? addDays(weekStart, 7) : addDays(from, AGENDA_DAYS);

  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars });
  const events = useQuery({
    queryKey: keys.events(from, to),
    queryFn: () => api.events(from, to),
    placeholderData: keepPreviousData,
    staleTime: 60_000,
    enabled: (calendars.data?.length ?? 0) > 0,
  });
  const items = useScheduleItems().data ?? [];
  const occurrences = useQuery({
    queryKey: keys.occurrences(from, to),
    queryFn: () => api.scheduleOccurrences(from, to),
    placeholderData: keepPreviousData,
  });

  const colors = useMemo(
    () => new Map((calendars.data ?? []).map((c) => [c.id, c.color])),
    [calendars.data],
  );
  const colorOf = (e: CalendarEvent) => colors.get(e.calendar_id) ?? GREY;
  const visible = (events.data?.events ?? []).filter((e) => !hidden.includes(e.calendar_id));
  // Reminders at times of their own, every one in range, shown in the grid too.
  // Event-relative ones show as a bell on their event instead.
  const timed = useMemo(() => {
    const own = (i: ScheduleItem) => i.schedule.type !== "before_event";
    if (occurrences.isError) {
      // An older daemon: only each item's next time.
      return items
        .filter((i) => own(i) && !finished(i) && !i.paused && i.next_at !== null)
        .map((item) => ({ item, o: upcoming(item.id, item.next_at!) }));
    }
    const byId = new Map(items.map((i) => [i.id, i]));
    return (occurrences.data ?? []).flatMap((o) => {
      const item = byId.get(o.item_id);
      return item && own(item) ? [{ item, o }] : [];
    });
  }, [items, occurrences.data, occurrences.isError]);

  const setViewSaved = (v: View) => {
    setView(v);
    store("mimi.calendar.view", v);
  };
  const toggle = (id: string) => {
    const next = hidden.includes(id) ? hidden.filter((h) => h !== id) : [...hidden, id];
    setHidden(next);
    store("mimi.calendar.hidden", next);
  };

  // ← → move a week, T comes back to today, unless typing or in a dialog.
  const nav = useRef({ weekStart });
  nav.current = { weekStart };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, select, [contenteditable], [role=dialog], [role=menu], [role=listbox]"))
        return;
      if (e.key === "ArrowLeft") setWeekStart(addDays(nav.current.weekStart, -7));
      else if (e.key === "ArrowRight") setWeekStart(addDays(nav.current.weekStart, 7));
      else if (e.key.toLowerCase() === "t") setWeekStart(startOfWeek(Date.now()));
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const noCalendars = calendars.isSuccess && calendars.data.length === 0;

  return (
    <CalendarActionsContext.Provider
      value={{
        ask: (e) => onAsk(mentionDraft("event", e.id, e.title)),
        hide: (e) => {
          if (!hidden.includes(e.calendar_id)) toggle(e.calendar_id);
          toast(`“${e.calendar}” is hidden`, { description: "Show it again from Calendars, at the top." });
        },
        editItem: setEditing,
        deleteItem: setDeletingItem,
        openConversation: onOpenConversation,
      }}
    >
      <div className="flex h-full flex-col px-6 pt-[68px] pb-5">
        <header className="flex flex-wrap items-center gap-x-3 gap-y-2 pb-4">
          <h1 className="min-w-[200px] type-title">{monthTitle(view === "week" ? weekStart : from)}</h1>
          <div className="flex items-center gap-0.5">
            <NavButton label="Previous week" onClick={() => setWeekStart(addDays(weekStart, -7))}>
              <ChevronLeft />
            </NavButton>
            <Button
              variant="secondary"
              size="sm"
              className="rounded-full px-3.5"
              disabled={weekStart === thisWeek}
              onClick={() => setWeekStart(thisWeek)}
            >
              Today
            </Button>
            <NavButton label="Next week" onClick={() => setWeekStart(addDays(weekStart, 7))}>
              <ChevronRight />
            </NavButton>
          </div>
          {events.isFetching && <Loader2 className="size-4 animate-spin text-faint" aria-label="Loading" />}
          <div className="flex-1" />
          <ViewSwitch value={view} onChange={setViewSaved} />
          {(calendars.data?.length ?? 0) > 0 && (
            <CalendarsPopover calendars={calendars.data!} hidden={hidden} onToggle={toggle} />
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Reminders and routines"
            aria-expanded={sideOpen}
            className={cn("rounded-full text-muted-foreground min-[1100px]:hidden", sideOpen && "bg-fill text-foreground")}
            onClick={() => setSideOpen((o) => !o)}
          >
            <Bell />
          </Button>
          <Button size="sm" disabled={!calendars.data?.length} onClick={() => setCreating({ start: null })}>
            <Plus /> New event
          </Button>
        </header>

        {(events.data?.unavailable.length ?? 0) > 0 && (
          <p className="-mt-1 mb-3 flex items-center gap-1.5 type-subhead text-cloud">
            <CircleAlert className="size-3.5 shrink-0" />
            <span className="truncate">Couldn't read {events.data!.unavailable.join("; ")}</span>
          </p>
        )}

        <div className="relative flex min-h-0 flex-1 gap-5">
          <ContextMenu>
            <ContextMenuTrigger asChild onContextMenu={(ev) => setSlot(slotAt(ev.target, ev.clientY))}>
              <div className="min-w-0 flex-1">
                {noCalendars ? (
                  <NoCalendars onConnect={() => onSection("connections")} />
                ) : calendars.isLoading ? (
                  <Skeleton className="h-full rounded-[18px]" />
                ) : view === "week" ? (
                  <WeekGrid
                    weekStart={weekStart}
                    now={now}
                    events={visible}
                    reminders={timed}
                    items={items}
                    loading={events.isLoading || events.isPlaceholderData}
                    colorOf={colorOf}
                    onOpen={setOpen}
                    onCreate={(start) => setCreating({ start })}
                    onReminder={setEditing}
                  />
                ) : (
                  <Agenda
                    from={from}
                    to={to}
                    events={visible}
                    reminders={timed}
                    items={items}
                    loading={events.isLoading}
                    colorOf={colorOf}
                    onOpen={setOpen}
                    onReminder={setEditing}
                  />
                )}
              </div>
            </ContextMenuTrigger>
            <ContextMenuContent className="w-[240px]">
              <ContextMenuItem disabled={!calendars.data?.length} onSelect={() => setCreating({ start: slot })}>
                <CalendarPlus /> {slot === null ? "New event…" : `New event ${slotLabel(slot)}…`}
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => setEditing("new")}>
                <Bell /> New reminder…
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem onSelect={() => setWeekStart(thisWeek)}>
                <CalendarDays /> Today <ContextMenuShortcut>T</ContextMenuShortcut>
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => setWeekStart(addDays(weekStart, -7))}>
                <ChevronLeft /> Previous week <ContextMenuShortcut>←</ContextMenuShortcut>
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => setWeekStart(addDays(weekStart, 7))}>
                <ChevronRight /> Next week <ContextMenuShortcut>→</ContextMenuShortcut>
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuRadioGroup value={view} onValueChange={(v) => setViewSaved(v as View)}>
                <ContextMenuRadioItem value="week">Week</ContextMenuRadioItem>
                <ContextMenuRadioItem value="agenda">Agenda</ContextMenuRadioItem>
              </ContextMenuRadioGroup>
            </ContextMenuContent>
          </ContextMenu>
          <aside
            className={cn(
              "w-[300px] shrink-0 overflow-y-auto pb-2",
              "max-[1099px]:absolute max-[1099px]:top-0 max-[1099px]:right-0 max-[1099px]:z-20 max-[1099px]:max-h-full max-[1099px]:rounded-[18px] max-[1099px]:bg-canvas max-[1099px]:p-3 max-[1099px]:shadow-[var(--shadow-float)]",
              !sideOpen && "max-[1099px]:hidden",
            )}
          >
            <RemindersPanel onOpenConversation={onOpenConversation} editing={editing} onEdit={setEditing} />
          </aside>
        </div>

        <EventSheet
          event={open}
          color={open ? colorOf(open) : GREY}
          onClose={() => setOpen(null)}
          onAsk={onAsk}
          onOpenPerson={onOpenPerson}
        />
        <DeleteItemDialog item={deletingItem} onClose={() => setDeletingItem(null)} />
        <NewEventDialog
          open={!!creating}
          start={creating?.start ?? null}
          calendars={calendars.data ?? []}
          onClose={() => setCreating(null)}
        />
      </div>
    </CalendarActionsContext.Provider>
  );
}

function NavButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={label} className="rounded-full" onClick={onClick}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {label} <span className="ml-1 opacity-60">{label.startsWith("Previous") ? "←" : "→"}</span>
      </TooltipContent>
    </Tooltip>
  );
}

function ViewSwitch({ value, onChange }: { value: View; onChange: (v: View) => void }) {
  return (
    <div role="radiogroup" aria-label="View" className="flex rounded-[9px] bg-fill p-[2px]">
      {(["week", "agenda"] as const).map((v) => (
        <button
          key={v}
          role="radio"
          aria-checked={value === v}
          onClick={() => onChange(v)}
          className={cn(
            "relative h-7 rounded-[7px] px-3 text-[13px] font-medium transition-colors",
            value === v ? "text-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {value === v && (
            <motion.span
              layoutId="calendar-view"
              className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
              transition={{ type: "spring", stiffness: 520, damping: 38 }}
            />
          )}
          <span className="relative">{v === "week" ? "Week" : "List"}</span>
        </button>
      ))}
    </div>
  );
}

function CalendarsPopover({
  calendars,
  hidden,
  onToggle,
}: {
  calendars: CalendarInfo[];
  hidden: string[];
  onToggle: (id: string) => void;
}) {
  const shown = calendars.filter((c) => !hidden.includes(c.id)).length;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="secondary" size="sm" className="rounded-full">
          <Eye /> {shown === calendars.length ? "All calendars" : `${shown} of ${calendars.length}`}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[260px] p-1.5">
        <p className="px-2.5 pt-1.5 pb-1 type-footnote text-muted-foreground">Show calendars</p>
        {calendars.map((c) => {
          const on = !hidden.includes(c.id);
          return (
            <button
              key={c.id}
              role="menuitemcheckbox"
              aria-checked={on}
              onClick={() => onToggle(c.id)}
              className="flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-[7px] text-left text-[14px] hover:bg-fill"
            >
              <span
                className="size-3.5 shrink-0 rounded-[4px] border-2 transition-colors"
                style={{ borderColor: c.color, background: on ? c.color : "transparent" }}
                aria-hidden
              />
              <span className="min-w-0 flex-1 truncate">{c.name}</span>
              {c.google && <span className="type-footnote text-faint">Google</span>}
            </button>
          );
        })}
      </PopoverContent>
    </Popover>
  );
}

function NoCalendars({ onConnect }: { onConnect: () => void }) {
  return (
    <div className="surface flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
      <IconTile size="lg" className="bg-event-soft text-event">
        <CalendarDays />
      </IconTile>
      <p className="type-headline">Connect a calendar</p>
      <p className="max-w-sm type-callout text-muted-foreground">
        Google, iCloud, Fastmail or Nextcloud. Your events show here, and your assistant can
        answer questions about them.
      </p>
      <Button size="sm" className="mt-1" onClick={onConnect}>
        Open Connections
      </Button>
    </div>
  );
}

/** Tinted block colours from a calendar colour: readable on white, never garish. */
const tint = (color: string) => ({
  background: `color-mix(in srgb, ${color} 15%, white)`,
  color: `color-mix(in srgb, ${color} 55%, #1d1d1f)`,
  borderColor: color,
});

function WeekGrid({
  weekStart,
  now,
  events,
  reminders,
  items,
  loading,
  colorOf,
  onOpen,
  onCreate,
  onReminder,
}: {
  weekStart: number;
  now: number;
  events: CalendarEvent[];
  reminders: Timed[];
  items: ScheduleItem[];
  loading: boolean;
  colorOf: (e: CalendarEvent) => string;
  onOpen: (e: CalendarEvent) => void;
  onCreate: (start: number) => void;
  onReminder: (i: ScheduleItem) => void;
}) {
  const days = Array.from({ length: 7 }, (_, i) => addDays(weekStart, i));
  const scroller = useRef<HTMLDivElement>(null);
  const onScroll = useScrollEdge();

  // Open around now in the current week; other weeks from their first morning event.
  // Once per week shown, when its events are in, not on every refresh.
  const scrolledFor = useRef<number | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || loading || scrolledFor.current === weekStart) return;
    scrolledFor.current = weekStart;
    const thisWeek = days.some((d) => sameDay(d, Date.now()));
    const first = Math.min(
      8,
      ...events.filter((e) => !e.all_day && e.start >= weekStart).map((e) => new Date(e.start).getHours()),
      ...reminders.map((r) => new Date(r.o.at).getHours()),
    );
    const hour = thisWeek ? new Date().getHours() - 1.5 : first - 0.5;
    el.scrollTop = Math.max(0, hour) * HOUR;
  });

  const allDay = days.map((d) => events.filter((e) => e.all_day && onDay(e, d)));
  const hasAllDay = allDay.some((d) => d.length > 0);
  const cols = "grid grid-cols-[52px_repeat(7,minmax(0,1fr))]";

  return (
    <div className="surface flex h-full flex-col overflow-hidden">
      <div className={cn(cols, "shrink-0 shadow-[inset_0_-0.5px_0_var(--separator)]")}>
        <div />
        {days.map((d) => {
          const today = sameDay(d, now);
          const date = new Date(d);
          return (
            <div key={d} className="flex items-baseline justify-center gap-1.5 py-2.5 type-subhead" aria-current={today ? "date" : undefined}>
              <span className={cn("text-muted-foreground", today && "text-[#ff3b30]")}>
                {date.toLocaleDateString([], { weekday: "short" })}
              </span>
              <span
                className={cn(
                  "inline-flex min-w-6 items-center justify-center rounded-full px-1 type-callout font-semibold tabular-nums",
                  today && "bg-[#ff3b30] text-white",
                )}
              >
                {date.getDate()}
              </span>
            </div>
          );
        })}
      </div>

      {hasAllDay && (
        <div className={cn(cols, "shrink-0 shadow-[inset_0_-0.5px_0_var(--separator)]")}>
          <div className="pt-1.5 pr-2 text-right type-footnote text-faint">all-day</div>
          {allDay.map((list, i) => (
            <div key={days[i]} className="flex min-w-0 flex-col gap-0.5 px-0.5 py-1 shadow-[inset_0.5px_0_0_var(--separator)]">
              {list.slice(0, 3).map((e) => (
                <EventMenu key={e.id} event={e} onOpen={onOpen}>
                  <button
                    onClick={() => onOpen(e)}
                    className="truncate rounded-[6px] border-l-[3px] px-1.5 py-[1px] text-left text-[12px] font-medium"
                    style={tint(colorOf(e))}
                  >
                    {e.title}
                  </button>
                </EventMenu>
              ))}
              {list.length > 3 && <span className="px-1.5 type-footnote text-muted-foreground">{list.length - 3} more</span>}
            </div>
          ))}
        </div>
      )}

      <div ref={scroller} onScroll={onScroll} className="relative min-h-0 flex-1 overflow-y-auto">
        <div className={cn(cols, "relative")} style={{ height: 24 * HOUR }}>
          <div className="relative">
            {Array.from({ length: 23 }, (_, h) => (
              <span
                key={h}
                className="absolute right-2 -translate-y-1/2 type-footnote text-faint tabular-nums"
                style={{ top: (h + 1) * HOUR }}
              >
                {clock(new Date(2000, 0, 1, h + 1).getTime())}
              </span>
            ))}
          </div>
          {/* Hour lines across the whole week, under the events. */}
          <div className="pointer-events-none absolute inset-0 col-start-2 col-end-9" aria-hidden>
            {Array.from({ length: 23 }, (_, h) => (
              <div key={h} className="absolute inset-x-0 h-px bg-separator" style={{ top: (h + 1) * HOUR }} />
            ))}
          </div>
          {days.map((d) => (
            <DayColumn
              key={d}
              day={d}
              now={now}
              placed={layoutDay(events, d)}
              reminders={reminders.filter((r) => r.o.at >= d && r.o.at < addDays(d, 1))}
              items={items}
              colorOf={colorOf}
              onOpen={onOpen}
              onCreate={onCreate}
              onReminder={onReminder}
            />
          ))}
        </div>
        {loading && events.length === 0 && (
          <div className="absolute inset-0 flex items-start justify-center pt-24">
            <Loader2 className="size-5 animate-spin text-faint" />
          </div>
        )}
      </div>
    </div>
  );
}

function DayColumn({
  day,
  now,
  placed,
  reminders,
  items,
  colorOf,
  onOpen,
  onCreate,
  onReminder,
}: {
  day: number;
  now: number;
  placed: Placed[];
  reminders: Timed[];
  items: ScheduleItem[];
  colorOf: (e: CalendarEvent) => string;
  onOpen: (e: CalendarEvent) => void;
  onCreate: (start: number) => void;
  onReminder: (i: ScheduleItem) => void;
}) {
  const today = sameDay(day, now);
  const weekend = [0, 6].includes(new Date(day).getDay());
  // Each chip centred on its time, kept inside the day and below the one before it, so
  // reminders a few minutes apart don't hide each other.
  const chipTops: number[] = [];
  for (const r of reminders) {
    const ideal = Math.max(0, ((r.o.at - day) / 3_600_000) * HOUR - 10);
    const prev = chipTops.at(-1);
    chipTops.push(Math.min(24 * HOUR - CHIP, prev === undefined ? ideal : Math.max(ideal, prev + CHIP + 1)));
  }
  return (
    <div
      data-day={day}
      className={cn("relative shadow-[inset_0.5px_0_0_var(--separator)]", weekend && "bg-[rgb(118_118_128/0.03)]")}
      onDoubleClick={(e) => {
        if (e.target !== e.currentTarget) return;
        const y = e.nativeEvent.offsetY;
        const minutes = Math.floor((y / HOUR) * 2) * 30;
        onCreate(day + minutes * 60_000);
      }}
      title="Double-click to add an event"
    >
      {placed.map((p) => {
        const e = p.event;
        const height = ((p.bottom - p.top) / 60) * HOUR - 2;
        const reminded = remindersFor(items, e.id).length > 0;
        return (
          <EventMenu key={e.id} event={e} onOpen={onOpen}>
            <button
              onClick={() => onOpen(e)}
              className="absolute flex flex-col overflow-hidden rounded-[7px] border-l-[3px] px-1.5 py-[3px] text-left transition-[filter] hover:brightness-[0.97] focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-ring/45 focus-visible:outline-none"
              style={{
                ...tint(colorOf(e)),
                top: (p.top / 60) * HOUR + 1,
                height,
                left: `calc(${(p.column / p.columns) * 100}% + 2px)`,
                width: `calc(${100 / p.columns}% - 4px)`,
              }}
            >
              <span className="flex items-center gap-1 text-[12px] leading-tight font-semibold">
                <span className="truncate">{e.title}</span>
                {reminded && <Bell className="size-2.5 shrink-0" aria-label="Reminder set" />}
              </span>
              {height > 32 && (
                <span className="truncate text-[11px] leading-tight opacity-80">
                  {e.start >= day ? clock(e.start) : ""}
                  {e.location ? ` · ${e.location}` : ""}
                </span>
              )}
            </button>
          </EventMenu>
        );
      })}
      {reminders.map((r, i) => (
        <CalendarItemMenu key={`${r.item.id}:${r.o.at}:${r.o.snoozed}`} item={r.item}>
          <button
            onClick={() => onReminder(r.item)}
            className={cn(
              "absolute inset-x-1 z-[1] flex h-5 items-center gap-1 rounded-full bg-background px-2 text-[11px] font-medium shadow-[0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(0_0_0/0.08)] hover:bg-subtle",
              // What already went off is quieter than what's to come.
              r.o.status !== null && "bg-subtle! text-muted-foreground shadow-none! hover:bg-fill!",
            )}
            style={{ top: chipTops[i] }}
            title={timedLabel(r)}
          >
            <TimedIcon t={r} className="size-3 shrink-0" />
            <span className={cn("truncate", notHappened(r) && "line-through")}>{r.item.title}</span>
          </button>
        </CalendarItemMenu>
      ))}
      {today && (
        <div
          className="pointer-events-none absolute inset-x-0 z-[2] h-0.5 bg-[#ff3b30]"
          style={{ top: ((now - day) / 3_600_000) * HOUR }}
          aria-hidden
        >
          <span className="absolute -top-[3px] -left-1 size-2 rounded-full bg-[#ff3b30]" />
        </div>
      )}
    </div>
  );
}

function Agenda({
  from,
  to,
  events,
  reminders,
  items,
  loading,
  colorOf,
  onOpen,
  onReminder,
}: {
  from: number;
  to: number;
  events: CalendarEvent[];
  reminders: Timed[];
  items: ScheduleItem[];
  loading: boolean;
  colorOf: (e: CalendarEvent) => string;
  onOpen: (e: CalendarEvent) => void;
  onReminder: (i: ScheduleItem) => void;
}) {
  const onScroll = useScrollEdge();
  const days: number[] = [];
  for (let d = startOfDay(from); d < to; d = addDays(d, 1)) days.push(d);
  const groups = days
    .map((d) => ({
      day: d,
      events: events.filter((e) => onDay(e, d) && (e.all_day || e.start >= d || d === startOfDay(from))),
      reminders: reminders.filter((r) => r.o.at >= d && r.o.at < addDays(d, 1)),
    }))
    .filter((g) => g.events.length + g.reminders.length > 0);

  if (loading && events.length === 0) return <Skeleton className="h-full rounded-[18px]" />;
  return (
    <div className="h-full overflow-y-auto pr-1" onScroll={onScroll}>
      {groups.length === 0 ? (
        <div className="surface flex flex-col items-center gap-2 px-6 py-12 text-center">
          <p className="type-headline">Nothing planned</p>
          <p className="type-callout text-muted-foreground">
            No events in the next {Math.round((to - from) / DAY / 7)} weeks.
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-6">
          {groups.map((g) => (
            <section key={g.day} className="flex flex-col gap-2">
              <h2 className="section-label">{dayTitle(g.day)}</h2>
              <Grouped>
                {g.reminders.map((r) => {
                  const state = timedState(r);
                  return (
                    <CalendarItemMenu key={`${r.item.id}:${r.o.at}:${r.o.snoozed}`} item={r.item}>
                      <button
                        onClick={() => onReminder(r.item)}
                        className="flex min-h-[52px] items-center gap-3.5 px-4 py-2 text-left transition-colors hover:bg-[rgb(118_118_128/0.06)]"
                      >
                        <span className="w-24 shrink-0 type-subhead text-muted-foreground tabular-nums">{timedClock(r)}</span>
                        <TimedIcon t={r} className="size-4 shrink-0" />
                        <span className="min-w-0 flex-1">
                          <span
                            className={cn(
                              "block truncate type-body",
                              r.o.status !== null && "text-muted-foreground",
                              notHappened(r) && "line-through",
                            )}
                          >
                            {r.item.title}
                          </span>
                          {r.o.count > 1 && (
                            <span className="block truncate type-subhead text-muted-foreground">{r.item.description}</span>
                          )}
                        </span>
                        {state && <span className="shrink-0 type-footnote text-faint">{state}</span>}
                      </button>
                    </CalendarItemMenu>
                  );
                })}
                {g.events.map((e) => (
                  <EventMenu key={e.id} event={e} onOpen={onOpen}>
                    <button
                      onClick={() => onOpen(e)}
                      className="flex min-h-[56px] items-center gap-3.5 px-4 py-2 text-left transition-colors hover:bg-[rgb(118_118_128/0.06)]"
                    >
                      <span className="w-24 shrink-0 type-subhead text-muted-foreground tabular-nums">
                        {e.all_day ? "All day" : clock(e.start)}
                      </span>
                      <span className="h-8 w-1 shrink-0 rounded-full" style={{ background: colorOf(e) }} aria-hidden />
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-1.5 type-body font-medium">
                          <span className="truncate">{e.title}</span>
                          {remindersFor(items, e.id).length > 0 && (
                            <Bell className="size-3 shrink-0 text-muted-foreground" aria-label="Reminder set" />
                          )}
                        </span>
                        <span className="flex items-center gap-1 truncate type-subhead text-muted-foreground">
                          {timeRange(e)}
                          {e.location && (
                            <>
                              <span className="text-faint">·</span>
                              <MapPin className="size-3 shrink-0" />
                              <span className="truncate">{e.location}</span>
                            </>
                          )}
                        </span>
                      </span>
                      <span className="hidden truncate type-footnote text-faint sm:block">{e.calendar}</span>
                    </button>
                  </EventMenu>
                ))}
              </Grouped>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
