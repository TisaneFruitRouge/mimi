import { useState } from "react";
import { motion } from "motion/react";
import { useQuery } from "@tanstack/react-query";
import {
  CalendarDays,
  Check,
  ChevronRight,
  Hash,
  Lock,
  Mail,
  MessageCircle,
  MoreHorizontal,
  Phone,
  Plug,
  Send,
  Users,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Connection } from "@/bindings/Connection";
import type { Integration } from "@/bindings/Integration";
import { Grouped, IconTile, Page, PageHeader, Pill, Row, Section } from "@/components/page";
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
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { type ConnectKind, ConnectDialog } from "@/features/connections/connect-dialogs";
import { PersonAvatar } from "@/components/people";
import { api, keys } from "@/lib/api";
import { useConnections, useIntegrations } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

const connectable = (id: string): id is ConnectKind =>
  id === "google_calendar" || id === "caldav" || id === "telegram" || id === "signal" || id === "matrix" || id === "email";

/** Integrations that make sense to connect more than once. */
const repeatable = (id: string) =>
  id === "google_calendar" || id === "caldav" || id === "carddav" || id === "email";

const look: Record<string, { icon: typeof Plug; tone: string }> = {
  google_calendar: { icon: CalendarDays, tone: "bg-event-soft text-event" },
  google: { icon: CalendarDays, tone: "bg-event-soft text-event" },
  caldav: { icon: CalendarDays, tone: "bg-event-soft text-event" },
  google_contacts: { icon: Users, tone: "bg-[#fbeedd] text-[#9a5a12]" },
  carddav: { icon: Users, tone: "bg-[#fbeedd] text-[#9a5a12]" },
  telegram: { icon: Send, tone: "bg-[#def3f7] text-[#136c86]" },
  signal: { icon: MessageCircle, tone: "bg-[#e5ecfb] text-[#3353a8]" },
  matrix: { icon: Hash, tone: "bg-fill text-foreground" },
  whatsapp: { icon: Phone, tone: "bg-[#e1f4e6] text-[#1d7a3a]" },
  email: { icon: Mail, tone: "bg-[#efe9fb] text-[#6146ad]" },
};
const lookOf = (id: string) => look[id] ?? { icon: Plug, tone: "bg-fill text-foreground" };

/** The tinted icon that stands for an integration everywhere in the app. */
export function IntegrationIcon({ id, size = "md" }: { id: string; size?: "sm" | "md" | "lg" }) {
  const { icon: Icon, tone } = lookOf(id);
  return (
    <IconTile size={size} className={tone}>
      <Icon />
    </IconTile>
  );
}

export function ConnectionsView({ onPeople }: { onPeople: () => void }) {
  const integrations = useIntegrations();
  const connections = useConnections().data ?? [];
  const [open, setOpen] = useState<Integration | null>(null);
  const [connecting, setConnecting] = useState<ConnectKind | null>(null);
  // A Google connection that needs signing in again.
  const [reconnect, setReconnect] = useState<string | undefined>(undefined);
  const [removing, setRemoving] = useState<Connection | null>(null);
  const all = integrations.data ?? [];
  const available = all.filter(
    (i) => i.status === "available" || (i.status === "connected" && repeatable(i.id)),
  );
  const soon = all.filter((i) => i.status === "coming_soon");
  const startConnect = (i: Integration) => {
    setOpen(null);
    // Address books come with the calendar account (same app password).
    const kind = i.id === "carddav" ? "caldav" : i.id;
    if (connectable(kind)) setConnecting(kind);
  };

  return (
    <Page>
      <PageHeader
        title="Connections"
        subtitle="Let your assistant help with the apps you use. It only sees what you connect, and always asks before it sends or changes anything."
      />

      <PeopleEntry onOpen={onPeople} />

      <Section title="Connected">
        {connections.length === 0 ? (
          <div className="surface flex items-center gap-4 px-5 py-5">
            <IconTile className="bg-fill text-muted-foreground">
              <Plug />
            </IconTile>
            <div>
              <p className="type-body font-medium">Nothing connected yet</p>
              <p className="type-subhead text-muted-foreground">
                Connect a calendar, your email or a messaging app below to get started.
              </p>
            </div>
          </div>
        ) : (
          <Grouped>
            {connections.map((c) => (
              <ConnectionRow
                key={c.id}
                connection={c}
                onRemove={() => setRemoving(c)}
                onSignInAgain={() => {
                  setReconnect(c.id);
                  setConnecting("google_calendar");
                }}
                onShowCode={() => setConnecting("signal")}
              />
            ))}
          </Grouped>
        )}
      </Section>

      <Section title="Available">
        <div className="grid grid-cols-3 gap-3">
          {integrations.isLoading &&
            [0, 1, 2].map((i) => <Skeleton key={i} className="h-[172px] rounded-[18px]" />)}
          {available.map((i, n) => (
            <motion.div
              key={i.id}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ type: "spring", stiffness: 380, damping: 32, delay: n * 0.04 }}
            >
              <IntegrationCard integration={i} onOpen={() => setOpen(i)} onConnect={() => startConnect(i)} />
            </motion.div>
          ))}
        </div>
      </Section>

      {soon.length > 0 && (
        <Section title="Coming soon">
          <div className="grid grid-cols-3 gap-3">
            {soon.map((i) => (
              <button
                key={i.id}
                onClick={() => setOpen(i)}
                className="pressable flex items-center gap-3 rounded-[16px] bg-[rgb(255_255_255/0.55)] px-3.5 py-3 text-left shadow-[0_0_0_0.5px_rgb(0_0_0/0.05)] hover:bg-background"
              >
                <span className="opacity-60 grayscale-[0.4]">
                  <IntegrationIcon id={i.id} size="sm" />
                </span>
                <span className="min-w-0 flex-1 truncate type-callout font-medium text-muted-foreground">
                  {i.name}
                </span>
              </button>
            ))}
          </div>
        </Section>
      )}

      <p className="flex items-center justify-center gap-1.5 type-footnote text-faint">
        <Lock className="size-3" />
        Sign-ins are stored encrypted on this computer. Disconnecting deletes them.
      </p>

      <IntegrationSheet integration={open} onClose={() => setOpen(null)} onConnect={startConnect} />
      <ConnectDialog
        kind={connecting}
        reconnect={reconnect}
        onClose={() => {
          setConnecting(null);
          setReconnect(undefined);
        }}
      />
      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Disconnect {removing?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              {removing?.integration === "signal" ? (
                <>
                  Your assistant stops reading Note to Self right away, and everything this computer
                  kept for Signal is deleted. On your phone, also remove it under Signal › Settings ›
                  Linked devices.
                </>
              ) : (
                <>
                  Your assistant loses access right away and the saved sign-in is deleted. Nothing is
                  deleted on the service itself.
                </>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => removing && api.disconnect(removing.id).catch((e) => toast.error(e.message))}
            >
              Disconnect
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

function IntegrationCard({
  integration: i,
  onOpen,
  onConnect,
}: {
  integration: Integration;
  onOpen: () => void;
  onConnect: () => void;
}) {
  const again = i.status === "connected";
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onOpen()}
      className="surface flex h-full cursor-pointer flex-col gap-4 p-4 text-left transition-shadow duration-200 hover:shadow-[var(--shadow-raised)]"
    >
      <IntegrationIcon id={i.id} />
      <div className="flex flex-col gap-1">
        <span className="type-body font-medium">{i.name}</span>
        <span className="line-clamp-2 type-subhead text-muted-foreground">{i.description}</span>
      </div>
      <Button
        size="sm"
        variant={again ? "secondary" : "default"}
        className="mt-auto self-start rounded-full px-4"
        onClick={(e) => {
          e.stopPropagation();
          onConnect();
        }}
      >
        {again ? "Add another" : "Connect"}
      </Button>
    </div>
  );
}

function ConnectionRow({
  connection: c,
  onRemove,
  onSignInAgain,
  onShowCode,
}: {
  connection: Connection;
  onRemove: () => void;
  onSignInAgain: () => void;
  /** Signal: show the code to link it (never opened as a link). */
  onShowCode: () => void;
}) {
  const dot = { ok: "bg-private", needs_action: "bg-cloud", error: "bg-destructive" }[c.status];
  const signal = c.integration === "signal";
  // Waiting for a scan, or unlinked from the phone (the daemon's line says "Link again").
  const signalLink = signal && (c.status === "needs_action" || /link again/i.test(c.detail));
  return (
    <Row
      icon={<IntegrationIcon id={c.integration} />}
      title={
        <span className="flex items-center gap-2">
          <span className="truncate">{c.name}</span>
          <span className={cn("size-1.5 shrink-0 rounded-full", dot)} aria-hidden />
        </span>
      }
      detail={
        <span
          className={cn(
            c.status === "error" && "text-destructive",
            c.status === "needs_action" && "text-cloud",
          )}
        >
          {c.detail}
        </span>
      }
      trailing={
        <>
          {c.action_url && !signal && (
            <Button size="sm" variant="lime" className="rounded-full px-3.5" onClick={() => openExternal(c.action_url!)}>
              Finish setup
            </Button>
          )}
          {signalLink && (
            <Button size="sm" variant="lime" className="rounded-full px-3.5" onClick={onShowCode}>
              {c.status === "error" ? "Link again" : "Show code"}
            </Button>
          )}
          {c.integration === "google" && c.status === "error" && (
            <Button size="sm" variant="lime" className="rounded-full px-3.5" onClick={onSignInAgain}>
              Sign in again
            </Button>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label={`Options for ${c.name}`} className="rounded-full text-muted-foreground">
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem variant="destructive" onSelect={onRemove}>
                Disconnect
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    />
  );
}

function IntegrationSheet({
  integration: i,
  onClose,
  onConnect,
}: {
  integration: Integration | null;
  onClose: () => void;
  onConnect: (i: Integration) => void;
}) {
  return (
    <Dialog open={!!i} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 p-0 sm:max-w-[420px]">
        {i && (
          <div className="flex flex-col items-center gap-5 px-7 pt-8 pb-7 text-center">
            <IntegrationIcon id={i.id} size="lg" />
            <div className="flex flex-col gap-1.5">
              <DialogTitle className="type-title">{i.name}</DialogTitle>
              <DialogDescription>{i.description}</DialogDescription>
            </div>
            <div className="flex w-full flex-col gap-2 text-left">
              <span className="section-label">What your assistant can do</span>
              <Grouped className="bg-subtle! shadow-none!">
                {i.abilities.map((a) => (
                  <div key={a} className="flex items-start gap-3 px-4 py-3 type-callout">
                    <Check className="mt-0.5 size-4 shrink-0 text-private" strokeWidth={2.4} />
                    {a}
                  </div>
                ))}
              </Grouped>
            </div>
            {i.status === "coming_soon" ? (
              <Pill className="h-8 px-4 type-subhead">Coming in an upcoming update</Pill>
            ) : (
              <Button size="lg" className="w-full" onClick={() => onConnect(i)}>
                {i.status === "connected" ? "Add another" : `Connect ${i.name}`}
              </Button>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** The way into People: everyone the connected apps know, in one place. */
function PeopleEntry({ onOpen }: { onOpen: () => void }) {
  const people = useQuery({ queryKey: keys.peopleList(""), queryFn: () => api.people("") }).data;
  const count = people?.length ?? 0;
  return (
    <button onClick={onOpen} className="surface pressable group flex items-center gap-4 px-5 py-4 text-left">
      <div className="flex -space-x-2">
        {(people ?? []).slice(0, 3).map((p) => (
          <span key={p.id} className="rounded-full ring-2 ring-background">
            <PersonAvatar id={p.id} name={p.name} />
          </span>
        ))}
        {count === 0 && (
          <IconTile className="bg-fill text-muted-foreground">
            <Users />
          </IconTile>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="type-body font-medium">People</div>
        <div className="type-subhead text-muted-foreground">
          {count === 0
            ? "Add the people you talk about, or connect an address book"
            : `${count} ${count === 1 ? "person" : "people"} you can mention with @`}
        </div>
      </div>
      <ChevronRight className="size-4 text-faint transition group-hover:translate-x-0.5" />
    </button>
  );
}
