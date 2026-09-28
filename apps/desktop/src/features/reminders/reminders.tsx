import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Bell,
  BellRing,
  CalendarDays,
  Check,
  ChevronRight,
  CircleAlert,
  Clock,
  MessageSquare,
  Monitor,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  Plus,
  Send,
  Sparkles,
  Trash2,
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
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { ScheduleDialog } from "@/features/reminders/schedule-dialog";
import { clock, when } from "@/features/reminders/time";
import { api, keys } from "@/lib/api";
import { useSettings } from "@/lib/queries";

const fail = (e: Error) => toast.error(e.message);

export const useScheduleItems = () => useQuery({ queryKey: keys.schedule, queryFn: api.schedule });

/** Won't happen again: a one-time reminder that went off, or one whose event was cancelled. */
export const finished = (i: ScheduleItem) => i.ended !== null || (i.next_at === null && !i.paused);

/**
 * Reminders and routines, beside the calendar: what's coming, with edit, pause, run now
 * and delete. `editing` lets the calendar open the same dialog from its grid.
 */
export function RemindersPanel({
  onOpenConversation,
  editing,
  onEdit,
}: {
  onOpenConversation: (id: string) => void;
  editing: ScheduleItem | "new" | null;
  onEdit: (item: ScheduleItem | "new" | null) => void;
}) {
  const items = useScheduleItems();
  const [deleting, setDeleting] = useState<ScheduleItem | null>(null);
  const [showFinished, setShowFinished] = useState(false);
  const list = items.data ?? [];
  const upcoming = list.filter((i) => !finished(i));
  const done = list.filter(finished);

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex min-h-7 items-end justify-between gap-4">
        <h2 className="section-label">Reminders and routines</h2>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="New reminder or routine"
          className="rounded-full text-muted-foreground"
          onClick={() => onEdit("new")}
        >
          <Plus />
        </Button>
      </div>
      {items.isLoading ? (
        <Grouped>
          {[0, 1].map((i) => (
            <div key={i} className="flex min-h-[60px] items-center gap-3.5 px-4">
              <Skeleton className="size-9 rounded-[10px]" />
              <Skeleton className="h-4 w-32" />
            </div>
          ))}
        </Grouped>
      ) : upcoming.length === 0 ? (
        <div className="surface flex flex-col items-start gap-2 px-4 py-4">
          <p className="type-callout font-medium">Nothing planned</p>
          <p className="type-subhead text-muted-foreground">
            Ask in a chat, like “remind me tomorrow at 9 to call Léa”, or add one here.
          </p>
          <Button size="sm" variant="secondary" className="mt-1" onClick={() => onEdit("new")}>
            Add one
          </Button>
        </div>
      ) : (
        <Grouped>
          {upcoming.map((i) => (
            <ItemRow
              key={i.id}
              item={i}
              onEdit={() => onEdit(i)}
              onDelete={() => setDeleting(i)}
              onOpenConversation={onOpenConversation}
            />
          ))}
        </Grouped>
      )}
      {done.length > 0 && (
        <>
          <button
            onClick={() => setShowFinished((s) => !s)}
            aria-expanded={showFinished}
            className="flex items-center gap-1 self-start rounded-md px-1 type-subhead text-muted-foreground hover:text-foreground"
          >
            <ChevronRight className={cn("size-3.5 transition-transform", showFinished && "rotate-90")} />
            Finished ({done.length})
          </button>
          {showFinished && (
            <Grouped>
              {done.map((i) => (
                <ItemRow
                  key={i.id}
                  item={i}
                  onEdit={() => onEdit(i)}
                  onDelete={() => setDeleting(i)}
                  onOpenConversation={onOpenConversation}
                />
              ))}
            </Grouped>
          )}
        </>
      )}

      <ScheduleDialog
        item={editing === "new" ? null : editing}
        open={editing !== null}
        onClose={() => onEdit(null)}
      />
      <DeleteItemDialog item={deleting} onClose={() => setDeleting(null)} />
    </div>
  );
}

export function DeleteItemDialog({ item, onClose }: { item: ScheduleItem | null; onClose: () => void }) {
  return (
    <AlertDialog open={!!item} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete “{item?.title}”?</AlertDialogTitle>
          <AlertDialogDescription>
            {item?.kind === "routine"
              ? "It won't run again. Its past results stay in their conversation."
              : "You won't be reminded of it again."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            onClick={() => item && api.deleteSchedule(item.id).catch(fail)}
          >
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Settings › Reminders and notifications: where they reach the user, and what happened lately. */
export function NotificationsSettings({
  onOpenConversation,
  onCalendar,
}: {
  onOpenConversation: (id: string) => void;
  onCalendar: () => void;
}) {
  const deliveries = useQuery({ queryKey: keys.deliveries, queryFn: api.deliveries }).data ?? [];
  return (
    <Page>
      <PageHeader
        title="Reminders and notifications"
        subtitle="How reminders and routine results reach you, and what went off lately."
      />
      <WhereTheyGo />

      <Section title="Your reminders and routines">
        <Grouped>
          <Row
            onClick={onCalendar}
            icon={
              <IconTile className="bg-event-soft text-event">
                <CalendarDays />
              </IconTile>
            }
            title="In Calendar"
            detail="See, change, pause or run them next to your events"
            trailing={<ChevronRight className="size-4 text-faint" />}
          />
        </Grouped>
      </Section>

      {deliveries.length > 0 && (
        <Section title="Recently">
          <Grouped>
            {deliveries.slice(0, 20).map((d) => (
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
    </Page>
  );
}

/**
 * Right-click on a reminder or routine (in the side list or the calendar): the same
 * choices as its "…" menu.
 */
export function ItemMenu({
  item: i,
  onEdit,
  onDelete,
  onOpenConversation,
  children,
}: {
  item: ScheduleItem;
  onEdit: () => void;
  onDelete: () => void;
  onOpenConversation: (id: string) => void;
  children: React.ReactElement;
}) {
  const routine = i.kind === "routine";
  const over = finished(i);
  const run = (fn: () => Promise<unknown>) => () => void fn().catch(fail);
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-[200px]">
        {!over && (
          <ContextMenuItem onSelect={onEdit}>
            <Pencil /> Edit…
          </ContextMenuItem>
        )}
        {routine && !over && (
          <ContextMenuItem
            onSelect={run(() => api.runRoutine(i.id).then(() => toast.success(`${i.title} is running`)))}
          >
            <Play /> Run now
          </ContextMenuItem>
        )}
        {routine && i.conversation_id && (
          <ContextMenuItem onSelect={() => onOpenConversation(i.conversation_id!)}>
            <MessageSquare /> Show results
          </ContextMenuItem>
        )}
        {!over && (
          <ContextMenuItem onSelect={run(() => api.updateSchedule(i.id, { paused: !i.paused }))}>
            {i.paused ? <Play /> : <Pause />} {i.paused ? "Resume" : "Pause"}
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem variant="destructive" onSelect={onDelete}>
          <Trash2 /> Delete…
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function ItemRow({
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
    <ItemMenu item={i} onEdit={onEdit} onDelete={onDelete} onOpenConversation={onOpenConversation}>
      <div>
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
      </div>
    </ItemMenu>
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
          detail={on ? "Shown even when the app is closed" : "Off. They still appear in the app."}
          trailing={
            <Switch
              checked={on}
              disabled={busy || !settings}
              onCheckedChange={toggle}
              aria-label="Notifications on this computer"
            />
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
