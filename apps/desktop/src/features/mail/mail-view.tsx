import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import {
  Archive,
  AtSign,
  ChevronDown,
  CircleAlert,
  Inbox,
  Layers,
  Loader2,
  Mail,
  MailOpen,
  MoreHorizontal,
  Paperclip,
  RefreshCw,
  Reply,
  Search,
  Send,
  Sparkles,
  SquarePen,
  Star,
  X,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailBox } from "@/bindings/MailBox";
import type { MailDraft } from "@/bindings/MailDraft";
import type { MailMessage } from "@/bindings/MailMessage";
import type { MailOverview } from "@/bindings/MailOverview";
import type { MailThread } from "@/bindings/MailThread";
import type { MailThreadDetail } from "@/bindings/MailThreadDetail";
import { LocalityBadge } from "@/components/locality-badge";
import { IconTile, Pill } from "@/components/page";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ConnectDialog } from "@/features/connections/connect-dialogs";
import { DraftEditor, SendButton, useSendDraft } from "@/features/mail/draft-editor";
import type { Section } from "@/features/shell/top-bar";
import { api, keys, type MailScope } from "@/lib/api";
import { type Draft, mentionDraft } from "@/lib/draft";
import { useAssistantName, useConnections } from "@/lib/queries";
import { useScrollEdge } from "@/lib/scroll-edge";

// --- Views ---------------------------------------------------------------------------

const views: { id: MailBox; label: string; icon: typeof Inbox; sorted: boolean }[] = [
  { id: "needs_reply", label: "Needs a reply", icon: Reply, sorted: true },
  { id: "important", label: "Important", icon: Star, sorted: true },
  { id: "other", label: "Everything else", icon: Layers, sorted: true },
  { id: "inbox", label: "Inbox", icon: Inbox, sorted: false },
  { id: "sent", label: "Sent", icon: Send, sorted: false },
  { id: "archive", label: "Archive", icon: Archive, sorted: false },
];
const viewOf = (id: MailBox) => views.find((v) => v.id === id)!;

const empty: Record<MailBox, string> = {
  needs_reply: "Nothing is waiting for your answer.",
  important: "Nothing important right now.",
  other: "No newsletters or notifications.",
  inbox: "Your inbox is empty.",
  sent: "Nothing sent in the last 90 days.",
  archive: "Nothing archived in the last 90 days.",
};

// The last view, per viewer. Only a convenience: everything works without it.
const VIEW_KEY = "mimi.mail.view";
function storedView(): MailBox | null {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    return views.some((x) => x.id === v) ? (v as MailBox) : null;
  } catch {
    return null;
  }
}
function storeView(v: MailBox) {
  try {
    localStorage.setItem(VIEW_KEY, v);
  } catch {
    // Not important.
  }
}

// A conversation another panel asked to show (e.g. "Recent emails" on a person's page).
let pendingThread: number | null = null;
/** Opens a conversation the next time the Mail panel shows. */
export function showMailThread(id: number) {
  pendingThread = id;
}

// Which account or address the panel shows, per viewer (like the view).
const SCOPE_KEY = "mimi.mail.scope";
function storedScope(): MailScope {
  try {
    const v = JSON.parse(localStorage.getItem(SCOPE_KEY) ?? "{}");
    return {
      account: typeof v.account === "string" ? v.account : undefined,
      address: typeof v.address === "string" ? v.address : undefined,
    };
  } catch {
    return {};
  }
}
function storeScope(s: MailScope) {
  try {
    localStorage.setItem(SCOPE_KEY, JSON.stringify(s));
  } catch {
    // Not important.
  }
}
const sameScope = (a: MailScope, b: MailScope) => (a.account ?? null) === (b.account ?? null) && (a.address ?? null) === (b.address ?? null);

interface ScopeRow {
  key: string;
  label: string;
  scope: MailScope;
  unread: number | null;
  nested: boolean;
  account: boolean;
}

/**
 * "Received on" choices: each account when there are several, and each address mail
 * arrived at when an account has more than one (aliases, catch-all, +tags). Empty when
 * there's only one address in all: nothing to split.
 */
function scopeRows(o: MailOverview): ScopeRow[] {
  const rows: ScopeRow[] = [];
  const several = o.accounts.length > 1;
  for (const a of o.accounts) {
    if (several)
      rows.push({
        key: a.connection_id,
        label: a.email,
        scope: { account: a.connection_id },
        unread: null,
        nested: false,
        account: true,
      });
    if (a.addresses.length > 1)
      for (const x of a.addresses)
        rows.push({
          key: `${a.connection_id}:${x.email}`,
          label: x.email,
          scope: { address: x.email },
          unread: x.unread,
          nested: several,
          account: false,
        });
  }
  return rows;
}

/** Whether conversations should say which address they arrived at. */
const manyAddresses = (o: MailOverview) => o.accounts.reduce((n, a) => n + a.addresses.length, 0) > 1;

/** The accounts' own addresses: mail to these needs no tag in the list, only aliases do. */
const mainAddresses = (o: MailOverview) => new Set(o.accounts.map((a) => a.email.toLowerCase()));

// --- Formatting ----------------------------------------------------------------------

function listDate(ms: number) {
  const d = new Date(ms);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const days = (now.getTime() - ms) / 86_400_000;
  if (days < 6) return d.toLocaleDateString([], { weekday: "short" });
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
}

function longDate(ms: number) {
  return new Date(ms).toLocaleString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

const nameOf = (a: { name: string | null; email: string }) => a.name || a.email.split("@")[0];

/** A subject as a # tag: one line, as the daemon's suggestions label it. */
function mailLabel(subject: string) {
  const s = subject.split(/\s+/).filter(Boolean).join(" ") || "(no subject)";
  return s.length > 60 ? `${s.slice(0, 60)}…` : s;
}

function who(t: MailThread) {
  const names = t.participants.map(nameOf);
  if (names.length === 0) return "Just you";
  const shown = names.slice(0, 2).join(", ");
  const text = names.length > 2 ? `${shown} +${names.length - 2}` : shown;
  return t.last_from_me ? `To ${text}` : text;
}

/** Where quoted history starts in a plain-text body, as the daemon finds it. */
function splitQuoted(body: string): [string, string | null] {
  const lines = body.split("\n");
  const at = lines.findIndex((line, i) => {
    const t = line.trim().toLowerCase();
    return (
      (t.startsWith(">") && lines.slice(i).every((l) => !l.trim() || l.trim().startsWith(">"))) ||
      (t.startsWith("on ") && t.endsWith("wrote:")) ||
      (t.startsWith("le ") && /a écrit ?:$/.test(t)) ||
      (t.startsWith("am ") && t.endsWith("schrieb:")) ||
      t.startsWith("-----original message-----")
    );
  });
  if (at <= 0) return [body, null];
  return [lines.slice(0, at).join("\n").trimEnd(), lines.slice(at).join("\n")];
}

// --- The panel -----------------------------------------------------------------------

/**
 * The Mail panel: views sorted for the user (needs a reply, important, everything
 * else) and the usual mailboxes, the conversations in the chosen one, and the chosen
 * conversation with Summarize, Draft reply, Reply and Archive. Sending is always the
 * user's own click on a message they can read in full.
 */
export function MailView({
  onSection,
  onAsk,
  onOpenPerson,
}: {
  onSection: (s: Section) => void;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
}) {
  const [scopeState, setScopeState] = useState<MailScope>(storedScope);
  const overview = useQuery({
    queryKey: keys.mailOverview(scopeState),
    queryFn: () => api.mailOverview(scopeState),
    placeholderData: keepPreviousData,
  });
  const o = overview.data;
  // A remembered account or address that's gone (disconnected) means all mail.
  const rows = o ? scopeRows(o) : [];
  const scope: MailScope = rows.some((r) => sameScope(r.scope, scopeState)) ? scopeState : {};
  const setScope = (next: MailScope) => {
    setScopeState(next);
    storeScope(next);
    setSelected(null);
  };
  const [view, setViewState] = useState<MailBox | null>(storedView);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<number | null>(() => {
    const id = pendingThread;
    pendingThread = null;
    return id;
  });
  const [composing, setComposing] = useState<MailDraft | null>(null);
  const [connecting, setConnecting] = useState(false);

  // First visit: what needs an answer, if anything does; else the inbox.
  const current: MailBox = view ?? (o && o.sorting && o.needs_reply > 0 ? "needs_reply" : "inbox");
  const setView = (v: MailBox) => {
    setViewState(v);
    storeView(v);
    setQuery("");
  };

  if (overview.isLoading) {
    return (
      <div className="flex h-full items-center justify-center pt-[60px]">
        <Loader2 className="size-5 animate-spin text-faint" />
      </div>
    );
  }
  if (!o || o.accounts.length === 0) {
    return (
      <>
        <NoAccount onConnect={() => setConnecting(true)} onSettings={() => onSection("connections")} />
        <ConnectDialog kind={connecting ? "email" : null} onClose={() => setConnecting(false)} />
      </>
    );
  }

  const compose = () => {
    setSelected(null);
    setComposing({ connection_id: null, to: [], cc: [], subject: "", body: "", reply_to: null });
  };

  return (
    <div className="flex h-full">
      <Mailboxes
        overview={o}
        view={current}
        onView={setView}
        scope={scope}
        onScope={setScope}
        onCompose={compose}
        onSettings={onSection}
      />
      <ThreadList
        view={current}
        onView={setView}
        scope={scope}
        onScope={setScope}
        rows={rows}
        showAddress={manyAddresses(o) && !scope.address}
        mainAddresses={mainAddresses(o)}
        query={query}
        onQuery={setQuery}
        selected={composing ? null : selected}
        onSelect={(id) => {
          setComposing(null);
          setSelected(id);
        }}
        onCompose={compose}
      />
      <div className="h-full min-w-0 flex-1">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={composing ? "compose" : (selected ?? "none")}
            className="h-full"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 36, mass: 0.8 }}
          >
            {composing ? (
              <Compose draft={composing} onChange={setComposing} onClose={() => setComposing(null)} />
            ) : selected !== null ? (
              <Reader
                id={selected}
                overview={o}
                showAddress={manyAddresses(o)}
                onGone={() => setSelected(null)}
                onAsk={onAsk}
                onOpenPerson={onOpenPerson}
              />
            ) : (
              <NothingOpen view={current} />
            )}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

function NoAccount({ onConnect, onSettings }: { onConnect: () => void; onSettings: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 pt-[60px] text-center">
      <IconTile size="lg" className="bg-[#efe9fb] text-[#6146ad]">
        <Mail />
      </IconTile>
      <h1 className="type-title">Your email, sorted</h1>
      <p className="max-w-[420px] type-body text-muted-foreground">
        Connect your email and your assistant sorts what needs a reply from the rest, sums up long
        threads and drafts answers. It all happens on this computer, and nothing is sent without
        your OK.
      </p>
      <div className="mt-2 flex gap-2">
        <Button variant="secondary" onClick={onSettings}>
          Open Connections
        </Button>
        <Button onClick={onConnect}>Connect email</Button>
      </div>
    </div>
  );
}

// --- Left: views and mailboxes -------------------------------------------------------

function Mailboxes({
  overview: o,
  view,
  onView,
  scope,
  onScope,
  onCompose,
  onSettings,
}: {
  overview: MailOverview;
  view: MailBox;
  onView: (v: MailBox) => void;
  scope: MailScope;
  onScope: (s: MailScope) => void;
  onCompose: () => void;
  onSettings: (s: Section) => void;
}) {
  const rows = scopeRows(o);
  const [refreshing, setRefreshing] = useState(false);
  const count: Partial<Record<MailBox, number>> = {
    needs_reply: o.needs_reply,
    important: o.important,
    inbox: o.unread,
  };
  const refresh = async () => {
    setRefreshing(true);
    try {
      await api.refreshMail();
    } catch (e) {
      toast.error((e as Error).message);
    }
    // The check runs in the background; new mail arrives as it's found.
    setTimeout(() => setRefreshing(false), 1200);
  };
  const item = (v: (typeof views)[number]) => {
    const active = v.id === view;
    const n = count[v.id] ?? 0;
    return (
      <button
        key={v.id}
        onClick={() => onView(v.id)}
        aria-current={active ? "page" : undefined}
        className={cn(
          "flex h-9 items-center gap-2.5 rounded-[8px] px-2.5 text-left type-callout transition-colors",
          active ? "bg-fill font-medium" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
        )}
      >
        <v.icon className={cn("size-4 shrink-0", active ? "text-foreground" : "text-muted-foreground")} />
        <span className="min-w-0 flex-1 truncate">{v.label}</span>
        {n > 0 && <span className="type-footnote font-medium text-muted-foreground tabular-nums">{n}</span>}
      </button>
    );
  };
  return (
    <aside className="hidden h-full w-[224px] shrink-0 flex-col gap-4 overflow-y-auto bg-[rgb(255_255_255/0.45)] px-3 pt-[72px] pb-5 shadow-[inset_-0.5px_0_var(--separator)] lg:flex">
      <div className="flex items-center justify-between gap-2 px-1.5">
        <h1 className="type-title">Mail</h1>
        <div className="flex gap-0.5">
          <IconButton label="Check for new mail" onClick={refresh} disabled={refreshing}>
            <RefreshCw className={refreshing ? "animate-spin" : ""} />
          </IconButton>
          <IconButton label="New message" onClick={onCompose}>
            <SquarePen />
          </IconButton>
        </div>
      </div>
      {o.sorting && (
        <nav aria-label="Sorted for you" className="flex flex-col gap-0.5">
          <span className="px-2.5 pb-1 section-label">Sorted for you</span>
          {views.filter((v) => v.sorted).map(item)}
        </nav>
      )}
      <nav aria-label="Mailboxes" className="flex flex-col gap-0.5">
        <span className="px-2.5 pb-1 section-label">Mailboxes</span>
        {views.filter((v) => !v.sorted).map(item)}
      </nav>
      {rows.length > 0 && (
        <nav aria-label="Received on" className="flex flex-col gap-0.5">
          <span className="px-2.5 pb-1 section-label">Received on</span>
          <ScopeButton
            label={o.accounts.length > 1 ? "All accounts" : "All addresses"}
            icon={Layers}
            active={!scope.account && !scope.address}
            onClick={() => onScope({})}
          />
          {rows.map((r) => (
            <ScopeButton
              key={r.key}
              label={r.label}
              icon={r.account ? Mail : AtSign}
              count={r.unread}
              nested={r.nested}
              active={sameScope(r.scope, scope)}
              onClick={() => onScope(r.scope)}
            />
          ))}
        </nav>
      )}
      <div className="mt-auto flex flex-col gap-2 px-2.5 type-footnote text-faint">
        {o.sorting ? (
          o.model_locality && (
            <span className="flex flex-wrap items-center gap-1.5">
              Sorted by your model <LocalityBadge locality={o.model_locality} />
            </span>
          )
        ) : (
          <button onClick={() => onSettings("privacy")} className="text-left hover:text-foreground">
            Sorting is off. Turn it on in Settings › Privacy.
          </button>
        )}
        {rows.length === 0 &&
          o.accounts.map((a) => (
            <span key={a.connection_id} className="truncate">
              {a.email}
            </span>
          ))}
      </div>
    </aside>
  );
}

function ScopeButton({
  label,
  icon: Icon,
  count,
  nested,
  active,
  onClick,
}: {
  label: string;
  icon: typeof Inbox;
  count?: number | null;
  nested?: boolean;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-current={active ? "true" : undefined}
      title={label}
      className={cn(
        "flex h-9 items-center gap-2.5 rounded-[8px] px-2.5 text-left type-callout transition-colors",
        nested && "pl-7",
        active ? "bg-fill font-medium" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
      )}
    >
      <Icon className={cn("size-4 shrink-0", active ? "text-foreground" : "text-muted-foreground")} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {!!count && <span className="type-footnote font-medium text-muted-foreground tabular-nums">{count}</span>}
    </button>
  );
}

function IconButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={label}
          className="rounded-full text-muted-foreground"
          onClick={onClick}
          disabled={disabled}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

// --- Middle: the conversations -------------------------------------------------------

function ThreadList({
  view,
  onView,
  scope,
  onScope,
  rows,
  showAddress,
  mainAddresses,
  query,
  onQuery,
  selected,
  onSelect,
  onCompose,
}: {
  view: MailBox;
  onView: (v: MailBox) => void;
  scope: MailScope;
  onScope: (s: MailScope) => void;
  rows: ScopeRow[];
  showAddress: boolean;
  mainAddresses: Set<string>;
  query: string;
  onQuery: (q: string) => void;
  selected: number | null;
  onSelect: (id: number) => void;
  onCompose: () => void;
}) {
  const searching = query.trim().length > 0;
  // A search looks through everything, not only the current view.
  const threads = useQuery({
    queryKey: keys.mailThreads(searching ? null : view, query, scope),
    queryFn: () => api.mailThreads(searching ? null : view, query, scope),
    placeholderData: keepPreviousData,
  });
  const scopeLabel = rows.find((r) => sameScope(r.scope, scope))?.label;
  const connections = useConnections().data ?? [];
  const accounts = connections.filter((c) => c.integration === "email");
  const problem = accounts.find((c) => c.status === "error");
  const checking = accounts.some((c) => c.detail.startsWith("Checking"));
  const list = threads.data ?? [];
  const box = useRef<HTMLDivElement>(null);

  const select = (id: number) => {
    onSelect(id);
    requestAnimationFrame(() => box.current?.querySelector<HTMLElement>(`[data-id="${id}"]`)?.focus());
  };
  const step = (by: number) => {
    if (list.length === 0) return;
    const i = list.findIndex((t) => t.id === selected);
    const next = i === -1 ? (by > 0 ? 0 : list.length - 1) : Math.min(list.length - 1, Math.max(0, i + by));
    select(list[next].id);
  };
  const current = viewOf(view);

  return (
    <div className="flex h-full w-[360px] shrink-0 flex-col pt-[68px] shadow-[inset_-0.5px_0_var(--separator)] max-lg:w-[320px]">
      <div className="flex items-center justify-between gap-2 px-4 pb-3">
        {/* Below the width where the mailboxes column shows, the title switches views. */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button className="flex min-w-0 items-center gap-1 type-title lg:pointer-events-none">
              <span className="truncate">{searching ? "Search" : current.label}</span>
              <ChevronDown className="size-4 shrink-0 text-faint lg:hidden" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            {views.map((v) => (
              <DropdownMenuItem key={v.id} onSelect={() => onView(v.id)}>
                <v.icon /> {v.label}
              </DropdownMenuItem>
            ))}
            {rows.length > 0 && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => onScope({})}>
                  <Layers /> All mail
                </DropdownMenuItem>
                {rows.map((r) => (
                  <DropdownMenuItem key={r.key} onSelect={() => onScope(r.scope)}>
                    {r.account ? <Mail /> : <AtSign />} <span className="truncate">{r.label}</span>
                  </DropdownMenuItem>
                ))}
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
        <span className="lg:hidden">
          <IconButton label="New message" onClick={onCompose}>
            <SquarePen />
          </IconButton>
        </span>
      </div>
      {scopeLabel && !searching && (
        <div className="-mt-2 flex items-center gap-1.5 px-4 pb-3 type-subhead text-muted-foreground">
          <span className="min-w-0 truncate">Received on {scopeLabel}</span>
          <button
            onClick={() => onScope({})}
            aria-label="Show all mail"
            className="shrink-0 rounded-full p-0.5 text-faint hover:bg-fill hover:text-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>
      )}
      <div className="px-3 pb-2">
        <label className="flex h-9 items-center gap-2 rounded-[10px] bg-fill px-3 focus-within:bg-background focus-within:shadow-[0_0_0_3px_rgb(200_242_93/0.35)]">
          <Search className="size-4 shrink-0 text-faint" />
          <input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            onKeyDown={(e) => {
              if ((e.key === "ArrowDown" || e.key === "Enter") && list[0]) {
                e.preventDefault();
                select(list[0].id);
              } else if (e.key === "Escape") onQuery("");
            }}
            placeholder="Search all mail"
            aria-label="Search mail"
            className="min-w-0 flex-1 bg-transparent type-callout outline-none placeholder:text-[#a1a1a6]"
          />
          {searching && (
            <button onClick={() => onQuery("")} aria-label="Clear search" className="text-faint hover:text-foreground">
              <X className="size-3.5" />
            </button>
          )}
        </label>
      </div>
      {problem && (
        <div className="mx-3 mb-2 flex gap-2 rounded-[10px] bg-[#fdeeec] px-3 py-2 type-subhead text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <span>
            {problem.name}: {problem.detail}
          </span>
        </div>
      )}
      <div
        ref={box}
        role="listbox"
        aria-label="Conversations"
        className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-4"
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            step(e.key === "ArrowDown" ? 1 : -1);
          }
        }}
      >
        {threads.isLoading &&
          [0, 1, 2, 3, 4].map((i) => (
            <div key={i} className="flex flex-col gap-1.5 px-3 py-3">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-3.5 w-52" />
              <Skeleton className="h-3.5 w-60" />
            </div>
          ))}
        {!threads.isLoading && list.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-6 pt-16 text-center type-callout text-muted-foreground">
            {checking ? (
              <>
                <Loader2 className="size-5 animate-spin text-faint" />
                Fetching your mail…
              </>
            ) : searching ? (
              `Nothing matches “${query}”.`
            ) : (
              empty[view]
            )}
          </div>
        )}
        {list.map((t) => (
          <ThreadRow
            key={t.id}
            thread={t}
            active={t.id === selected}
            focusable={t.id === selected || (selected === null && t === list[0])}
            showCategory={searching || view === "inbox"}
            showAddress={showAddress && !!t.received_on && !mainAddresses.has(t.received_on)}
            onClick={() => onSelect(t.id)}
          />
        ))}
      </div>
    </div>
  );
}

const categoryPill: Record<string, { label: string; className: string }> = {
  needs_reply: { label: "Reply", className: "bg-[#fdf0dc] text-[#9a5600]" },
  important: { label: "Important", className: "bg-[#e5eefc] text-[#1f64c7]" },
};

function ThreadRow({
  thread: t,
  active,
  focusable,
  showCategory,
  showAddress,
  onClick,
}: {
  thread: MailThread;
  active: boolean;
  focusable: boolean;
  showCategory: boolean;
  showAddress: boolean;
  onClick: () => void;
}) {
  const pill = showCategory && t.category ? categoryPill[t.category] : undefined;
  return (
    <button
      data-id={t.id}
      role="option"
      aria-selected={active}
      tabIndex={focusable ? 0 : -1}
      onClick={onClick}
      className={cn(
        "relative flex flex-col gap-0.5 rounded-[10px] py-2.5 pr-3 pl-6 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/45",
        active ? "bg-[rgb(118_118_128/0.14)]" : "hover:bg-[rgb(118_118_128/0.07)]",
      )}
    >
      {t.unread && <span className="absolute top-[15px] left-2.5 size-2 rounded-full bg-[#0a84ff]" aria-label="Unread" />}
      <span className="flex items-baseline gap-2">
        <span className={cn("min-w-0 flex-1 truncate type-callout", t.unread ? "font-semibold" : "font-medium")}>
          {who(t)}
          {t.message_count > 1 && <span className="ml-1.5 font-normal text-faint tabular-nums">{t.message_count}</span>}
        </span>
        <span className="shrink-0 type-footnote text-faint tabular-nums">{listDate(t.last_at)}</span>
      </span>
      <span className="flex items-center gap-1.5">
        <span className="min-w-0 flex-1 truncate type-subhead">{t.subject || "(no subject)"}</span>
        {t.flagged && <Star className="size-3 shrink-0 fill-[#ff9f0a] text-[#ff9f0a]" aria-label="Flagged" />}
        {showAddress && t.received_on && !t.last_from_me && (
          <span title={`Received on ${t.received_on}`} className="flex min-w-0 shrink">
            <Pill className="h-[18px] max-w-[130px] bg-fill px-2 text-[11px] text-muted-foreground">
              <span className="truncate">{t.received_on}</span>
            </Pill>
          </span>
        )}
        {pill && <Pill className={cn("h-[18px] px-2 text-[11px]", pill.className)}>{pill.label}</Pill>}
      </span>
      <span className="line-clamp-2 type-subhead text-muted-foreground">
        {t.summary ? (
          <>
            <Sparkles className="mr-1 inline size-3 -translate-y-px text-faint" aria-label="Summary" />
            {t.summary}
          </>
        ) : (
          t.snippet
        )}
      </span>
    </button>
  );
}

function NothingOpen({ view }: { view: MailBox }) {
  const v = viewOf(view);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 pt-[60px] text-center">
      <IconTile size="lg" className="bg-fill text-muted-foreground">
        <v.icon />
      </IconTile>
      <p className="type-headline">Choose a conversation</p>
      <p className="max-w-sm type-callout text-muted-foreground">
        Read it here, ask for a summary, or have a reply drafted for you to check and send.
      </p>
    </div>
  );
}

// --- Right: one conversation ---------------------------------------------------------

function Reader({
  id,
  overview,
  showAddress,
  onGone,
  onAsk,
  onOpenPerson,
}: {
  id: number;
  overview: MailOverview;
  showAddress: boolean;
  onGone: () => void;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
}) {
  const onScroll = useScrollEdge();
  const assistant = useAssistantName();
  const detail = useQuery({ queryKey: keys.mailThread(id), queryFn: () => api.mailThread(id), retry: false });
  const [summary, setSummary] = useState<string | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [reply, setReply] = useState<MailDraft | null>(null);
  const [archiving, setArchiving] = useState(false);
  const d = detail.data;

  // Opening an unread conversation marks it read, as mail apps do.
  const marked = useRef(false);
  useEffect(() => {
    if (d?.thread.unread && !marked.current) {
      marked.current = true;
      api.markMailRead(id, true).catch(() => {});
    }
  }, [d, id]);

  if (detail.isError) {
    return (
      <div className="flex h-full items-center justify-center pt-[60px] type-callout text-muted-foreground">
        This conversation isn't here any more.
      </div>
    );
  }
  if (!d) {
    return (
      <div className="mx-auto flex max-w-[760px] flex-col gap-4 px-6 pt-[92px]">
        <Skeleton className="h-7 w-2/3" />
        <Skeleton className="h-40 w-full rounded-[18px]" />
      </div>
    );
  }

  const t = d.thread;
  const summarize = async () => {
    setSummarizing(true);
    try {
      setSummary((await api.summarizeMail(id)).summary);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setSummarizing(false);
    }
  };
  const archive = async () => {
    setArchiving(true);
    try {
      await api.archiveMail(id);
      toast.success("Archived");
      onGone();
    } catch (e) {
      toast.error((e as Error).message);
      setArchiving(false);
    }
  };
  const startReply = () => setReply(replyTo(d));
  // The conversation itself goes along as a # mention, so the assistant reads it.
  const ask = () => onAsk(mentionDraft("mail_thread", String(t.id), mailLabel(t.subject)));
  const openPerson = async (email: string) => {
    try {
      const hit = (await api.people(email))[0];
      if (hit) onOpenPerson(hit.id);
      else toast(`${email} isn't in your contacts.`);
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <div className="h-full overflow-y-auto" onScroll={onScroll}>
      <div className="mx-auto flex w-full max-w-[760px] flex-col gap-5 px-6 pt-[84px] pb-20">
        <header className="flex flex-col gap-3">
          <div className="flex flex-col gap-1">
            <h1 className="type-title break-words">{t.subject || "(no subject)"}</h1>
            {showAddress && t.received_on && (
              <p className="flex items-center gap-1 type-subhead text-muted-foreground">
                <AtSign className="size-3.5" /> Received on {t.received_on}
              </p>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" size="sm" onClick={startReply} disabled={!!reply}>
              <Reply /> Reply
            </Button>
            <Button variant="secondary" size="sm" onClick={summarize} disabled={summarizing}>
              {summarizing ? <Loader2 className="animate-spin" /> : <Sparkles />} Summarize
            </Button>
            <Button variant="secondary" size="sm" onClick={archive} disabled={archiving}>
              {archiving ? <Loader2 className="animate-spin" /> : <Archive />} Archive
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label="More" className="rounded-full text-muted-foreground">
                  <MoreHorizontal />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem
                  onSelect={() => api.markMailRead(id, false).then(onGone, (e) => toast.error((e as Error).message))}
                >
                  <MailOpen /> Mark as unread
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={ask}>
                  <Sparkles /> Ask {assistant} about this
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        {(summary || t.summary) && (
          <div className="flex gap-3 rounded-[16px] bg-subtle px-4 py-3">
            <Sparkles className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
              <p className="type-callout whitespace-pre-wrap">{summary ?? t.summary}</p>
              {overview.model_locality && (
                <span className="flex items-center gap-2 type-footnote text-faint">
                  Written by your model <LocalityBadge locality={overview.model_locality} className="h-5" />
                </span>
              )}
            </div>
          </div>
        )}

        <Messages messages={d.messages} onPerson={openPerson} />

        {reply ? (
          <ReplyBox threadId={id} draft={reply} onChange={setReply} onClose={() => setReply(null)} overview={overview} />
        ) : (
          <button
            onClick={startReply}
            className="surface pressable flex items-center gap-3 px-4 py-3.5 text-left type-callout text-muted-foreground"
          >
            <Reply className="size-4" /> Reply to {t.participants[0] ? nameOf(t.participants[0]) : "this conversation"}…
          </button>
        )}
      </div>
    </div>
  );
}

function replyTo(d: MailThreadDetail): MailDraft {
  const lastOther = [...d.messages].reverse().find((m) => !m.from_me);
  const to = lastOther ? [lastOther.from.email] : (d.messages.at(-1)?.to.map((a) => a.email) ?? []);
  const subject = /^(re|aw|sv|antw|réf?)\s*:/i.test(d.thread.subject) ? d.thread.subject : `Re: ${d.thread.subject}`;
  return { connection_id: d.thread.connection_id, to, cc: [], subject, body: "", reply_to: d.thread.id };
}

function Messages({ messages, onPerson }: { messages: MailMessage[]; onPerson: (email: string) => void }) {
  // Long threads start with the older messages folded, like mail apps.
  const [open, setOpen] = useState<Set<number>>(() => new Set(messages.slice(-2).map((m) => m.id)));
  return (
    <div className="flex flex-col gap-3">
      {messages.map((m) =>
        open.has(m.id) ? (
          <MessageCard key={m.id} message={m} onPerson={onPerson} />
        ) : (
          <button
            key={m.id}
            onClick={() => setOpen((s) => new Set(s).add(m.id))}
            className="surface pressable flex items-center gap-3 px-4 py-3 text-left"
          >
            <span className="w-32 shrink-0 truncate type-callout font-medium">{m.from_me ? "You" : nameOf(m.from)}</span>
            <span className="min-w-0 flex-1 truncate type-subhead text-muted-foreground">{m.body.slice(0, 200)}</span>
            <span className="shrink-0 type-footnote text-faint">{listDate(m.date)}</span>
          </button>
        ),
      )}
    </div>
  );
}

function MessageCard({ message: m, onPerson }: { message: MailMessage; onPerson: (email: string) => void }) {
  const [text, quoted] = splitQuoted(m.body);
  const [showQuoted, setShowQuoted] = useState(false);
  const recipients = [...m.to, ...m.cc];
  return (
    <article className="surface flex flex-col gap-3 px-5 py-4">
      <header className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            {m.from_me ? (
              <span className="type-callout font-semibold">You</span>
            ) : (
              <button
                onClick={() => onPerson(m.from.email)}
                className="truncate type-callout font-semibold hover:underline"
                title={m.from.email}
              >
                {nameOf(m.from)}
              </button>
            )}
            {!m.from_me && m.from.name && <span className="truncate type-footnote text-faint">{m.from.email}</span>}
          </div>
          {recipients.length > 0 && (
            <div className="truncate type-footnote text-faint">to {recipients.map(nameOf).join(", ")}</div>
          )}
        </div>
        <span className="shrink-0 type-footnote text-faint">{longDate(m.date)}</span>
      </header>
      {/* Plain text only: nothing in an email is turned into a link or formatting. */}
      <div className="type-body leading-[1.55] break-words whitespace-pre-wrap">{text || " "}</div>
      {quoted && (
        <div>
          <button
            onClick={() => setShowQuoted((s) => !s)}
            aria-label={showQuoted ? "Hide quoted text" : "Show quoted text"}
            className="rounded-full bg-fill px-2.5 py-0.5 type-footnote font-semibold text-muted-foreground hover:text-foreground"
          >
            •••
          </button>
          {showQuoted && (
            <div className="mt-2 border-l-2 border-separator pl-3 type-subhead break-words whitespace-pre-wrap text-muted-foreground">
              {quoted}
            </div>
          )}
        </div>
      )}
      {m.attachments.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {m.attachments.map((a, i) => (
            <Pill key={`${a}-${i}`} className="max-w-[240px]">
              <Paperclip className="size-3 shrink-0" />
              <span className="truncate">{a}</span>
            </Pill>
          ))}
          <span className="type-footnote text-faint">Open attachments in your usual mail app.</span>
        </div>
      )}
    </article>
  );
}

function ReplyBox({
  threadId,
  draft,
  onChange,
  onClose,
  overview,
}: {
  threadId: number;
  draft: MailDraft;
  onChange: (d: MailDraft) => void;
  onClose: () => void;
  overview: MailOverview;
}) {
  const [instructions, setInstructions] = useState("");
  const [writing, setWriting] = useState(false);
  // Remounts the editor when the assistant fills it in.
  const [version, setVersion] = useState(0);
  const { sending, send } = useSendDraft(onClose);
  const write = async () => {
    setWriting(true);
    try {
      const d = await api.draftMailReply(threadId, instructions.trim() || null);
      onChange({ ...draft, body: d.body, to: draft.to.length ? draft.to : d.to });
      setVersion((v) => v + 1);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setWriting(false);
    }
  };
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 420, damping: 34 }}
      className="overflow-hidden rounded-[18px] bg-background shadow-[var(--shadow-raised)]"
    >
      <form
        className="flex items-center gap-2 bg-subtle/70 px-3 py-2"
        onSubmit={(e) => {
          e.preventDefault();
          write();
        }}
      >
        <Sparkles className="ml-1 size-4 shrink-0 text-muted-foreground" />
        <input
          value={instructions}
          onChange={(e) => setInstructions(e.target.value)}
          placeholder="What should it say? (optional)"
          aria-label="What the reply should say"
          className="min-w-0 flex-1 bg-transparent type-callout outline-none placeholder:text-[#a1a1a6]"
        />
        {overview.model_locality && <LocalityBadge locality={overview.model_locality} />}
        <Button type="submit" size="sm" variant="secondary" disabled={writing}>
          {writing && <Loader2 className="animate-spin" />} {draft.body ? "Write again" : "Draft it for me"}
        </Button>
      </form>
      <DraftEditor key={version} draft={draft} onChange={onChange} autoFocus="body" className="border-t-[0.5px] border-separator" />
      <div className="flex items-center justify-between gap-3 border-t-[0.5px] border-separator px-4 py-2.5">
        <span className="type-footnote text-faint">Sent from your account when you press Send.</span>
        <div className="flex gap-2">
          <Button variant="ghost" onClick={onClose} disabled={sending}>
            Discard
          </Button>
          <SendButton sending={sending} onClick={() => send(draft)} disabled={!draft.body.trim() || draft.to.length === 0} />
        </div>
      </div>
    </motion.div>
  );
}

function Compose({
  draft,
  onChange,
  onClose,
}: {
  draft: MailDraft;
  onChange: (d: MailDraft) => void;
  onClose: () => void;
}) {
  const onScroll = useScrollEdge();
  const { sending, send } = useSendDraft(onClose);
  return (
    <div className="h-full overflow-y-auto" onScroll={onScroll}>
      <div className="mx-auto flex w-full max-w-[760px] flex-col gap-5 px-6 pt-[84px] pb-20">
        <h1 className="type-title">New message</h1>
        <div className="overflow-hidden rounded-[18px] bg-background shadow-[var(--shadow-card)]">
          <DraftEditor draft={draft} onChange={onChange} autoFocus="to" />
          <div className="flex items-center justify-end gap-2 border-t-[0.5px] border-separator px-4 py-2.5">
            <Button variant="ghost" onClick={onClose} disabled={sending}>
              Discard
            </Button>
            <SendButton sending={sending} onClick={() => send(draft)} disabled={draft.to.length === 0 || !draft.body.trim()} />
          </div>
        </div>
      </div>
    </div>
  );
}
