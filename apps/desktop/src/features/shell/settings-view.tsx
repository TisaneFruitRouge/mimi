import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  BellRing,
  Blocks,
  BookOpen,
  ChevronRight,
  ExternalLink,
  Hand,
  Loader2,
  Lock,
  LogOut,
  Mail,
  Power,
  RotateCcw,
  Send,
  Settings,
  ShieldCheck,
  Smile,
  Sparkles,
  TriangleAlert,
  AudioLines,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import { LocalityBadge } from "@/components/locality-badge";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useQuery } from "@tanstack/react-query";
import type { MailSorter } from "@/bindings/MailSorter";
import { ConnectionsView } from "@/features/connections/connections-view";
import { MemoryView } from "@/features/memory/memory-view";
import { ModelsView } from "@/features/models/models-view";
import { UndoSendSetting } from "@/features/mail/send-later";
import { PermissionsView } from "@/features/permissions/permissions-view";
import { PersonalityView } from "@/features/personality/personality-view";
import { NotificationsSettings } from "@/features/reminders/reminders";
import { UpdatesGroup } from "@/features/shell/updates";
import { VoiceView } from "@/features/voice/voice-view";
import type { Section as Place, SettingsPage } from "@/features/shell/top-bar";
import { api, keys } from "@/lib/api";
import { mod } from "@/lib/platform";
import { useAssistantName, useConnections, useProviders, useSettings } from "@/lib/queries";
import {
  type BackgroundStatus,
  backgroundStatus,
  isTauri,
  openExternal,
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
  { id: "personality", label: "Personality", icon: Smile, tone: "bg-[#ff2d55] text-white" },
  { id: "connections", label: "Connections", icon: Blocks, tone: "bg-[#0a84ff] text-white" },
  { id: "models", label: "Models", icon: Sparkles, tone: "bg-lime text-lime-ink" },
  { id: "voice", label: "Voice", icon: AudioLines, tone: "bg-[#5e5ce6] text-white" },
  { id: "memory", label: "Memory", icon: BookOpen, tone: "bg-[#bf5af2] text-white" },
  { id: "notifications", label: "Reminders & notifications", icon: BellRing, tone: "bg-[#ff3b30] text-white" },
  { id: "permissions", label: "Permissions", icon: Hand, tone: "bg-[#ff9f0a] text-white" },
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
            {page === "general" && <GeneralSettings onSection={onSection} />}
            {page === "personality" && <PersonalityView />}
            {page === "connections" && <ConnectionsView onPeople={() => onSection("people")} />}
            {page === "models" && <ModelsView onChat={() => onSection("chat")} />}
            {page === "voice" && <VoiceView />}
            {page === "memory" && <MemoryView />}
            {page === "notifications" && (
              <NotificationsSettings onOpenConversation={onOpenConversation} onCalendar={() => onSection("calendar")} />
            )}
            {page === "permissions" && <PermissionsView />}
            {page === "privacy" && <PrivacySettings onSection={onSection} />}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

function GeneralSettings({ onSection }: { onSection: (s: Place) => void }) {
  const settings = useSettings().data;
  const assistant = useAssistantName();
  const email = (useConnections().data ?? []).some((c) => c.integration === "email");

  return (
    <Page>
      <PageHeader title="General" subtitle="How your assistant runs, updates, and keyboard shortcuts." />
      <Section title="Assistant">
        <Grouped>
          <Row
            icon={
              <IconTile size="sm" className="bg-[#ff2d55] text-white">
                <Smile />
              </IconTile>
            }
            title="Name and personality"
            detail={`${assistant}, how it talks and what it keeps in mind`}
            trailing={<ChevronRight className="size-4 text-faint" />}
            onClick={() => onSection("personality")}
          />
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

      {email && <UndoSendSetting />}

      <UpdatesGroup />

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
  const signal = connections.some((c) => c.integration === "signal");
  const matrix = connections.filter((c) => c.integration === "matrix");
  const email = connections.some((c) => c.integration === "email");
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

      {email && <MailSortingGroup />}

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

      {signal && (
        <Section title="Signal">
          <Grouped>
            <Row
              icon={
                <IconTile size="sm" className="bg-private-soft text-private">
                  <Lock />
                </IconTile>
              }
              title="Only Note to Self"
              detail="Linked like Signal on a computer, it receives your other chats too, but ignores them: nothing from them is read or kept. Messages stay end-to-end encrypted."
              className="[&_.truncate]:whitespace-normal"
            />
          </Grouped>
        </Section>
      )}
      {matrix.length > 0 && (
        <Section title="Matrix">
          <Grouped>
            {matrix.map((c) => {
              const plain = c.detail.includes("not encrypted");
              return (
                <Row
                  key={c.id}
                  icon={
                    <IconTile size="sm" className={plain ? "bg-cloud-soft text-cloud" : "bg-private-soft text-private"}>
                      {plain ? <TriangleAlert /> : <Lock />}
                    </IconTile>
                  }
                  title={plain ? "Your chat isn't end-to-end encrypted" : "End-to-end encrypted"}
                  detail={
                    plain
                      ? "Your Matrix server can read it. Turn on encryption in the chat's settings."
                      : "Only your devices and this computer can read your chat with your assistant."
                  }
                  className="[&_.truncate]:whitespace-normal"
                />
              );
            })}
          </Grouped>
        </Section>
      )}
    </Page>
  );
}

/**
 * Whether new mail is sorted in the background, and by what: the user's own model (the
 * default), or Jev, a cloud decision model, with their TypeSafe key.
 */
function MailSortingGroup() {
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const overview = useQuery({ queryKey: keys.mailOverview(), queryFn: () => api.mailOverview() }).data;
  const [askingKey, setAskingKey] = useState(false);
  const model = providers.find((p) => p.id === settings?.default_model?.provider_id);
  const sorter = settings?.mail_sorter ?? "model";
  const toggle = (on: boolean) => {
    if (!settings) return;
    api.putSettings({ ...settings, mail_sorting: on }).catch((e) => toast.error((e as Error).message));
  };
  const choose = (next: MailSorter) => {
    if (!settings || next === sorter) return;
    if (next === "jev" && !overview?.jev_connected) {
      setAskingKey(true);
      return;
    }
    api.putSettings({ ...settings, mail_sorter: next }).catch((e) => toast.error((e as Error).message));
  };
  const removeKey = () =>
    api
      .jevDisconnect()
      .then(() => toast.success("TypeSafe key removed. Your model sorts your mail again."))
      .catch((e) => toast.error((e as Error).message));
  return (
    <Section title="Email">
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="bg-[#efe9fb] text-[#6146ad]">
              <Mail />
            </IconTile>
          }
          title="Sort new mail in the background"
          detail="Files each new email under what needs a reply, what's important and everything else. Newsletters are recognised without a model."
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <>
              <Switch
                checked={!!settings?.mail_sorting}
                disabled={!settings}
                onCheckedChange={toggle}
                aria-label="Sort new mail in the background"
              />
            </>
          }
        />
        {settings?.mail_sorting && (
          <Row
            icon={
              <IconTile size="sm" className="bg-fill text-muted-foreground">
                <Sparkles />
              </IconTile>
            }
            title="Sorted by"
            detail={
              sorter === "jev"
                ? "Jev by TypeSafe, in the cloud: the sender, subject and text of new mail are sent to TypeSafe to be sorted (newsletters too when you have smart folders). Suspicious mail never is. No summaries."
                : "Your model. Or choose Jev, a fast cloud service that only sorts (your own TypeSafe key)."
            }
            className="[&_.truncate]:whitespace-normal"
            trailing={
              <div className="flex items-center gap-2">
                <div role="radiogroup" aria-label="Sorted by" className="flex rounded-[9px] bg-fill p-[2px]">
                  {(["model", "jev"] as const).map((s) => (
                    <button
                      key={s}
                      role="radio"
                      aria-checked={sorter === s}
                      onClick={() => choose(s)}
                      className={cn(
                        "h-7 rounded-[7px] px-3 text-[13px] font-medium whitespace-nowrap transition-colors",
                        sorter === s
                          ? "bg-background text-foreground shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {s === "model" ? "Your model" : "Jev"}
                    </button>
                  ))}
                </div>
                {sorter === "jev" ? (
                  <LocalityBadge locality="cloud" />
                ) : (
                  model && <LocalityBadge locality={model.locality} />
                )}
              </div>
            }
          />
        )}
        {overview?.jev_connected && (
          <Row
            icon={
              <IconTile size="sm" className="bg-cloud-soft text-cloud">
                <Lock />
              </IconTile>
            }
            title="TypeSafe key saved"
            detail="Stored encrypted on this computer, and only sent to TypeSafe."
            trailing={
              <Button variant="secondary" size="sm" onClick={removeKey}>
                Remove
              </Button>
            }
          />
        )}
      </Grouped>
      <JevKeyDialog
        open={askingKey}
        onClose={() => setAskingKey(false)}
        onSaved={() => {
          setAskingKey(false);
          if (settings)
            api
              .putSettings({ ...settings, mail_sorter: "jev" })
              .then(() => toast.success("Jev now sorts your new mail"))
              .catch((e) => toast.error((e as Error).message));
        }}
      />
    </Section>
  );
}

/** Asks for the user's TypeSafe key, checked with TypeSafe before it's saved. */
function JevKeyDialog({ open, onClose, onSaved }: { open: boolean; onClose: () => void; onSaved: () => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.jevConnect(key);
      setKey("");
      onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[460px]">
        <DialogHeader>
          <DialogTitle>Sort mail with Jev</DialogTitle>
          <DialogDescription>
            Jev is a cloud service by TypeSafe that only sorts: it answers in a fraction of a second but
            writes nothing, so there are no summaries. Each new email it sorts or files into smart
            folders (sender, subject and text) is sent to TypeSafe. Newsletters are only sent when you
            have smart folders, and suspicious mail never is.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (key.trim()) void save();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <label htmlFor="jev-key" className="type-subhead font-medium">
              Your TypeSafe API key
            </label>
            <Input
              id="jev-key"
              type="password"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              autoComplete="off"
              autoFocus
            />
            <p className="type-subhead text-muted-foreground">
              TypeSafe charges for use.{" "}
              <button
                type="button"
                className="font-medium text-foreground underline underline-offset-2"
                onClick={() => openExternal("https://docs.typesafe.ai/introduction/quickstart")}
              >
                Get a key
              </button>
            </p>
          </div>
          {error && <p className="type-subhead text-destructive">{error}</p>}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !key.trim()}>
              {busy && <Loader2 className="animate-spin" />} {busy ? "Checking…" : "Use Jev"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
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
          ? "Starts when you log in, so your messaging apps and reminders work with this window closed."
          : "Starts when you log in and keeps going with this window closed, so your messaging apps and reminders always work."
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
