import { useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw, Search, Users } from "lucide-react";
import { toast } from "sonner";

import type { DuplicateSuggestion } from "@/bindings/DuplicateSuggestion";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { ChannelIcons, PersonAvatar } from "@/components/people";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { AddPersonDialog, PersonDialog } from "@/features/people/person-dialogs";
import { api, keys } from "@/lib/api";

/** Everyone the assistant can mention and reach. */
export function PeopleView({ onConnections }: { onConnections: () => void }) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const people = useQuery({
    queryKey: keys.peopleList(query),
    queryFn: () => api.people(query),
    placeholderData: keepPreviousData,
  });
  const duplicates = useQuery({ queryKey: keys.duplicates, queryFn: api.duplicates }).data ?? [];
  const list = people.data ?? [];

  const refresh = async () => {
    setSyncing(true);
    try {
      await api.syncPeople();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setSyncing(false);
    }
  };

  return (
    <Page>
      <PageHeader
        title="People"
        subtitle="Everyone you can mention with @. They come from your address books, or you add them yourself."
        action={
          <div className="flex shrink-0 gap-2">
            <Button variant="ghost" size="sm" onClick={refresh} disabled={syncing}>
              <RefreshCw className={syncing ? "animate-spin" : ""} /> Refresh
            </Button>
            <Button size="sm" onClick={() => setAdding(true)}>
              <Plus /> Add someone
            </Button>
          </div>
        }
      />

      {duplicates.length > 0 && <Duplicates pairs={duplicates} />}

      <Section>
        <label className="flex h-10 items-center gap-2 rounded-[10px] bg-fill px-3 focus-within:bg-background focus-within:shadow-[0_0_0_3px_rgb(200_242_93/0.35)]">
          <Search className="size-4 shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search by name, number or email"
            aria-label="Search people"
            className="min-w-0 flex-1 bg-transparent type-body outline-none placeholder:text-[#a1a1a6]"
          />
        </label>

        {people.isLoading ? (
          <Grouped>
            {[0, 1, 2].map((i) => (
              <div key={i} className="flex min-h-[60px] items-center gap-3.5 px-4">
                <Skeleton className="size-8 rounded-full" />
                <Skeleton className="h-4 w-40" />
              </div>
            ))}
          </Grouped>
        ) : list.length === 0 ? (
          query ? (
            <p className="px-1 type-callout text-muted-foreground">No one matches “{query}”.</p>
          ) : (
            <div className="surface flex flex-col items-center gap-3 px-6 py-10 text-center">
              <IconTile size="lg" className="bg-fill text-muted-foreground">
                <Users />
              </IconTile>
              <p className="type-headline">No one here yet</p>
              <p className="max-w-sm type-callout text-muted-foreground">
                Connect an iCloud, Fastmail or Nextcloud account and its address book appears
                here. You can also add people yourself.
              </p>
              <div className="mt-1 flex gap-2">
                <Button variant="secondary" size="sm" onClick={onConnections}>
                  Open Connections
                </Button>
                <Button size="sm" onClick={() => setAdding(true)}>
                  Add someone
                </Button>
              </div>
            </div>
          )
        ) : (
          <Grouped>
            {list.map((p) => (
              <Row
                key={p.id}
                onClick={() => setOpen(p.id)}
                icon={<PersonAvatar id={p.id} name={p.name} />}
                title={
                  <>
                    {p.name}
                    {p.nickname && <span className="font-normal text-faint"> · {p.nickname}</span>}
                  </>
                }
                trailing={<ChannelIcons channels={p.channels} />}
              />
            ))}
          </Grouped>
        )}
      </Section>

      <PersonDialog id={open} onClose={() => setOpen(null)} onSelect={setOpen} />
      <AddPersonDialog open={adding} onOpenChange={setAdding} onAdded={setOpen} />
    </Page>
  );
}

function Duplicates({ pairs }: { pairs: DuplicateSuggestion[] }) {
  const [busy, setBusy] = useState<string | null>(null);
  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(null);
    }
  };
  return (
    <Section title="Possible duplicates">
      <p className="-mt-1 px-1 type-subhead text-muted-foreground">
        These share a name. If they're the same person, combine them so the assistant sees one
        person with every way to reach them.
      </p>
      <Grouped>
        {pairs.slice(0, 5).map(({ a, b }) => {
          const key = `${a.id}-${b.id}`;
          return (
            <Row
              key={key}
              icon={
                <div className="flex -space-x-2">
                  <span className="rounded-full ring-2 ring-background">
                    <PersonAvatar id={a.id} name={a.name} size="sm" />
                  </span>
                  <span className="rounded-full ring-2 ring-background">
                    <PersonAvatar id={b.id} name={b.name} size="sm" />
                  </span>
                </div>
              }
              title={
                <>
                  {a.name} <span className="font-normal text-faint">and</span> {b.name}
                </>
              }
              detail={
                <span className="inline-flex items-center gap-2">
                  <ChannelIcons channels={a.channels} />
                  <span className="text-faint">·</span>
                  <ChannelIcons channels={b.channels} />
                </span>
              }
              trailing={
                <>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy === key}
                    onClick={() => run(key, () => api.dismissDuplicate(a.id, b.id))}
                  >
                    Not the same
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy === key}
                    onClick={() => run(key, () => api.mergePeople(a.id, b.id))}
                  >
                    Same person
                  </Button>
                </>
              }
            />
          );
        })}
      </Grouped>
    </Section>
  );
}
