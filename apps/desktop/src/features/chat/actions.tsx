import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Bell, BookmarkCheck, Check, CircleAlert, ExternalLink, Hand, Loader2, X } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Action } from "@/bindings/Action";
import { Button } from "@/components/ui/button";
import { describeArgs } from "@/features/chat/action-formatters";
import { api } from "@/lib/api";
import { openExternal } from "@/lib/transport";

/** Looking things up in memory is routine; it isn't shown. */
const MEMORY_READS = new Set(["memory_search", "memory_read", "memory_list"]);
/** Changes to memory show as one quiet line each, with Undo. */
const MEMORY_WRITES = new Set(["memory_write", "memory_update", "memory_forget"]);
/** Reminders and routines set, changed or cancelled: also one quiet line with Undo. */
const SCHEDULE_WRITES = new Set(["reminder_add", "routine_add", "schedule_change", "schedule_cancel"]);

/** What the assistant did (reads) and asked to do (approval cards) in one reply. */
export function Actions({ actions }: { actions: Action[] }) {
  // Only memory changes that happened; refused or empty ones aren't news.
  const remembered = actions.filter(
    (a) => MEMORY_WRITES.has(a.tool) && a.status === "done" && memoryRevision(a) !== null,
  );
  const scheduled = actions.filter(
    (a) => SCHEDULE_WRITES.has(a.tool) && a.status === "done" && scheduleRevision(a) !== null,
  );
  const reads = actions.filter(
    (a) =>
      !a.requires_approval &&
      !MEMORY_READS.has(a.tool) &&
      !MEMORY_WRITES.has(a.tool) &&
      !scheduled.includes(a),
  );
  const asks = actions.filter((a) => a.requires_approval);
  if (reads.length + remembered.length + scheduled.length + asks.length === 0) return null;
  return (
    <div className="flex flex-col gap-3">
      {reads.length > 0 && (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 type-subhead">
          {reads.map((a) => (
            <ReadStatus key={a.id} action={a} />
          ))}
        </div>
      )}
      {remembered.map((a) => (
        <UndoLine key={a.id} action={a} icon={<BookmarkCheck />} undo={() => api.undoMemory(memoryRevision(a) ?? 0)} />
      ))}
      {scheduled.map((a) => (
        <UndoLine key={a.id} action={a} icon={<Bell />} undo={() => api.undoSchedule(scheduleRevision(a) ?? 0)} />
      ))}
      {asks.map((a) => (
        <ApprovalCard key={a.id} action={a} />
      ))}
    </div>
  );
}

function memoryRevision(a: Action): number | null {
  const r = (a.output as { revision?: unknown } | null)?.revision;
  return typeof r === "number" ? r : null;
}

function scheduleRevision(a: Action): number | null {
  const r = (a.output as { schedule_revision?: unknown } | null)?.schedule_revision;
  return typeof r === "number" ? r : null;
}

/** "Remembered that Sam is your brother · Undo", "Reminder set tomorrow at 9:00: … · Undo". */
function UndoLine({
  action: a,
  icon,
  undo: revert,
}: {
  action: Action;
  icon: React.ReactNode;
  undo: () => Promise<unknown>;
}) {
  const [busy, setBusy] = useState(false);
  const undone = (a.output as { undone?: unknown } | null)?.undone === true;
  const undo = async () => {
    setBusy(true);
    try {
      await revert();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-center gap-1.5 type-subhead text-faint [&>svg]:size-3.5 [&>svg]:shrink-0">
      {icon}
      <span className={cn(undone && "line-through")}>{upperFirst(a.result ?? a.summary)}</span>
      {undone ? (
        <span>· Undone</span>
      ) : (
        <button
          onClick={undo}
          disabled={busy}
          className="rounded px-1 font-medium text-muted-foreground underline-offset-2 hover:text-foreground hover:underline disabled:opacity-60"
        >
          Undo
        </button>
      )}
    </div>
  );
}

function ReadStatus({ action: a }: { action: Action }) {
  if (a.status === "done")
    return (
      <span className="inline-flex items-center gap-1.5 text-faint">
        <Check className="size-3.5 text-private" strokeWidth={2.4} />
        {upperFirst(a.result ?? a.summary)}
      </span>
    );
  if (a.status === "failed")
    return (
      <span className="inline-flex items-center gap-1.5 text-destructive" title={a.error ?? undefined}>
        <X className="size-3.5" />
        Couldn't {lowerFirst(a.summary)}
      </span>
    );
  return <span className="shimmer">{lowerFirst(a.summary)}…</span>;
}

/** Actions approved in this window, so a follow-up link opens here and nowhere else. */
const approvedHere = new Set<string>();
const openedHere = new Set<string>();

/** A link the user has to finish something at, e.g. saving an event in Google Calendar. */
function openUrlOf(a: Action): string | null {
  const url = (a.output as { open_url?: unknown } | null)?.open_url;
  return typeof url === "string" && url.startsWith("https://") ? url : null;
}

function ApprovalCard({ action: a }: { action: Action }) {
  const [busy, setBusy] = useState<null | "approve" | "reject">(null);
  const rows = describeArgs(a.tool, a.arguments as Record<string, unknown>);

  if (a.status !== "pending_approval") return <DecidedCard action={a} />;

  const decide = async (kind: "approve" | "reject") => {
    setBusy(kind);
    try {
      if (kind === "approve") approvedHere.add(a.id);
      await (kind === "approve" ? api.approveAction(a.id) : api.rejectAction(a.id));
    } catch (e) {
      toast.error((e as Error).message);
      setBusy(null);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ type: "spring", stiffness: 420, damping: 32 }}
      className="rounded-[20px] bg-background p-4 shadow-[var(--shadow-raised)]"
    >
      <div className="flex items-start gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-lime-soft text-lime-deep">
          <Hand className="size-[18px]" />
        </span>
        <div className="flex min-w-0 flex-col gap-0.5 pt-px">
          <span className="type-footnote font-medium text-lime-deep">Needs your OK</span>
          <span className="type-headline">{a.summary}</span>
        </div>
      </div>
      {rows.length > 0 && (
        <dl className="mt-3.5 grid grid-cols-[auto_1fr] gap-x-5 gap-y-2 rounded-[14px] bg-subtle px-4 py-3 type-callout">
          {rows.map((r) => (
            <div key={r.label} className="contents">
              <dt className="text-muted-foreground">{r.label}</dt>
              <dd className="min-w-0 break-words whitespace-pre-wrap">{r.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="mt-4 flex justify-end gap-2">
        <Button variant="secondary" onClick={() => decide("reject")} disabled={busy !== null}>
          Not now
        </Button>
        <Button variant="lime" onClick={() => decide("approve")} disabled={busy !== null} className="min-w-[96px]">
          {busy === "approve" ? <Loader2 className="animate-spin" /> : null}
          Approve
        </Button>
      </div>
    </motion.div>
  );
}

/** An approval card after the decision: one compact line. */
function DecidedCard({ action: a }: { action: Action }) {
  const finishAt = a.status === "done" ? openUrlOf(a) : null;
  useEffect(() => {
    if (finishAt && approvedHere.has(a.id) && !openedHere.has(a.id)) {
      openedHere.add(a.id);
      openExternal(finishAt);
    }
  }, [finishAt, a.id]);
  const tone = {
    approved: "bg-fill text-muted-foreground",
    running: "bg-fill text-muted-foreground",
    done: "bg-private-soft text-private",
    rejected: "bg-fill text-muted-foreground",
    failed: "bg-[#fdeeec] text-destructive",
    pending_approval: "",
  }[a.status];
  const icon =
    a.status === "done" ? (
      <Check className="size-3" strokeWidth={3} />
    ) : a.status === "failed" ? (
      <CircleAlert className="size-3" strokeWidth={2.6} />
    ) : a.status === "rejected" ? (
      <X className="size-3" strokeWidth={3} />
    ) : (
      <Loader2 className="size-3 animate-spin" strokeWidth={2.6} />
    );
  const text =
    a.status === "done"
      ? upperFirst(a.result ?? a.summary)
      : a.status === "rejected"
        ? `Not done: ${lowerFirst(a.summary)}`
        : a.status === "failed"
          ? `Couldn't do it${a.error ? `: ${a.error}` : "."}`
          : `On it: ${lowerFirst(a.summary)}…`;
  return (
    <div className="flex items-center gap-3 rounded-[14px] bg-background px-3.5 py-2.5 type-callout shadow-[var(--shadow-card)]">
      <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full", tone)}>{icon}</span>
      <span
        className={cn(
          "min-w-0 flex-1 break-words",
          a.status === "failed" ? "text-destructive" : a.status === "done" ? "" : "text-muted-foreground",
        )}
      >
        {text}
      </span>
      {finishAt && (
        <Button variant="lime" size="sm" onClick={() => openExternal(finishAt)} className="-my-1">
          <ExternalLink /> Save in Google Calendar
        </Button>
      )}
    </div>
  );
}

function upperFirst(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function lowerFirst(s: string) {
  return s.charAt(0).toLowerCase() + s.slice(1);
}
