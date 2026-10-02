import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarClock, ChevronDown, CircleAlert, Clock, Loader2, Paperclip, Send, Undo2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import type { OutgoingMail } from "@/bindings/OutgoingMail";
import { Grouped, IconTile, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { clock, when } from "@/features/reminders/time";
import { api, keys } from "@/lib/api";
import { useScrollEdge } from "@/lib/scroll-edge";
import { useSettings } from "@/lib/queries";

/*
 * Undo send and Send later. The waiting happens in the daemon's outbox (see the Email
 * doc), never here: closing the window doesn't lose or double-send anything. This file
 * shows it: the toast with Undo, the Send later menu, the Scheduled view, the setting.
 */

// --- Putting a draft back where it came from -------------------------------------------

type MailRequest = { compose: MailDraft } | { scheduled: string | null };
let pendingRequest: MailRequest | null = null;
const requestListeners = new Set<(r: MailRequest) => void>();

/** Opens a compose with a draft, or the Scheduled view, in the Mail panel (going there). */
export function requestMail(r: MailRequest) {
  if (requestListeners.size > 0) requestListeners.forEach((l) => l(r));
  else {
    pendingRequest = r;
    location.hash = "#/mail";
  }
}

/** The Mail panel's side of `requestMail`. Mount once, in the panel. */
export function useMailRequests(handle: (r: MailRequest) => void) {
  const ref = useRef(handle);
  ref.current = handle;
  useEffect(() => {
    const listener = (r: MailRequest) => ref.current(r);
    if (pendingRequest) {
      listener(pendingRequest);
      pendingRequest = null;
    }
    requestListeners.add(listener);
    return () => {
      requestListeners.delete(listener);
    };
  }, []);
}

// Editors that can take a draft back on Undo, by where it was sent from.
const homes = new Map<string, (d: MailDraft) => void>();

/**
 * Lets the editor a draft was sent from take it back on Undo while it's on screen
 * (`reply:<thread>`, `card:<action>`). Otherwise the draft opens in a new compose.
 */
export function useDraftHome(origin: string | null, restore: (d: MailDraft) => void) {
  const ref = useRef(restore);
  ref.current = restore;
  useEffect(() => {
    if (!origin) return;
    const fn = (d: MailDraft) => ref.current(d);
    homes.set(origin, fn);
    return () => {
      if (homes.get(origin) === fn) homes.delete(origin);
    };
  }, [origin]);
}

export function reopenDraft(origin: string, draft: MailDraft) {
  const home = homes.get(origin);
  if (home) home(draft);
  else requestMail({ compose: draft });
}

// --- Sending ---------------------------------------------------------------------------

// The waiting sends this window started, so only it shows their "Sent".
const mine = new Set<string>();

const subjectOf = (d: MailDraft) => d.subject.trim() || "(no subject)";
/** `when` inside a sentence: "tomorrow at 8:00", "on Friday at 7:00". */
const atTime = (ms: number) => {
  const w = when(ms);
  return /^(Today|Tomorrow|Yesterday)\b/.test(w) ? w[0].toLowerCase() + w.slice(1) : `on ${w}`;
};
const sentLabel = (d: MailDraft) => (d.to.length === 1 ? `Sent to ${d.to[0]}` : "Sent");

/** "Sending…" with Undo, for as long as the daemon waits. */
export function announceQueued(item: OutgoingMail, origin: string) {
  if (item.status === "sent") {
    toast.success(sentLabel(item.draft));
    return;
  }
  if (item.kind === "scheduled") {
    toast.success(`Scheduled for ${when(item.send_at)}`, {
      id: item.id,
      description: "It's in Scheduled in Mail until then.",
      action: { label: "Undo", onClick: () => takeBack(item, origin) },
    });
    return;
  }
  mine.add(item.id);
  toast("Sending…", {
    id: item.id,
    duration: Math.max(0, item.send_at - Date.now()) + 1500,
    action: { label: "Undo", onClick: () => takeBack(item, origin) },
  });
}

function takeBack(item: OutgoingMail, origin: string) {
  api.cancelOutgoing(item.id).then(
    (draft) => {
      reopenDraft(origin, draft);
      toast("Not sent. It's back where you wrote it.", { id: item.id, action: undefined, duration: 4000 });
    },
    (e) => toast.error((e as Error).message, { id: item.id, action: undefined }),
  );
}

/** What the daemon says about a waiting email: sent, or not sent and why. */
export function announceOutbox(item: OutgoingMail) {
  if (item.status === "sent") {
    if (item.kind === "undo") {
      if (!mine.delete(item.id)) return;
      toast.success(sentLabel(item.draft), { id: item.id, action: undefined, duration: 4000 });
      return;
    }
    const late = item.sent_at !== null && item.sent_at - item.send_at > 2 * 60_000;
    toast.success(`Sent “${subjectOf(item.draft)}”`, {
      id: item.id,
      action: undefined,
      description: late
        ? `It was due ${atTime(item.send_at)}, when this computer was off or asleep.`
        : `Scheduled for ${when(item.send_at)}.`,
    });
  } else if (item.status === "failed") {
    mine.delete(item.id);
    toast.error(`“${subjectOf(item.draft)}” wasn't sent`, {
      id: item.id,
      description: item.error ?? undefined,
      duration: 30_000,
      action: { label: "Open", onClick: () => requestMail({ scheduled: item.id }) },
    });
  }
}

/** The times "Send later" offers, from now: tomorrow morning and afternoon, Monday morning. */
function presets(now = new Date()) {
  const at = (days: number, hour: number) => {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, hour);
    return d.getTime();
  };
  const toMonday = ((8 - now.getDay()) % 7) || 7;
  const list = [
    { label: "Tomorrow morning", at: at(1, 8) },
    { label: "Tomorrow afternoon", at: at(1, 13) },
  ];
  // On a Sunday, Monday morning is tomorrow morning.
  if (toMonday > 1) list.push({ label: "Monday morning", at: at(toMonday, 8) });
  return list;
}

const OFF_NOTE =
  "It goes at that time if this computer is on. If it's off or asleep then, it goes as soon as Mimi runs again.";

/**
 * Send, and beside it the Send later menu: a few times to pick, or any date and time.
 * Without `onSchedule`, just Send.
 */
export function SendSplitButton({
  sending,
  onClick,
  onSchedule,
  disabled,
}: {
  sending: boolean;
  onClick: () => void;
  onSchedule?: (at: number) => void;
  disabled?: boolean;
}) {
  const [picking, setPicking] = useState(false);
  const send = (
    <Button
      variant="lime"
      onClick={onClick}
      disabled={sending || disabled}
      className={cn("min-w-[92px]", onSchedule && "rounded-r-none")}
    >
      {sending ? <Loader2 className="animate-spin" /> : <Send />}
      Send
    </Button>
  );
  if (!onSchedule) return send;
  return (
    <div className="flex">
      {send}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="lime"
            size="icon"
            disabled={sending || disabled}
            aria-label="Send later"
            title="Send later"
            className="w-8 rounded-l-none shadow-[inset_0.5px_0_rgb(0_0_0/0.12)]"
          >
            <ChevronDown />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-[280px]">
          <DropdownMenuLabel>Send later</DropdownMenuLabel>
          {presets().map((p) => (
            <DropdownMenuItem key={p.label} onSelect={() => onSchedule(p.at)}>
              <Clock />
              <span className="flex-1">{p.label}</span>
              <span className="type-footnote text-faint tabular-nums">{clock(p.at)}</span>
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setPicking(true)}>
            <CalendarClock /> Pick a date and time…
          </DropdownMenuItem>
          <p className="px-2 pt-1.5 pb-1 type-footnote text-faint">{OFF_NOTE}</p>
        </DropdownMenuContent>
      </DropdownMenu>
      <PickTime
        open={picking}
        title="Send later"
        confirm="Schedule"
        onClose={() => setPicking(false)}
        onPick={(at) => {
          setPicking(false);
          onSchedule(at);
        }}
      />
    </div>
  );
}

const pad = (n: number) => String(n).padStart(2, "0");
const dateValue = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const timeValue = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** Any date and time, with what happens when the computer is off then. */
function PickTime({
  open,
  title,
  confirm,
  initial,
  onClose,
  onPick,
}: {
  open: boolean;
  title: string;
  confirm: string;
  initial?: number;
  onClose: () => void;
  onPick: (at: number) => void;
}) {
  const start = () => new Date(initial ?? presets()[0].at);
  const [date, setDate] = useState(() => dateValue(start()));
  const [time, setTime] = useState(() => timeValue(start()));
  useEffect(() => {
    if (!open) return;
    setDate(dateValue(start()));
    setTime(timeValue(start()));
    // Only when it opens.
  }, [open]);
  const at = new Date(`${date}T${time}`).getTime();
  const valid = Number.isFinite(at) && at > Date.now();
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{OFF_NOTE}</DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <Input type="date" value={date} onChange={(e) => setDate(e.target.value)} aria-label="Date" />
          <Input
            type="time"
            value={time}
            onChange={(e) => setTime(e.target.value)}
            aria-label="Time"
            className="w-[136px] shrink-0"
          />
        </div>
        <p className={cn("type-subhead", valid ? "text-muted-foreground" : "text-destructive")}>
          {valid ? `It goes ${atTime(at)}.` : "Pick a time in the future."}
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="lime" disabled={!valid} onClick={() => onPick(at)}>
            {confirm}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// --- The Scheduled view ----------------------------------------------------------------

export const useOutbox = () => useQuery({ queryKey: keys.mailOutbox, queryFn: api.mailOutbox });

/** What waits in Scheduled: everything but sends still in their Undo seconds. */
const shown = (list: OutgoingMail[] | undefined) =>
  (list ?? []).filter((i) => i.kind === "scheduled" || i.status === "failed" || i.error !== null);

/** The sidebar's "Scheduled" entry, with how many wait and a mark when one wasn't sent. */
export function ScheduledNavItem({ active, onClick }: { active: boolean; onClick: () => void }) {
  const items = shown(useOutbox().data);
  const failed = items.some((i) => i.status === "failed");
  return (
    <button
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-9 items-center gap-2.5 rounded-[8px] px-2.5 text-left type-callout transition-colors",
        active ? "bg-fill font-medium" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
      )}
    >
      <Clock className={cn("size-4 shrink-0", active ? "text-foreground" : "text-muted-foreground")} />
      <span className="min-w-0 flex-1 truncate">Scheduled</span>
      {failed && <CircleAlert className="size-3.5 text-destructive" aria-label="Not sent" />}
      {items.length > 0 && (
        <span className="type-footnote font-medium text-muted-foreground tabular-nums">{items.length}</span>
      )}
    </button>
  );
}

function statusLine(i: OutgoingMail) {
  if (i.status === "failed") return "Not sent";
  if (i.status === "sending") return "Sending…";
  if (i.error) return `Trying again ${atTime(i.send_at)}`;
  return when(i.send_at);
}

/** The middle column in the Scheduled view: what's waiting, problems first. */
export function ScheduledList({
  selected,
  onSelect,
}: {
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const outbox = useOutbox();
  const items = shown(outbox.data);
  return (
    <div className="flex h-full w-[360px] shrink-0 flex-col pt-[68px] shadow-[inset_-0.5px_0_var(--separator)] max-lg:w-[320px]">
      <div className="px-4 pb-3">
        <h2 className="type-title">Scheduled</h2>
        <p className="type-subhead text-muted-foreground">Emails waiting for the time you chose.</p>
      </div>
      <div role="listbox" aria-label="Scheduled emails" className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-4">
        {!outbox.isLoading && items.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-6 pt-16 text-center type-callout text-muted-foreground">
            Nothing scheduled. Use the arrow beside Send to send an email later.
          </div>
        )}
        {items.map((i) => (
          <button
            key={i.id}
            role="option"
            aria-selected={i.id === selected}
            onClick={() => onSelect(i.id)}
            className={cn(
              "flex flex-col gap-0.5 rounded-[10px] px-3 py-2.5 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/45",
              i.id === selected ? "bg-[rgb(118_118_128/0.14)]" : "hover:bg-[rgb(118_118_128/0.07)]",
            )}
          >
            <span className="flex items-baseline gap-2">
              <span className="min-w-0 flex-1 truncate type-callout font-medium">To {i.draft.to.join(", ")}</span>
              <span
                className={cn(
                  "shrink-0 type-footnote tabular-nums",
                  i.status === "failed" || i.error ? "text-destructive" : "text-faint",
                )}
              >
                {statusLine(i)}
              </span>
            </span>
            <span className="truncate type-subhead">{subjectOf(i.draft)}</span>
            <span className="line-clamp-1 type-subhead text-muted-foreground">
              {i.status === "failed" ? i.error : i.draft.body}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * One scheduled email, read-only, with what can be done about it: send now, change the
 * time, edit (it comes out of the schedule into a compose), cancel.
 */
export function ScheduledReader({
  id,
  onGone,
  onEdit,
}: {
  id: string | null;
  onGone: () => void;
  onEdit: (draft: MailDraft) => void;
}) {
  const onScroll = useScrollEdge();
  const qc = useQueryClient();
  const item = shown(useOutbox().data).find((i) => i.id === id) ?? null;
  const [busy, setBusy] = useState(false);
  const [picking, setPicking] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  if (!item) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-6 pt-[60px] text-center">
        <IconTile size="lg" className="bg-fill text-muted-foreground">
          <Clock />
        </IconTile>
        <p className="type-headline">Choose a scheduled email</p>
        <p className="max-w-sm type-callout text-muted-foreground">
          Send it now, change its time, edit it, or cancel it.
        </p>
      </div>
    );
  }
  const run = async (f: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await f();
      if (done) toast.success(done);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
      qc.invalidateQueries({ queryKey: keys.mailOutbox });
    }
  };
  // Out of the outbox, back into a compose.
  const takeOut = (edit: boolean) =>
    run(async () => {
      const draft = await api.cancelOutgoing(item.id);
      onGone();
      if (edit) onEdit(draft);
    }, edit ? undefined : "Cancelled. It won't be sent.");
  const d = item.draft;
  const failed = item.status === "failed";
  return (
    <div className="h-full overflow-y-auto" onScroll={onScroll}>
      <div className="mx-auto flex w-full max-w-[760px] flex-col gap-5 px-6 pt-[84px] pb-20">
        <header className="flex flex-col gap-3">
          <h1 className="type-title">{subjectOf(d)}</h1>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="lime"
              size="sm"
              disabled={busy || item.status === "sending"}
              onClick={() =>
                run(async () => {
                  await api.sendOutgoingNow(item.id);
                  onGone();
                }, sentLabel(d))
              }
            >
              {busy ? <Loader2 className="animate-spin" /> : <Send />} Send now
            </Button>
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => setPicking(true)}>
              <CalendarClock /> {failed ? "Send later" : "Change time"}
            </Button>
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => takeOut(true)}>
              <Undo2 /> Edit
            </Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => setCancelling(true)}>
              Cancel sending…
            </Button>
          </div>
        </header>

        {failed ? (
          <div role="alert" className="flex gap-3 rounded-[16px] bg-[#fdeeec] px-4 py-3">
            <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <p className="type-callout font-semibold text-destructive">This email wasn't sent</p>
              <p className="type-subhead text-foreground/80">{item.error}</p>
            </div>
          </div>
        ) : (
          <div className="flex gap-3 rounded-[16px] bg-subtle px-4 py-3">
            <Clock className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <p className="type-callout">
                {item.error ? `Trying again ${atTime(item.send_at)}` : `Goes ${atTime(item.send_at)}`}
                {item.by_assistant && ", as you approved"}
              </p>
              <p className="type-subhead text-muted-foreground">{item.error ?? OFF_NOTE}</p>
            </div>
          </div>
        )}

        <div className="flex flex-col gap-1 rounded-[18px] bg-background px-5 py-4 shadow-[var(--shadow-card)]">
          <Field label="From">{item.from}</Field>
          <Field label="To">{d.to.join(", ")}</Field>
          {d.cc.length > 0 && <Field label="Cc">{d.cc.join(", ")}</Field>}
          {d.bcc.length > 0 && <Field label="Bcc">{d.bcc.join(", ")}</Field>}
          <p className="mt-3 type-body leading-[1.5] whitespace-pre-wrap">{d.body}</p>
          {(d.attachments.length > 0 || d.forward_of !== null) && (
            <div className="mt-3 flex flex-wrap gap-1.5">
              {d.forward_of !== null && (
                <span className="inline-flex h-[26px] items-center gap-1.5 rounded-full bg-fill px-2.5 text-[12px] font-medium text-muted-foreground">
                  <Paperclip className="size-3" /> The original's attachments
                </span>
              )}
              {d.attachments.map((a, i) => (
                <span
                  key={i}
                  className="inline-flex h-[26px] max-w-[260px] items-center gap-1.5 rounded-full bg-fill px-2.5 text-[12px] font-medium text-muted-foreground"
                >
                  <Paperclip className="size-3 shrink-0" />
                  <span className="truncate">{a.name}</span>
                </span>
              ))}
            </div>
          )}
        </div>
      </div>

      <PickTime
        open={picking}
        title={failed ? "Send later" : "Change the time"}
        confirm="Schedule"
        initial={failed ? undefined : item.send_at}
        onClose={() => setPicking(false)}
        onPick={(at) => {
          setPicking(false);
          run(() => api.rescheduleOutgoing(item.id, at), `Scheduled for ${when(at)}`);
        }}
      />
      <AlertDialog open={cancelling} onOpenChange={setCancelling}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Cancel sending?</AlertDialogTitle>
            <AlertDialogDescription>
              “{subjectOf(d)}” won't be sent. You can keep it as a draft to finish later, or delete it.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Don't cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => takeOut(false)}>
              Delete it
            </AlertDialogAction>
            <AlertDialogAction onClick={() => takeOut(true)}>Keep as draft</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <p className="flex gap-2 type-subhead">
      <span className="w-12 shrink-0 text-faint">{label}</span>
      <span className="min-w-0 break-words">{children}</span>
    </p>
  );
}

// --- The setting -----------------------------------------------------------------------

const waits = [0, 5, 10, 20, 30];

/** Settings: how long Send waits so it can be undone. */
export function UndoSendSetting() {
  const settings = useSettings().data;
  if (!settings) return null;
  return (
    <Section title="Email">
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="bg-[#efe9fb] text-[#6146ad]">
              <Undo2 />
            </IconTile>
          }
          title="Undo send"
          detail="After you press Send, the email waits this long so you can take it back."
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <Select
              value={String(settings.undo_send_secs)}
              onValueChange={(v) =>
                api
                  .putSettings({ ...settings, undo_send_secs: Number(v) })
                  .catch((e) => toast.error((e as Error).message))
              }
            >
              <SelectTrigger className="w-[130px]" aria-label="Undo send">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {waits.map((w) => (
                  <SelectItem key={w} value={String(w)}>
                    {w === 0 ? "Off" : `${w} seconds`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          }
        />
      </Grouped>
    </Section>
  );
}
