import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Bell,
  BookOpen,
  Loader2,
  Mail,
  MessageSquare,
  MoreHorizontal,
  Pencil,
  Plus,
  Sparkles,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { Person } from "@/bindings/Person";
import { Grouped, IconTile, Row, Section } from "@/components/page";
import { PersonAvatar } from "@/components/people";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { addDays, startOfDay, timeRange } from "@/features/calendar/dates";
import { EventSheet } from "@/features/calendar/event-sheet";
import { HandleForm, HandleRow, sourcesLine } from "@/features/people/person-dialogs";
import { ScheduleDialog } from "@/features/reminders/schedule-dialog";
import { when } from "@/features/reminders/time";
import type { Section as Place } from "@/features/shell/top-bar";
import { DaemonError, api, keys } from "@/lib/api";
import { type Draft, mentionDraft } from "@/lib/draft";
import { useAssistantName } from "@/lib/queries";
import { useScrollEdge } from "@/lib/scroll-edge";

/** How far ahead "Coming up" looks. */
const AHEAD_DAYS = 60;

/**
 * One person: how to reach them, what the assistant remembers, what's coming up with
 * them, recent email and the chats where they were mentioned.
 */
export function PersonPage({
  id,
  onOpenPerson,
  onAsk,
  onOpenConversation,
  onSection,
}: {
  id: string;
  onOpenPerson: (id: string | null) => void;
  onAsk: (draft: Draft) => void;
  onOpenConversation: (id: string) => void;
  onSection: (s: Place) => void;
}) {
  const onScroll = useScrollEdge();
  const person = useQuery({ queryKey: keys.person(id), queryFn: () => api.person(id), retry: false });

  return (
    <div className="h-full overflow-y-auto" onScroll={onScroll}>
      <div className="mx-auto flex w-full max-w-[720px] flex-col gap-9 px-8 pt-[92px] pb-20">
        {person.data ? (
          <Details
            person={person.data}
            onOpenPerson={onOpenPerson}
            onAsk={onAsk}
            onOpenConversation={onOpenConversation}
            onSection={onSection}
          />
        ) : person.isError ? (
          <p className="type-body text-muted-foreground">This person isn't in your contacts anymore.</p>
        ) : (
          <div className="flex items-center gap-4">
            <Skeleton className="size-16 rounded-full" />
            <Skeleton className="h-7 w-48" />
          </div>
        )}
      </div>
    </div>
  );
}

function Details({
  person: p,
  onOpenPerson,
  onAsk,
  onOpenConversation,
  onSection,
}: {
  person: Person;
  onOpenPerson: (id: string | null) => void;
  onAsk: (draft: Draft) => void;
  onOpenConversation: (id: string) => void;
  onSection: (s: Place) => void;
}) {
  const assistant = useAssistantName();
  const [editing, setEditing] = useState(false);
  const [adding, setAdding] = useState(false);
  const [reminding, setReminding] = useState(false);
  const first = p.nickname || p.name.split(/\s+/)[0];

  return (
    <>
      <header className="flex items-center gap-5">
        <PersonAvatar id={p.id} name={p.name} size="lg" />
        <div className="min-w-0 flex-1">
          <h1 className="truncate type-large-title">{p.name}</h1>
          <p className="truncate type-body text-muted-foreground">
            {p.nickname ? `Also called ${p.nickname}` : sourcesLine(p)}
          </p>
        </div>
      </header>

      <div className="-mt-3 flex flex-wrap items-center gap-2">
        <Button onClick={() => onAsk(mentionDraft("person", p.id, p.name))}>
          <Sparkles /> Ask {assistant} about {first}
        </Button>
        <Button variant="secondary" onClick={() => setReminding(true)}>
          <Bell /> Remind me…
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" aria-label={`More for ${p.name}`} className="rounded-full text-muted-foreground">
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={() => setEditing(true)}>
              <Pencil /> Change name
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => setAdding(true)}>
              <Plus /> Add a way to reach them
            </DropdownMenuItem>
            {p.manual && p.sources.length === 0 && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() =>
                    api
                      .removePerson(p.id)
                      .then(() => onOpenPerson(null))
                      .catch((e) => toast.error(e.message))
                  }
                >
                  <Trash2 /> Remove {p.name}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {editing && <NameForm person={p} onDone={() => setEditing(false)} />}

      <Section title="How to reach them">
        {p.handles.length === 0 && !adding ? (
          <p className="type-callout text-muted-foreground">Nothing yet.</p>
        ) : (
          p.handles.length > 0 && (
            <Grouped>
              {p.handles.map((h) => (
                <HandleRow key={h.id} person={p} handle={h} />
              ))}
            </Grouped>
          )
        )}
        {adding ? (
          <HandleForm
            onCancel={() => setAdding(false)}
            onSubmit={async (h) => {
              await api.addHandle(p.id, h);
              setAdding(false);
            }}
          />
        ) : (
          <Button variant="secondary" size="sm" className="self-start" onClick={() => setAdding(true)}>
            <Plus /> Add a way to reach them
          </Button>
        )}
      </Section>

      <Remembered person={p} assistant={assistant} onOpenMemory={() => onSection("memory")} />
      <ComingUp person={p} onAsk={onAsk} onOpenPerson={onOpenPerson} />
      <RecentMail person={p} onOpenMail={() => onSection("mail")} />
      <Conversations person={p} assistant={assistant} onOpen={onOpenConversation} />

      {p.sources.length > 1 && (
        <Section title="Combined from">
          <Grouped>
            {p.sources.map((s) => (
              <div key={`${s.source_id}/${s.record}`} className="flex min-h-[48px] items-center gap-3 px-4 py-2 type-callout">
                <span className="min-w-0 flex-1 truncate">
                  {s.name} <span className="text-faint">· {s.source_name}</span>
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-muted-foreground"
                  onClick={() =>
                    api
                      .splitPerson(p.id, s.source_id, s.record)
                      .then((fresh) => {
                        toast.success(`${fresh.name} is now separate`);
                        onOpenPerson(fresh.id);
                      })
                      .catch((e) => toast.error(e.message))
                  }
                >
                  Not the same person
                </Button>
              </div>
            ))}
          </Grouped>
        </Section>
      )}

      <ScheduleDialog item={null} open={reminding} title={`Get back to ${first}`} onClose={() => setReminding(false)} />
    </>
  );
}

function NameForm({ person: p, onDone }: { person: Person; onDone: () => void }) {
  const [name, setName] = useState(p.name);
  const [nickname, setNickname] = useState(p.nickname ?? "");
  const save = async () => {
    try {
      await api.updatePerson(p.id, { name, nickname });
      onDone();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  return (
    <form
      className="surface flex flex-col gap-3 p-4"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="person-name">Name</Label>
          <Input id="person-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="person-nickname">Nickname</Label>
          <Input id="person-nickname" value={nickname} onChange={(e) => setNickname(e.target.value)} placeholder="Optional" />
        </div>
      </div>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" disabled={!name.trim()}>
          Save
        </Button>
      </div>
    </form>
  );
}

const notFound = (e: unknown) => e instanceof DaemonError && (e.code === "not_found" || e.kind !== "api");

/** Memory notes about this person. Older daemons don't know this question: say nothing's noted. */
function Remembered({
  person: p,
  assistant,
  onOpenMemory,
}: {
  person: Person;
  assistant: string;
  onOpenMemory: () => void;
}) {
  const notes = useQuery({
    queryKey: keys.personExtra(p.id, "memory"),
    queryFn: () => api.personMemory(p.id),
    retry: false,
  });
  const list = notes.data ?? [];
  return (
    <Section
      title={`What ${assistant} remembers`}
      action={
        <Button variant="ghost" size="sm" className="text-muted-foreground" onClick={onOpenMemory}>
          Open Memory
        </Button>
      }
    >
      {notes.isLoading ? (
        <Skeleton className="h-[60px] rounded-[16px]" />
      ) : notes.isError && !notFound(notes.error) ? (
        <p className="type-callout text-muted-foreground">Couldn't read memory just now.</p>
      ) : list.length === 0 ? (
        <p className="type-callout text-muted-foreground">
          Nothing noted yet. Tell {assistant} about {p.nickname || p.name.split(/\s+/)[0]} in a chat and it'll remember.
        </p>
      ) : (
        <Grouped>
          {list.map((n) => (
            <Row
              key={n.path}
              onClick={onOpenMemory}
              icon={
                <IconTile size="sm" className="bg-[#f3e8fd] text-[#8a3ec2]">
                  <BookOpen />
                </IconTile>
              }
              title={n.title}
              detail={firstLine(n.body)}
            />
          ))}
        </Grouped>
      )}
    </Section>
  );
}

const firstLine = (body: string) =>
  body
    .split("\n")
    .map((l) => l.replace(/^[-*#\s]+/, "").trim())
    .find(Boolean) ?? "";

function ComingUp({
  person: p,
  onAsk,
  onOpenPerson,
}: {
  person: Person;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string | null) => void;
}) {
  const [from] = useState(() => startOfDay(Date.now()));
  const to = addDays(from, AHEAD_DAYS);
  const events = useQuery({
    queryKey: keys.personExtra(p.id, "events"),
    queryFn: () => api.personEvents(p.id, from, to),
    staleTime: 60_000,
  });
  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars }).data ?? [];
  const color = (e: CalendarEvent) => calendars.find((c) => c.id === e.calendar_id)?.color ?? "#8e8e93";
  const [open, setOpen] = useState<CalendarEvent | null>(null);
  // A daily standup is one line, at its next time, not sixty.
  const seen = new Set<string>();
  const list = (events.data ?? []).filter((e) => {
    const key = `${e.calendar_id}\n${e.title}`;
    if (e.end <= Date.now() || seen.has(key)) return false;
    seen.add(key);
    return true;
  });

  return (
    <Section title="Coming up with them">
      {events.isLoading ? (
        <Skeleton className="h-[60px] rounded-[16px]" />
      ) : list.length === 0 ? (
        <p className="type-callout text-muted-foreground">
          {calendars.length === 0
            ? "Connect a calendar to see events with them."
            : "Nothing in the next two months."}
        </p>
      ) : (
        <Grouped>
          {list.slice(0, 8).map((e) => (
            <Row
              key={e.id}
              onClick={() => setOpen(e)}
              icon={<DateTile ms={e.start} color={color(e)} />}
              title={e.title}
              detail={`${when(e.start).replace(/ at .*/, "")} · ${timeRange(e)}${e.location ? ` · ${e.location}` : ""}`}
            />
          ))}
        </Grouped>
      )}
      <EventSheet
        event={open}
        color={open ? color(open) : "#8e8e93"}
        onClose={() => setOpen(null)}
        onAsk={onAsk}
        onOpenPerson={(id) => onOpenPerson(id)}
      />
    </Section>
  );
}

/** A small calendar-page tile: weekday and day number, in the calendar's colour. */
function DateTile({ ms, color }: { ms: number; color: string }) {
  const d = new Date(ms);
  return (
    <div
      className="flex size-9 shrink-0 flex-col items-center justify-center rounded-[10px] leading-none"
      style={{ background: `color-mix(in srgb, ${color} 14%, white)`, color: `color-mix(in srgb, ${color} 60%, #1d1d1f)` }}
    >
      <span className="text-[9px] font-semibold uppercase">{d.toLocaleDateString([], { weekday: "short" })}</span>
      <span className="text-[15px] font-semibold tabular-nums">{d.getDate()}</span>
    </div>
  );
}

/** Recent email with this person. Hidden until email is connected. */
function RecentMail({ person: p, onOpenMail }: { person: Person; onOpenMail: () => void }) {
  const hasEmail = p.handles.some((h) => h.channel === "email");
  const threads = useQuery({
    queryKey: keys.personExtra(p.id, "mail"),
    queryFn: () => api.personMail(p.id),
    enabled: hasEmail,
    retry: false,
  });
  if (!hasEmail || threads.isError || threads.isLoading) return null;
  const list = threads.data ?? [];
  return (
    <Section title="Recent emails">
      {list.length === 0 ? (
        <p className="type-callout text-muted-foreground">No recent emails with them.</p>
      ) : (
        <Grouped>
          {list.slice(0, 5).map((t) => (
            <Row
              key={t.id}
              onClick={onOpenMail}
              icon={
                <IconTile size="sm" className="bg-[#efe9fb] text-[#6146ad]">
                  <Mail />
                </IconTile>
              }
              title={
                <span className={t.unread ? "font-semibold" : undefined}>{t.subject || "(no subject)"}</span>
              }
              detail={t.summary || t.snippet}
              trailing={<span className="type-footnote text-faint">{shortDate(t.last_at)}</span>}
            />
          ))}
        </Grouped>
      )}
    </Section>
  );
}

function Conversations({
  person: p,
  assistant,
  onOpen,
}: {
  person: Person;
  assistant: string;
  onOpen: (id: string) => void;
}) {
  const convs = useQuery({
    queryKey: keys.personExtra(p.id, "conversations"),
    queryFn: () => api.personConversations(p.id),
    staleTime: 30_000,
  });
  const list = convs.data ?? [];
  return (
    <Section title="Chats about them">
      {convs.isLoading ? (
        <Loader2 className="size-4 animate-spin text-faint" />
      ) : list.length === 0 ? (
        <p className="type-callout text-muted-foreground">
          Mention them with @ in a chat with {assistant} and it shows here.
        </p>
      ) : (
        <Grouped>
          {list.slice(0, 8).map((c) => (
            <Row
              key={c.id}
              onClick={() => onOpen(c.id)}
              icon={
                <IconTile size="sm" className="bg-fill text-muted-foreground">
                  <MessageSquare />
                </IconTile>
              }
              title={c.title}
              trailing={<span className="type-footnote text-faint">{shortDate(c.updated_at)}</span>}
            />
          ))}
        </Grouped>
      )}
    </Section>
  );
}

function shortDate(ms: number) {
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleDateString([], { day: "numeric", month: "short", year: sameYear ? undefined : "numeric" });
}
