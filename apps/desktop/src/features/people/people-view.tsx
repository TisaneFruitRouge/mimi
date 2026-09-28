import { useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw, Search, Users } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { DuplicateSuggestion } from "@/bindings/DuplicateSuggestion";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { ChannelIcons, PersonAvatar } from "@/components/people";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { AddPersonDialog } from "@/features/people/person-dialogs";
import { PersonPage } from "@/features/people/person-page";
import type { Section as Place } from "@/features/shell/top-bar";
import { api, keys } from "@/lib/api";
import { ThingMenu } from "@/components/app-context-menu";
import { type Draft, mentionDraft } from "@/lib/draft";

/** Everyone the assistant can mention and reach: a list, and the chosen person beside it. */
export function PeopleView({
  personId,
  onOpenPerson,
  onAsk,
  onOpenConversation,
  onSection,
}: {
  personId: string | null;
  onOpenPerson: (id: string | null) => void;
  onAsk: (draft: Draft) => void;
  onOpenConversation: (id: string) => void;
  onSection: (s: Place) => void;
}) {
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const people = useQuery({
    queryKey: keys.peopleList(query),
    queryFn: () => api.people(query),
    placeholderData: keepPreviousData,
  });
  const duplicates = useQuery({ queryKey: keys.duplicates, queryFn: api.duplicates }).data ?? [];
  const everyone = people.data ?? [];

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

  /** Selects someone and moves focus to their row, for arrow-key browsing. */
  const select = (id: string) => {
    onOpenPerson(id);
    requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-id="${id}"]`)?.focus());
  };
  const step = (by: number) => {
    if (everyone.length === 0) return;
    const i = everyone.findIndex((p) => p.id === personId);
    const next = i === -1 ? (by > 0 ? 0 : everyone.length - 1) : Math.min(everyone.length - 1, Math.max(0, i + by));
    select(everyone[next].id);
  };

  const noOne = !people.isLoading && everyone.length === 0 && !query;

  return (
    <div className="flex h-full">
      <aside className="flex h-full w-[300px] shrink-0 flex-col bg-[rgb(255_255_255/0.45)] pt-[68px] shadow-[inset_-0.5px_0_var(--separator)]">
        <div className="flex items-center justify-between gap-2 px-4 pb-3">
          <h1 className="type-title">People</h1>
          <div className="flex gap-0.5">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Refresh from your address books"
                  className="rounded-full text-muted-foreground"
                  onClick={refresh}
                  disabled={syncing}
                >
                  <RefreshCw className={syncing ? "animate-spin" : ""} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Refresh from your address books</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Add someone"
                  className="rounded-full text-muted-foreground"
                  onClick={() => setAdding(true)}
                >
                  <Plus />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Add someone</TooltipContent>
            </Tooltip>
          </div>
        </div>
        <div className="px-3 pb-2">
          <label className="flex h-9 items-center gap-2 rounded-[10px] bg-fill px-3 focus-within:bg-background focus-within:shadow-[0_0_0_3px_rgb(200_242_93/0.35)]">
            <Search className="size-4 shrink-0 text-faint" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if ((e.key === "ArrowDown" || e.key === "Enter") && everyone[0]) {
                  e.preventDefault();
                  select(everyone[0].id);
                }
              }}
              placeholder="Name, number or email"
              aria-label="Search people"
              className="min-w-0 flex-1 bg-transparent type-callout outline-none placeholder:text-[#a1a1a6]"
            />
          </label>
        </div>
        {duplicates.length > 0 && (
          <button
            onClick={() => onOpenPerson(null)}
            className={cn(
              "mx-3 mb-2 flex items-center gap-2 rounded-[10px] bg-cloud-soft px-3 py-2 text-left type-subhead text-cloud hover:brightness-[0.98]",
              !personId && "ring-1 ring-cloud/30",
            )}
          >
            <Users className="size-4 shrink-0" />
            {duplicates.length === 1 ? "1 possible duplicate" : `${duplicates.length} possible duplicates`}
          </button>
        )}
        <div
          ref={list}
          role="listbox"
          aria-label="People"
          className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-4"
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              step(e.key === "ArrowDown" ? 1 : -1);
            }
          }}
        >
          {people.isLoading &&
            [0, 1, 2, 3].map((i) => (
              <div key={i} className="flex h-12 items-center gap-3 px-2">
                <Skeleton className="size-8 rounded-full" />
                <Skeleton className="h-4 w-36" />
              </div>
            ))}
          {!people.isLoading && everyone.length === 0 && query && (
            <p className="px-3 py-2 type-callout text-muted-foreground">No one matches “{query}”.</p>
          )}
          {everyone.map((p) => {
            const active = p.id === personId;
            return (
              <ThingMenu
                key={p.id}
                onOpen={() => onOpenPerson(p.id)}
                onAsk={() => onAsk(mentionDraft("person", p.id, p.name))}
              >
                <button
                  data-id={p.id}
                  role="option"
                  aria-selected={active}
                  tabIndex={active || (!personId && p === everyone[0]) ? 0 : -1}
                  onClick={() => onOpenPerson(p.id)}
                  className={cn(
                    "flex min-h-12 items-center gap-3 rounded-[10px] px-2 py-1.5 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/45",
                    active ? "bg-[rgb(118_118_128/0.14)]" : "hover:bg-[rgb(118_118_128/0.07)]",
                  )}
                >
                  <PersonAvatar id={p.id} name={p.name} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate type-callout font-medium">{p.name}</span>
                    {p.nickname && <span className="block truncate type-footnote text-muted-foreground">{p.nickname}</span>}
                  </span>
                  <ChannelIcons channels={p.channels} />
                </button>
              </ThingMenu>
            );
          })}
        </div>
      </aside>

      <div className="h-full min-w-0 flex-1">
        {personId ? (
          <PersonPage
            key={personId}
            id={personId}
            onOpenPerson={onOpenPerson}
            onAsk={onAsk}
            onOpenConversation={onOpenConversation}
            onSection={onSection}
          />
        ) : (
          <Page className="max-w-[720px]">
            {noOne ? (
              <div className="surface mt-10 flex flex-col items-center gap-3 px-6 py-10 text-center">
                <IconTile size="lg" className="bg-fill text-muted-foreground">
                  <Users />
                </IconTile>
                <p className="type-headline">No one here yet</p>
                <p className="max-w-sm type-callout text-muted-foreground">
                  Connect an iCloud, Fastmail or Nextcloud account and its address book appears
                  here. You can also add people yourself.
                </p>
                <div className="mt-1 flex gap-2">
                  <Button variant="secondary" size="sm" onClick={() => onSection("connections")}>
                    Open Connections
                  </Button>
                  <Button size="sm" onClick={() => setAdding(true)}>
                    Add someone
                  </Button>
                </div>
              </div>
            ) : duplicates.length > 0 ? (
              <>
                <PageHeader
                  title="Possible duplicates"
                  subtitle="These share a name. If they're the same person, combine them so your assistant sees one person with every way to reach them."
                />
                <Duplicates pairs={duplicates} />
              </>
            ) : (
              <div className="mt-24 flex flex-col items-center gap-3 text-center">
                <IconTile size="lg" className="bg-fill text-muted-foreground">
                  <Users />
                </IconTile>
                <p className="type-headline">Choose someone</p>
                <p className="max-w-sm type-callout text-muted-foreground">
                  See how to reach them, what your assistant remembers about them, and what's
                  coming up with them.
                </p>
              </div>
            )}
          </Page>
        )}
      </div>

      <AddPersonDialog open={adding} onOpenChange={setAdding} onAdded={(id) => onOpenPerson(id)} />
    </div>
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
    <Section>
      <Grouped>
        {pairs.slice(0, 10).map(({ a, b }) => {
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
