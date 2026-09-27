import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  BellRing,
  Blocks,
  BookOpen,
  ChevronRight,
  ExternalLink,
  Loader2,
  Lock,
  LogOut,
  Power,
  RotateCcw,
  Send,
  Settings,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import { LocalityBadge } from "@/components/locality-badge";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConnectionsView } from "@/features/connections/connections-view";
import { MemoryView } from "@/features/memory/memory-view";
import { ModelsView } from "@/features/models/models-view";
import { NotificationsSettings } from "@/features/reminders/reminders";
import type { Section as Place, SettingsPage } from "@/features/shell/top-bar";
import { api } from "@/lib/api";
import { mod } from "@/lib/platform";
import { useConnections, useProviders, useSettings } from "@/lib/queries";
import {
  type BackgroundStatus,
  backgroundStatus,
  isTauri,
  openInBrowser,
  request,
  setBackground,
} from "@/lib/transport";

export const settingsPages: {
  id: SettingsPage;
  label: string;
  icon: typeof Settings;
  tone: string;
}[] = [
  { id: "general", label: "General", icon: Settings, tone: "bg-[#8e8e93] text-white" },
  { id: "connections", label: "Connections", icon: Blocks, tone: "bg-[#0a84ff] text-white" },
  { id: "models", label: "Models", icon: Sparkles, tone: "bg-lime text-lime-ink" },
  { id: "memory", label: "Memory", icon: BookOpen, tone: "bg-[#bf5af2] text-white" },
  { id: "notifications", label: "Reminders & notifications", icon: BellRing, tone: "bg-[#ff3b30] text-white" },
  { id: "privacy", label: "Privacy", icon: Lock, tone: "bg-private text-white" },
];

const shortcuts = [
  ["New chat", `${mod}N`],
  ["Search conversations", `${mod}K`],
  ["Chat, Calendar, Mail, People", `${mod}1 – 4`],
  ["Settings", `${mod},`],
  ["Calendar: previous or next week", "← →"],
  ["Calendar: today", "T"],
];

/**
 * Everything set up once, like System Settings: a sidebar of sections and the chosen
 * one beside it. Daily panels stay in the top bar.
 */
export function SettingsView({
  page,
  onSection,
  onOpenConversation,
}: {
  page: SettingsPage;
  onSection: (s: Place) => void;
  onOpenConversation: (id: string) => void;
}) {
  const nav = useRef<HTMLElement>(null);
  // Up and down move through the sidebar, like a list.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const i = settingsPages.findIndex((p) => p.id === page);
    const next = settingsPages[(i + (e.key === "ArrowDown" ? 1 : -1) + settingsPages.length) % settingsPages.length];
    onSection(next.id);
    requestAnimationFrame(() => nav.current?.querySelector<HTMLButtonElement>(`[data-page="${next.id}"]`)?.focus());
  };

  return (
    <div className="flex h-full">
      <aside className="flex h-full w-[268px] shrink-0 flex-col gap-3 overflow-y-auto bg-[rgb(255_255_255/0.45)] px-3 pt-[76px] pb-6 shadow-[inset_-0.5px_0_var(--separator)]">
        <h1 className="px-2.5 type-title">Settings</h1>
        <nav ref={nav} aria-label="Settings" className="flex flex-col gap-0.5" onKeyDown={onKeyDown}>
          {settingsPages.map((p) => {
            const active = p.id === page;
            return (
              <button
                key={p.id}
                data-page={p.id}
                onClick={() => onSection(p.id)}
                aria-current={active ? "page" : undefined}
                tabIndex={active ? 0 : -1}
                className={cn(
                  "flex h-9 items-center gap-2.5 rounded-[8px] px-2 text-left type-callout transition-colors",
                  active ? "bg-fill font-medium text-foreground" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
                )}
              >
                <IconTile className={cn("size-6 rounded-[6px] [&_svg]:size-[14px]", p.tone)}>
                  <p.icon strokeWidth={2.2} />
                </IconTile>
                <span className="truncate">{p.label}</span>
              </button>
            );
          })}
        </nav>
      </aside>
      <div className="h-full min-w-0 flex-1">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={page}
            className="h-full"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 36, mass: 0.8 }}
          >
            {page === "general" && <GeneralSettings />}
            {page === "connections" && <ConnectionsView onPeople={() => onSection("people")} />}
            {page === "models" && <ModelsView onChat={() => onSection("chat")} />}
            {page === "memory" && <MemoryView />}
            {page === "notifications" && (
              <NotificationsSettings onOpenConversation={onOpenConversation} onCalendar={() => onSection("calendar")} />
            )}
            {page === "privacy" && <PrivacySettings onSection={onSection} />}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

function GeneralSettings() {
  const settings = useSettings().data;
  const [name, setName] = useState<string | null>(null);
  const value = name ?? settings?.assistant_name ?? "";

  const saveName = async () => {
    if (!settings || name === null || !value.trim()) return;
    try {
      await api.putSettings({ ...settings, assistant_name: value });
      setName(null);
      toast.success("Saved");
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Page>
      <PageHeader title="General" subtitle="Your assistant's name, how it runs, and shortcuts." />
      <Section title="Assistant">
        <Grouped>
          <form
            className="flex items-center gap-3 px-4 py-3"
            onSubmit={(e) => {
              e.preventDefault();
              saveName();
            }}
          >
            <label htmlFor="assistant-name" className="w-20 shrink-0 type-body">
              Name
            </label>
            <Input
              id="assistant-name"
              value={value}
              onChange={(e) => setName(e.target.value)}
              onBlur={saveName}
              className="h-9 max-w-sm"
            />
            {name !== null && (
              <Button type="submit" size="sm" disabled={!value.trim()}>
                Save
              </Button>
            )}
          </form>
          <Row
            icon={
              <IconTile size="sm" className="bg-fill text-muted-foreground">
                <RotateCcw />
              </IconTile>
            }
            title="Show the welcome again"
            detail="Choose a model, connect apps and introduce yourself"
            trailing={<ChevronRight className="size-4 text-faint" />}
            onClick={() => {
              if (!settings) return;
              api
                .putSettings({ ...settings, onboarding_done: false, onboarding_step: 0 })
                .catch((e) => toast.error((e as Error).message));
            }}
          />
        </Grouped>
      </Section>

      {isTauri && <BackgroundGroup />}

      <Section title="Keyboard shortcuts">
        <Grouped>
          {shortcuts.map(([label, keys]) => (
            <div key={label} className="flex h-11 items-center justify-between px-4 type-callout">
              <span>{label}</span>
              <kbd className="font-sans type-subhead text-muted-foreground">{keys}</kbd>
            </div>
          ))}
        </Grouped>
      </Section>

      <Section title={isTauri ? "Browser" : "This browser"}>
        {isTauri ? (
          <Button
            variant="outline"
            className="self-start"
            onClick={() => openInBrowser().catch((e) => toast.error(e?.message ?? String(e)))}
          >
            <ExternalLink /> Open in your browser
          </Button>
        ) : (
          <Button
            variant="outline"
            className="self-start"
            onClick={() => request("POST", "/web/logout").finally(() => location.reload())}
          >
            <LogOut /> Sign out of this browser
          </Button>
        )}
      </Section>
    </Page>
  );
}

function PrivacySettings({ onSection }: { onSection: (s: Place) => void }) {
  const providers = useProviders().data ?? [];
  const connections = useConnections().data ?? [];
  const telegram = connections.some((c) => c.integration === "telegram");
  return (
    <Page>
      <PageHeader
        title="Privacy"
        subtitle="What stays on this computer, and what goes elsewhere when you ask for it."
      />
      <Section title="Your data">
        <Grouped>
          <Row
            icon={
              <IconTile size="sm" className="bg-private-soft text-private">
                <Lock />
              </IconTile>
            }
            title="Stored on this computer"
            detail="Conversations, memory, settings and passwords are encrypted here, and nowhere else."
            className="[&_.truncate]:whitespace-normal"
          />
          <Row
            icon={
              <IconTile size="sm" className="bg-private-soft text-private">
                <ShieldCheck />
              </IconTile>
            }
            title="No account, no tracking"
            detail="Mimi has no server of its own. Nothing is sent to the people who make it."
            className="[&_.truncate]:whitespace-normal"
          />
          <Row
            onClick={() => onSection("memory")}
            icon={
              <IconTile size="sm" className="bg-[#bf5af2] text-white">
                <BookOpen />
              </IconTile>
            }
            title="What your assistant remembers"
            detail="See, change or forget it"
            trailing={<ChevronRight className="size-4 text-faint" />}
          />
        </Grouped>
      </Section>

      {providers.length > 0 && (
        <Section
          title="Where your messages go"
          action={
            <Button variant="ghost" size="sm" onClick={() => onSection("models")}>
              Change model
            </Button>
          }
        >
          <Grouped>
            {providers.map((p) => (
              <Row key={p.id} title={p.name} trailing={<LocalityBadge locality={p.locality} />} className="min-h-[52px]" />
            ))}
          </Grouped>
        </Section>
      )}

      {telegram && (
        <Section title="Telegram">
          <Grouped>
            <Row
              icon={
                <IconTile size="sm" className="bg-network-soft text-network">
                  <Send />
                </IconTile>
              }
              title="Bot messages aren't end-to-end encrypted"
              detail="Telegram can read what you and your bot send each other. Keep private things in this app."
              className="[&_.truncate]:whitespace-normal"
            />
          </Grouped>
        </Section>
      )}
    </Page>
  );
}

/** Desktop only: keep the assistant running (and starting at login) with no window. */
function BackgroundGroup() {
  const [status, setStatus] = useState<BackgroundStatus | null>(null);
  const [busy, setBusy] = useState(false);

  // Read each time this page opens: the service can change outside the app.
  useEffect(() => {
    backgroundStatus().then(setStatus).catch(() => setStatus(null));
  }, []);

  const toggle = async (enabled: boolean) => {
    setBusy(true);
    try {
      setStatus(await setBackground(enabled));
      toast.success(enabled ? "Mimi will keep running in the background" : "Mimi now runs only while it's open");
    } catch (e) {
      toast.error((e as Error).message);
      backgroundStatus().then(setStatus).catch(() => {});
    } finally {
      setBusy(false);
    }
  };

  const detail = !status
    ? "Checking…"
    : !status.available
      ? (status.unavailable_reason ?? "Not available on this computer.")
      : status.enabled
        ? status.no_restart
          ? "Starts when you log in, so Telegram and reminders work with this window closed."
          : "Starts when you log in and keeps going with this window closed, so Telegram and reminders always work."
        : "Mimi only works while this window is open.";

  return (
    <Section title="Background">
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="bg-private-soft text-private">
              {busy ? <Loader2 className="animate-spin" /> : <Power />}
            </IconTile>
          }
          title="Keep Mimi running in the background"
          detail={detail}
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <Switch
              checked={!!status?.enabled}
              disabled={!status?.available || busy}
              onCheckedChange={toggle}
              aria-label="Keep Mimi running in the background"
            />
          }
        />
      </Grouped>
    </Section>
  );
}
