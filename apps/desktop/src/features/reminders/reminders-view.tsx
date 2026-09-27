import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Bell,
  BellRing,
  Check,
  CircleAlert,
  Clock,
  Monitor,
  MoreHorizontal,
  Plus,
  Send,
  Sparkles,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Delivery } from "@/bindings/Delivery";
import type { ScheduleItem } from "@/bindings/ScheduleItem";
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
import { Grouped, IconTile, Page, PageHeader, Pill, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { ScheduleDialog } from "@/features/reminders/schedule-dialog";
import { clock, when } from "@/features/reminders/time";
import { api, keys } from "@/lib/api";
import { useSettings } from "@/lib/queries";

const fail = (e: Error) => toast.error(e.message);

/** Reminders the user set and routines the assistant runs for them, with what happened lately. */
export function RemindersView({ onOpenConversation }: { onOpenConversation: (id: string) => void }) {
  const items = useQuery({ queryKey: keys.schedule, queryFn: api.schedule });
  const deliveries = useQuery({ queryKey: keys.deliveries, queryFn: api.deliveries }).data ?? [];
  const [editing, setEditing] = useState<ScheduleItem | "new" | null>(null);
  const [deleting, setDeleting] = useState<ScheduleItem | null>(null);

  const list = items.data ?? [];
  const upcoming = list.filter((i) => !finished(i));
  const done = list.filter(finished);

  return (
    <Page>
      <PageHeader
        title="Reminders"
        subtitle="Things to remind you of, and routines your assistant runs for you. You can also just ask in a chat."
        action={
          <Button size="sm" onClick={() => setEditing("new")}>
            <Plus /> New
          </Button>
        }
      />

      <Section title="Coming up">
        {items.isLoading ? (
          <Grouped>
            {[0, 1].map((i) => (
              <div key={i} className="flex min-h-[60px] items-center gap-3.5 px-4">
                <Skeleton className="size-9 rounded-[10px]" />
                <Skeleton className="h-4 w-48" />
              </div>
            ))}
          </Grouped>
        ) : upcoming.length === 0 ? (
          <div className="surface flex flex-col items-center gap-3 px-6 py-10 text-center">
            <IconTile size="lg" className="bg-fill text-muted-foreground">
              <Bell />
            </IconTile>
            <p className="type-headline">Nothing planned</p>
            <p className="max-w-sm type-callout text-muted-foreground">
              Try asking “remind me tomorrow at 9 to call Léa”, or “every morning at 7, send me my
              day”.
            </p>
            <Button size="sm" className="mt-1" onClick={() => setEditing("new")}>
              Add one
            </Button>
          </div>
        ) : (
          <Grouped>
            {upcoming.map((i) => (
              <ItemRow
                key={i.id}
                item={i}
                onEdit={() => setEditing(i)}
                onDelete={() => setDeleting(i)}
                onOpenConversation={onOpenConversation}
              />
            ))}
          </Grouped>
        )}
      </Section>

      {done.length > 0 && (
        <Section title="Finished">
          <Grouped>
            {done.map((i) => (
              <ItemRow
                key={i.id}
                item={i}
                onEdit={() => setEditing(i)}
                onDelete={() => setDeleting(i)}
                onOpenConversation={onOpenConversation}
              />
            ))}
          </Grouped>
        </Section>
      )}

      {deliveries.length > 0 && (
        <Section title="Recently">
          <Grouped>
            {deliveries.slice(0, 12).map((d) => (
              <DeliveryRow
                key={d.id}
                delivery={d}
                latest={deliveries.find((x) => x.item_id === d.item_id) === d}
                onOpenConversation={onOpenConversation}
              />
            ))}
          </Grouped>
        </Section>
      )}

      <WhereTheyGo />

      <ScheduleDialog
        item={editing === "new" ? null : editing}
        open={editing !== null}
        onClose={() => setEditing(null)}
      />
      <AlertDialog open={!!deleting} onOpenChange={(o) => !o && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{deleting?.title}”?</AlertDialogTitle>
            <AlertDialogDescription>
              {deleting?.kind === "routine"
                ? "It won't run again. Its past results stay in their conversation."
                : "You won't be reminded of it again."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => deleting && api.deleteSchedule(deleting.id).catch(fail)}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

/** Won't happen again: a one-time reminder that went off, or one whose event was cancelled. */
const finished = (i: ScheduleItem) => i.ended !== null || (i.next_at === null && !i.paused);

function ItemRow({
  item: i,
  onEdit,
  onDelete,
  onOpenConversation,
}: {
  item: ScheduleItem;
  onEdit: () => void;
  onDelete: () => void;
  onOpenConversation: (id: string) => void;
}) {
  const routine = i.kind === "routine";
  const over = finished(i);
  const repeats = i.schedule.type !== "once";
  let detail: string;
  if (i.ended) detail = i.ended;
  else if (over) detail = i.last_at ? `Went off ${when(i.last_at).toLowerCase()}` : "Done";
  else if (i.paused) detail = i.description;
  else if (repeats) detail = `${when(i.next_at!)} · ${i.description}`;
  else detail = when(i.next_at!);

  const run = (fn: () => Promise<unknown>) => () => void fn().catch(fail);
  return (
    <Row
      onClick={over ? undefined : onEdit}
      className={cn(over && "opacity-70")}
      icon={
        <IconTile className={routine ? "bg-lime-soft text-lime-deep" : "bg-fill text-foreground"}>
          {routine ? <Sparkles /> : <Bell />}
        </IconTile>
      }
      title={
        <span className="inline-flex max-w-full items-center gap-2">
          <span className="truncate">{i.title}</span>
          {i.paused && <Pill>Paused</Pill>}
        </span>
      }
      detail={detail}
      trailing={
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Options for ${i.title}`}
              className="rounded-full text-muted-foreground"
              onClick={(e) => e.stopPropagation()}
            >
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
            {routine && !over && (
              <DropdownMenuItem
                onSelect={run(() =>
                  api.runRoutine(i.id).then(() => toast.success(`${i.title} is running`)),
                )}
              >
                Run now
              </DropdownMenuItem>
            )}
            {routine && i.conversation_id && (
              <DropdownMenuItem onSelect={() => onOpenConversation(i.conversation_id!)}>
                Show results
              </DropdownMenuItem>
            )}
            {!over && <DropdownMenuItem onSelect={onEdit}>Edit</DropdownMenuItem>}
            {!over && (
              <DropdownMenuItem onSelect={run(() => api.updateSchedule(i.id, { paused: !i.paused }))}>
                {i.paused ? "Resume" : "Pause"}
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onSelect={onDelete}>
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    />
  );
}

const statusText: Record<Delivery["status"], string> = {
  delivered: "Sent",
  late: "Sent late",
  missed: "Missed",
  done: "Done",
  snoozed: "Snoozed",
  running: "Running…",
  failed: "Didn't finish",
  skipped: "Skipped",
};

function DeliveryRow({
  delivery: d,
  latest,
  onOpenConversation,
}: {
  delivery: Delivery;
  /** The newest delivery of its reminder: the only one worth acting on. */
  latest: boolean;
  onOpenConversation: (id: string) => void;
}) {
  const open = d.conversation_id && d.status !== "running" ? d.conversation_id : null;
  const pending =
    latest && d.kind === "reminder" && (d.status === "delivered" || d.status === "late");
  const bad = d.status === "missed" || d.status === "failed" || d.status === "skipped";
  const icon =
    d.status === "done" ? (
      <Check className="text-private" />
    ) : bad ? (
      <CircleAlert className="text-muted-foreground" />
    ) : d.status === "snoozed" ? (
      <Clock className="text-muted-foreground" />
    ) : (
      <BellRing className="text-muted-foreground" />
    );
  const late = d.status === "late" ? ` (due ${clock(d.due_at)})` : "";
  return (
    <Row
      onClick={open ? () => onOpenConversation(open) : undefined}
      icon={<IconTile size="sm" className="bg-fill">{icon}</IconTile>}
      title={d.title}
      detail={
        <>
          {when(d.at)}
          {late} · {statusText[d.status]}
          {d.detail && bad ? ` · ${d.detail}` : ""}
        </>
      }
      trailing={
        pending ? (
          <>
            <Button size="sm" variant="ghost" onClick={() => api.snoozeReminder(d.id, 10).catch(fail)}>
              In 10 min
            </Button>
            <Button size="sm" variant="secondary" onClick={() => api.reminderDone(d.id).catch(fail)}>
              Done
            </Button>
          </>
        ) : open ? (
          <span className="type-subhead text-faint">Open</span>
        ) : undefined
      }
    />
  );
}

/** Where reminders and routine results show up, and the switch for desktop notifications. */
function WhereTheyGo() {
  const settings = useSettings().data;
  const connections = useQuery({ queryKey: keys.connections, queryFn: api.connections }).data ?? [];
  const telegram = connections.find((c) => c.integration === "telegram" && c.status === "ok");
  const [busy, setBusy] = useState(false);
  const on = settings?.desktop_notifications ?? true;
  const toggle = async () => {
    if (!settings) return;
    setBusy(true);
    try {
      await api.putSettings({ ...settings, desktop_notifications: !on });
    } catch (e) {
      fail(e as Error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section title="Where they reach you">
      <Grouped>
        <Row
          icon={
            <IconTile className="bg-fill text-foreground">
              <Monitor />
            </IconTile>
          }
          title="Notifications on this computer"
          detail={on ? "Shown even when the app is closed" : "Off. They still appear here and in the app."}
          trailing={
            <button
              role="switch"
              aria-checked={on}
              aria-label="Notifications on this computer"
              disabled={busy || !settings}
              onClick={toggle}
              className={cn(
                "relative h-7 w-12 shrink-0 rounded-full transition-colors disabled:opacity-60",
                on ? "bg-private" : "bg-input",
              )}
            >
              <span
                className={cn(
                  "absolute top-1 left-1 size-5 rounded-full bg-white shadow-sm transition-transform",
                  on && "translate-x-5",
                )}
              />
            </button>
          }
        />
        <Row
          icon={
            <IconTile className="bg-network-soft text-network">
              <Send />
            </IconTile>
          }
          title="Telegram"
          detail={
            telegram
              ? "Sent to your bot, with Done and Snooze buttons. Routines ask for your OK there too."
              : "Connect a Telegram bot to get them on your phone."
          }
          trailing={telegram ? <Pill>On</Pill> : undefined}
        />
      </Grouped>
    </Section>
  );
}
