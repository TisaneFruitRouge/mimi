import { useState } from "react";
import { Check, CircleAlert, Loader2, X } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Action } from "@/bindings/Action";
import { describeArgs } from "@/features/chat/action-formatters";
import { api } from "@/lib/api";

/** What the assistant did (reads) and asked to do (approval cards) in one reply. */
export function Actions({ actions }: { actions: Action[] }) {
  const reads = actions.filter((a) => !a.requires_approval);
  const asks = actions.filter((a) => a.requires_approval);
  if (actions.length === 0) return null;
  return (
    <div className="flex flex-col gap-2.5">
      {reads.length > 0 && (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[11.5px]">
          {reads.map((a, i) => (
            <span key={a.id} className="inline-flex items-center gap-2">
              {i > 0 && <span className="text-faint">·</span>}
              <ReadStatus action={a} />
            </span>
          ))}
        </div>
      )}
      {asks.map((a) => (
        <ApprovalCard key={a.id} action={a} />
      ))}
    </div>
  );
}

function ReadStatus({ action: a }: { action: Action }) {
  if (a.status === "done")
    return (
      <span className="inline-flex items-center gap-1 text-private">
        <Check className="size-3" strokeWidth={2.5} />
        {a.result ?? a.summary}
      </span>
    );
  if (a.status === "failed")
    return (
      <span className="inline-flex items-center gap-1 text-destructive" title={a.error ?? undefined}>
        <X className="size-3" strokeWidth={2.5} />
        {a.summary}
      </span>
    );
  return <span className="shimmer">{lowerFirst(a.summary)}…</span>;
}

function ApprovalCard({ action: a }: { action: Action }) {
  const [busy, setBusy] = useState<null | "approve" | "reject">(null);
  const rows = describeArgs(a.tool, a.arguments as Record<string, unknown>);

  if (a.status !== "pending_approval") return <DecidedCard action={a} />;

  const decide = async (kind: "approve" | "reject") => {
    setBusy(kind);
    try {
      await (kind === "approve" ? api.approveAction(a.id) : api.rejectAction(a.id));
    } catch (e) {
      toast.error((e as Error).message);
      setBusy(null);
    }
  };

  return (
    <div className="overflow-hidden rounded-2xl border bg-background shadow-xs">
      <div className="flex flex-col gap-3 px-4 pt-3.5 pb-4">
        <div className="font-mono text-[11.5px] text-faint">
          <span className="font-medium tracking-[0.04em] text-[#4d6b00]">WAITING FOR YOU</span> ·{" "}
          {a.summary}
        </div>
        {rows.length > 0 && (
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[14px] leading-relaxed">
            {rows.map((r) => (
              <div key={r.label} className="contents">
                <dt className="text-muted-foreground">{r.label}</dt>
                <dd className="min-w-0 break-words whitespace-pre-wrap">{r.value}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
      <div className="flex gap-2 border-t bg-subtle/70 px-3 py-2.5">
        <button
          onClick={() => decide("approve")}
          disabled={busy !== null}
          className="flex h-8 items-center gap-1.5 rounded-[9px] bg-lime px-3.5 text-[13px] font-medium text-lime-ink shadow-[inset_0_0_0_1px_rgba(0,0,0,0.06)] transition hover:brightness-95 disabled:opacity-60"
        >
          {busy === "approve" && <Loader2 className="size-3.5 animate-spin" />}
          Approve
        </button>
        <button
          onClick={() => decide("reject")}
          disabled={busy !== null}
          className="h-8 rounded-[9px] px-3.5 text-[13px] font-medium text-muted-foreground transition hover:bg-secondary hover:text-foreground disabled:opacity-60"
        >
          Don’t
        </button>
      </div>
    </div>
  );
}

/** An approval card after the decision: one compact line. */
function DecidedCard({ action: a }: { action: Action }) {
  const tone = {
    approved: "text-muted-foreground",
    running: "text-muted-foreground",
    done: "text-private",
    rejected: "text-faint",
    failed: "text-destructive",
    pending_approval: "",
  }[a.status];
  const icon =
    a.status === "done" ? (
      <Check className="size-3.5" strokeWidth={2.5} />
    ) : a.status === "failed" ? (
      <CircleAlert className="size-3.5" />
    ) : a.status === "rejected" ? (
      <X className="size-3.5" />
    ) : (
      <Loader2 className="size-3.5 animate-spin" />
    );
  const text =
    a.status === "done"
      ? (a.result ?? a.summary)
      : a.status === "rejected"
        ? `You said no · ${lowerFirst(a.summary)}`
        : a.status === "failed"
          ? `${a.summary}: ${a.error ?? "it didn't work"}`
          : `Approved · ${lowerFirst(a.summary)}…`;
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-xl border border-dashed px-3.5 py-2.5 font-mono text-[12px]",
        tone,
      )}
    >
      <span className="mt-px">{icon}</span>
      <span className="min-w-0 break-words">{text}</span>
    </div>
  );
}

function lowerFirst(s: string) {
  return s.charAt(0).toLowerCase() + s.slice(1);
}
