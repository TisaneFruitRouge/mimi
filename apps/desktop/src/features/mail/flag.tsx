import { useCallback } from "react";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { Star } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailThread } from "@/bindings/MailThread";
import type { MailThreadDetail } from "@/bindings/MailThreadDetail";
import { api, keys } from "@/lib/api";

/**
 * Flagging (starring) conversations. The star changes at once; the daemon tells the mail
 * server and, if the server doesn't take it, undoes its own copy and fails, so the star
 * goes back and a message says why.
 */

const FLAG_COLOR = "#ff9f0a";

/** Every list of conversations in the cache (`keys.mailThreads`). */
const LISTS = ["mail", "threads"] as const;

function setCached(qc: QueryClient, id: number, flagged: boolean) {
  qc.setQueriesData<MailThread[]>({ queryKey: LISTS }, (old) =>
    old?.map((t) => (t.id === id ? { ...t, flagged } : t)),
  );
  qc.setQueryData<MailThreadDetail>(keys.mailThread(id), (old) =>
    old ? { ...old, thread: { ...old.thread, flagged } } : old,
  );
}

/** Whether a conversation shows as flagged, from whatever has it (`null`: none). */
function cachedFlag(qc: QueryClient, id: number): boolean | null {
  const detail = qc.getQueryData<MailThreadDetail>(keys.mailThread(id));
  if (detail) return detail.thread.flagged;
  for (const [, list] of qc.getQueriesData<MailThread[]>({ queryKey: LISTS })) {
    const t = list?.find((x) => x.id === id);
    if (t) return t.flagged;
  }
  return null;
}

/**
 * Flags or unflags a conversation: `flag(id, true | false)`, or `flag(id)` to toggle it
 * (from what the panel shows). Optimistic; failures show a toast and put the star back.
 */
export function useFlagThread(): (id: number, flagged?: boolean) => Promise<void> {
  const qc = useQueryClient();
  return useCallback(
    async (id: number, flagged?: boolean) => {
      const next = flagged ?? !cachedFlag(qc, id);
      // A refresh already on its way could land with the old star.
      await qc.cancelQueries({ queryKey: LISTS });
      await qc.cancelQueries({ queryKey: keys.mailThread(id) });
      setCached(qc, id, next);
      try {
        await api.flagMail(id, next);
      } catch (e) {
        setCached(qc, id, !next);
        toast.error((e as Error).message);
      } finally {
        qc.invalidateQueries({ queryKey: keys.mail });
      }
    },
    [qc],
  );
}

/** The flag's star: filled when flagged. */
export function FlagStar({ flagged, className }: { flagged: boolean; className?: string }) {
  return (
    <Star
      className={cn(className)}
      style={flagged ? { color: FLAG_COLOR, fill: FLAG_COLOR } : undefined}
      aria-hidden
    />
  );
}

/**
 * The star in a conversation row's margin: shown when flagged, and on hover (or when the
 * row has focus) to flag it. Put it in a `group/row` element next to the row.
 */
export function RowFlag({ thread: t, className }: { thread: MailThread; className?: string }) {
  const flag = useFlagThread();
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={t.flagged ? "Remove flag" : "Flag"}
      aria-pressed={t.flagged}
      title={t.flagged ? "Remove flag" : "Flag"}
      onClick={(e) => {
        e.stopPropagation();
        void flag(t.id, !t.flagged);
      }}
      className={cn(
        "flex size-5 items-center justify-center rounded-full text-faint transition-opacity hover:bg-fill hover:text-foreground",
        t.flagged
          ? "opacity-100"
          : "opacity-0 group-focus-within/row:opacity-100 group-hover/row:opacity-100 focus-visible:opacity-100",
        className,
      )}
    >
      <FlagStar flagged={t.flagged} className="size-3.5" />
    </button>
  );
}
