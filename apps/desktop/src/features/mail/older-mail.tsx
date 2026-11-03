import { type ReactNode, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CircleAlert, History, Loader2 } from "lucide-react";

import type { MailThread } from "@/bindings/MailThread";
import { Button } from "@/components/ui/button";
import { api, keys } from "@/lib/api";

/**
 * Below the results of a search: mail older than the three months kept on this computer,
 * looked for on the mail server only when the user asks. A server search is slow (seconds,
 * sometimes more), so it never runs while typing; the results stay for the same words.
 * Conversations found open like any other. Key it by the search, so new words ask again.
 */
export function OlderMail({
  query,
  account,
  shown,
  renderThread,
}: {
  query: string;
  /** Only this account's server (all of them when null). */
  account: string | null;
  /** Conversations already in the list above, left out here. */
  shown: ReadonlySet<number>;
  renderThread: (t: MailThread) => ReactNode;
}) {
  const [asked, setAsked] = useState(false);
  const older = useQuery({
    queryKey: keys.mailOlder(query, account),
    queryFn: () => api.mailOlder(query, account),
    enabled: asked,
    staleTime: Infinity,
    retry: false,
  });

  if (!older.data && !older.isFetching && !older.isError) {
    return (
      <div className="mt-3 flex flex-col items-center gap-2 px-6 py-4 text-center type-subhead text-muted-foreground">
        <span>Only the last three months of mail are kept on this computer.</span>
        <Button variant="secondary" size="sm" onClick={() => setAsked(true)}>
          <History /> Search older mail
        </Button>
      </div>
    );
  }

  const found = (older.data?.threads ?? []).filter((t) => !shown.has(t.id));
  return (
    <section aria-label="Older mail on the server" className="mt-3 flex flex-col gap-px">
      <div className="flex items-center gap-2 px-3 pt-2 pb-1 shadow-[inset_0_0.5px_var(--separator)]">
        <span className="section-label !px-0">Older mail on the server</span>
      </div>
      {older.isFetching && (
        <div className="flex items-center gap-2 px-3 py-3 type-subhead text-muted-foreground">
          <Loader2 className="size-4 animate-spin text-faint" />
          Searching your mail server…
        </div>
      )}
      {older.isError && !older.isFetching && (
        <div className="flex flex-col items-start gap-2 px-3 py-3 type-subhead text-destructive">
          <span>{(older.error as Error).message}</span>
          <Button variant="secondary" size="sm" onClick={() => older.refetch()}>
            Try again
          </Button>
        </div>
      )}
      {older.data && !older.isFetching && (
        <>
          {found.length === 0 && (
            <p className="px-3 py-3 type-subhead text-muted-foreground">
              {older.data.threads.length === 0
                ? `No older mail matches “${query.trim()}”.`
                : "Nothing older than what's above."}
            </p>
          )}
          {found.map(renderThread)}
          {older.data.more && (
            <p className="px-3 pt-2 type-footnote text-faint">
              Showing the newest matches only. Add a word to find something more precise.
            </p>
          )}
          {older.data.problems.map((p) => (
            <p key={p} className="flex gap-1.5 px-3 pt-2 type-footnote text-muted-foreground">
              <CircleAlert className="mt-px size-3.5 shrink-0 text-faint" />
              <span>{p}</span>
            </p>
          ))}
        </>
      )}
    </section>
  );
}
