import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  ArrowLeft,
  ArrowRight,
  CalendarDays,
  Check,
  Cloud,
  Cpu,
  Download,
  Eye,
  Loader2,
  Lock,
  Mail,
  Radar,
  Send,
  Sparkles,
  WifiOff,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { CatalogModel } from "@/bindings/CatalogModel";
import type { ModelRef } from "@/bindings/ModelRef";
import type { Settings } from "@/bindings/Settings";
import { AssistantAvatar, type AvatarMood } from "@/components/assistant-avatar";
import { LogoMark } from "@/components/brand";
import { IconTile, Pill } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Textarea } from "@/components/ui/textarea";
import { type ConnectKind, ConnectDialog } from "@/features/connections/connect-dialogs";
import { AddSourceDialog } from "@/features/models/source-dialogs";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { macOverlayTitleBar } from "@/lib/platform";
import {
  useConnections,
  useModelInfo,
  useProviders,
  usePulls,
  useRecommendations,
  useAssistantName,
  useSettings,
} from "@/lib/queries";
import { openExternal } from "@/lib/transport";

/** The steps, in order. The index is saved in settings so closing midway resumes. */
const STEPS = ["welcome", "model", "connect", "about", "finish"] as const;

/** First run: welcome, pick a model for this computer, optional connections, a few words
 * about the user, then the model finishes downloading and chat opens. */
export function Onboarding() {
  const settings = useSettings().data;
  const [direction, setDirection] = useState(1);
  if (!settings) return null;
  const index = Math.min(settings.onboarding_step, STEPS.length - 1);
  const step = STEPS[index];

  const go = async (to: number, patch: Partial<Settings> = {}) => {
    setDirection(to >= index ? 1 : -1);
    try {
      await api.putSettings({ ...settings, ...patch, onboarding_step: to });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  const next = (patch?: Partial<Settings>) => go(index + 1, patch);
  const back = () => go(Math.max(0, index - 1));

  return (
    <div className="flex h-screen flex-col overflow-y-auto">
      <header
        data-tauri-drag-region
        className={cn("flex h-[60px] shrink-0 items-center justify-between px-5", macOverlayTitleBar && "pl-[84px]")}
      >
        <div className="flex items-center gap-2">
          <LogoMark />
          <span className="text-[15px] font-semibold tracking-[-0.016em]">Mimi</span>
        </div>
        {index > 0 && (
          <div className="flex gap-1.5" aria-label={`Step ${index + 1} of ${STEPS.length}`}>
            {STEPS.map((s, i) => (
              <span
                key={s}
                className={cn(
                  "h-1.5 rounded-full transition-all duration-300",
                  i === index ? "w-5 bg-foreground" : i < index ? "w-1.5 bg-foreground/40" : "w-1.5 bg-fill",
                )}
              />
            ))}
          </div>
        )}
      </header>
      <main className="flex flex-1 justify-center px-6 pt-4 pb-16">
        <div className="w-full max-w-[640px]">
          <AnimatePresence mode="wait" initial={false} custom={direction}>
            <motion.div
              key={step}
              custom={direction}
              initial={{ opacity: 0, x: 24 * direction }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -24 * direction }}
              transition={{ type: "spring", stiffness: 420, damping: 36 }}
            >
              {step === "welcome" && <Welcome onNext={() => next()} />}
              {step === "model" && <ChooseModel onNext={next} onBack={back} />}
              {step === "connect" && <Connect onNext={() => next()} onBack={back} />}
              {step === "about" && <AboutYou onNext={() => next()} onBack={back} />}
              {step === "finish" && (
                <Finish
                  onDone={() => go(index, { onboarding_done: true, onboarding_step: 0 })}
                  onChooseAgain={() => go(STEPS.indexOf("model"), { pending_model: null })}
                />
              )}
            </motion.div>
          </AnimatePresence>
        </div>
      </main>
    </div>
  );
}

function StepTitle({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div className="flex flex-col gap-2">
      <h1 className="type-large-title">{title}</h1>
      {subtitle && <p className="max-w-[540px] type-body text-muted-foreground">{subtitle}</p>}
    </div>
  );
}

function Footer({
  onBack,
  children,
}: {
  onBack?: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="mt-10 flex items-center justify-between gap-3">
      {onBack ? (
        <Button variant="ghost" onClick={onBack} className="text-muted-foreground">
          <ArrowLeft /> Back
        </Button>
      ) : (
        <span />
      )}
      <div className="flex items-center gap-2">{children}</div>
    </div>
  );
}

// --- 1. Welcome --------------------------------------------------------------------

function Welcome({ onNext }: { onNext: () => void }) {
  const promises = [
    { icon: Lock, text: "Your conversations stay on this computer." },
    { icon: WifiOff, text: "Nothing is sent anywhere unless a task needs it, and you'll always see when." },
    { icon: Eye, text: "You can see, change or delete everything I remember." },
  ];
  // A happy hello on arrival, then it settles into its calm idle.
  const name = useAssistantName();
  const [mood, setMood] = useState<AvatarMood>("happy");
  useEffect(() => {
    const timer = setTimeout(() => setMood("idle"), 2400);
    return () => clearTimeout(timer);
  }, []);
  return (
    <div className="flex flex-col items-center pt-[8vh] text-center">
      <motion.div
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 20 }}
      >
        <AssistantAvatar size={112} mood={mood} />
      </motion.div>
      <h1 className="mt-6 text-[34px] leading-[40px] font-semibold tracking-[-0.03em]">Hi, I'm {name}.</h1>
      <p className="mt-2 max-w-[460px] type-body text-muted-foreground">
        Your personal assistant. I live on this computer, not in someone else's cloud.
      </p>
      <div className="mt-9 flex w-full max-w-[460px] flex-col gap-3 text-left">
        {promises.map((p, i) => (
          <motion.div
            key={p.text}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.15 + i * 0.08, type: "spring", stiffness: 400, damping: 34 }}
            className="flex items-start gap-3"
          >
            <IconTile size="sm" className="bg-private-soft text-private">
              <p.icon />
            </IconTile>
            <span className="pt-0.5 type-callout">{p.text}</span>
          </motion.div>
        ))}
      </div>
      <Button size="lg" variant="lime" className="mt-10 rounded-full px-8" onClick={onNext} autoFocus>
        Get started <ArrowRight />
      </Button>
    </div>
  );
}

// --- 2. The model ------------------------------------------------------------------

type Choice =
  | { kind: "download"; model: CatalogModel }
  | { kind: "installed"; ref: ModelRef }
  | { kind: "cloud" };

function ChooseModel({ onNext, onBack }: { onNext: (patch?: Partial<Settings>) => void; onBack: () => void }) {
  const rec = useRecommendations();
  const settings = useSettings().data;
  const info = useModelInfo();
  const [choice, setChoice] = useState<Choice | null>(null);
  const [addingCloud, setAddingCloud] = useState(false);
  const [busy, setBusy] = useState(false);
  const r = rec.data;

  const providers = useProviders().data ?? [];
  const [known, setKnown] = useState<string[]>([]);

  // When the cloud dialog closes with a new cloud service added, use its first model.
  const cloudDialogChanged = async (open: boolean) => {
    setAddingCloud(open);
    if (open) {
      setKnown(providers.map((p) => p.id));
      return;
    }
    const added = (await api.providers()).find((p) => p.locality === "cloud" && !known.includes(p.id));
    if (!added) return;
    try {
      const models = await api.models(added.id);
      const current = await api.settings();
      const default_model =
        current.default_model?.provider_id === added.id || models.length === 0
          ? current.default_model
          : { provider_id: added.id, model: models[0].id };
      await onNext({ default_model, pending_model: null });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  // Start with the best option selected, so Continue works right away.
  useEffect(() => {
    if (!r || choice) return;
    const firstInstalled = r.installed.find((m) => m.fits);
    if (firstInstalled) setChoice({ kind: "installed", ref: firstInstalled.model });
    else if (r.prefer_cloud || !r.download_provider_id || r.suggested.length === 0) setChoice({ kind: "cloud" });
    else setChoice({ kind: "download", model: quickStart(r.suggested) });
  }, [r, choice]);

  if (!r) {
    return (
      <div className="flex justify-center pt-[20vh]">
        <Loader2 className="size-5 animate-spin text-faint" />
      </div>
    );
  }
  const installed = r.installed.filter((m) => m.fits);
  // Quick to download first; the biggest model that fits is offered as an upgrade.
  const quick = r.suggested.length > 0 ? quickStart(r.suggested) : null;
  const downloads = quick ? [quick, ...r.suggested.filter((m) => m.id !== quick.id)] : [];
  const best = r.suggested[0];
  const server = r.detected_servers.find((s) => !s.already_added);
  const canDownload = r.download_provider_id !== null;
  const gpu = r.hardware.gpus.find((g) => g.kind === "discrete") ?? r.hardware.gpus[0];

  const confirm = async () => {
    if (!choice || !settings) return;
    if (choice.kind === "cloud") {
      cloudDialogChanged(true);
      return;
    }
    setBusy(true);
    try {
      if (choice.kind === "installed") {
        await onNext({ default_model: choice.ref, pending_model: null });
      } else if (r.download_provider_id) {
        const ref = { provider_id: r.download_provider_id, model: choice.model.id };
        await api.pull(r.download_provider_id, choice.model.id);
        // Becomes the default when the download finishes, even if Mimi is closed meanwhile.
        await onNext({ pending_model: ref });
      }
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <StepTitle
        title="How I'll think"
        subtitle="I need a model: the brain that writes my answers. The best choice depends on this computer."
      />

      <div className="surface flex items-center gap-4 px-5 py-4">
        <IconTile className="bg-fill text-muted-foreground">
          <Cpu />
        </IconTile>
        <div className="min-w-0 flex-1">
          <p className="type-body font-medium">{r.summary}</p>
          <p className="type-subhead text-muted-foreground">
            {Math.round(r.hardware.total_memory_bytes / 2 ** 30)} GB memory{gpu ? ` · ${gpu.name}` : ""}
          </p>
        </div>
      </div>

      {server && (
        <DetectedServer name={server.name} count={server.model_count} baseUrl={server.base_url} />
      )}

      <div className="flex flex-col gap-2.5" role="radiogroup" aria-label="Choose a model">
        {installed.map((m) => {
          const selected =
            choice?.kind === "installed" &&
            choice.ref.model === m.model.model &&
            choice.ref.provider_id === m.model.provider_id;
          const { name, description } = info(m.model.model);
          return (
            <Option
              key={`${m.model.provider_id}/${m.model.model}`}
              selected={selected}
              onSelect={() => setChoice({ kind: "installed", ref: m.model })}
              icon={<Check />}
              tone="bg-private-soft text-private"
              title={name}
              badge={<Pill className="bg-private-soft text-private">Already on this computer</Pill>}
              detail={description ?? `From ${m.provider_name}`}
            />
          );
        })}
        {r.prefer_cloud && (
          <CloudOption selected={choice?.kind === "cloud"} onSelect={() => setChoice({ kind: "cloud" })} recommended />
        )}
        {canDownload &&
          downloads.map((m) => (
            <Option
              key={m.id}
              selected={choice?.kind === "download" && choice.model.id === m.id}
              onSelect={() => setChoice({ kind: "download", model: m })}
              icon={<Download />}
              tone="bg-lime-soft text-lime-deep"
              title={m.name}
              badge={
                m.id === quick?.id && !r.prefer_cloud && installed.length === 0 ? (
                  <Pill className="bg-lime-soft text-lime-deep">Recommended</Pill>
                ) : m.id === best?.id && best.id !== quick?.id ? (
                  <Pill className="bg-fill text-muted-foreground">Best quality</Pill>
                ) : undefined
              }
              detail={`${m.description} ${formatBytes(m.download_bytes)} download, private.`}
            />
          ))}
        {!canDownload && installed.length === 0 && <NoLocalRuntime />}
        {!r.prefer_cloud && (
          <CloudOption selected={choice?.kind === "cloud"} onSelect={() => setChoice({ kind: "cloud" })} />
        )}
      </div>

      <Footer onBack={onBack}>
        <Button size="lg" onClick={confirm} disabled={!choice || busy} className="rounded-full px-6">
          {busy && <Loader2 className="animate-spin" />}
          {choice?.kind === "download" ? "Download and continue" : "Continue"}
          {!busy && <ArrowRight />}
        </Button>
      </Footer>
      <AddSourceDialog open={addingCloud} onOpenChange={cloudDialogChanged} only="cloud" />
    </div>
  );
}

function Option({
  selected,
  onSelect,
  icon,
  tone,
  title,
  badge,
  detail,
}: {
  selected: boolean;
  onSelect: () => void;
  icon: React.ReactNode;
  tone: string;
  title: string;
  badge?: React.ReactNode;
  detail: React.ReactNode;
}) {
  return (
    <button
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={cn(
        "surface pressable flex items-center gap-4 px-5 py-4 text-left",
        selected && "shadow-[0_0_0_2px_var(--foreground),var(--shadow-card)]!",
      )}
    >
      <IconTile className={tone}>{icon}</IconTile>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="type-body font-medium">{title}</span>
          {badge}
        </div>
        <p className="type-subhead text-muted-foreground">{detail}</p>
      </div>
      <span
        className={cn(
          "flex size-5 shrink-0 items-center justify-center rounded-full transition-colors",
          selected ? "bg-foreground text-background" : "shadow-[inset_0_0_0_1.5px_rgb(0_0_0/0.18)]",
        )}
      >
        {selected && <Check className="size-3" strokeWidth={3} />}
      </span>
    </button>
  );
}

function CloudOption({
  selected,
  onSelect,
  recommended,
}: {
  selected: boolean;
  onSelect: () => void;
  recommended?: boolean;
}) {
  return (
    <Option
      selected={selected}
      onSelect={onSelect}
      icon={<Cloud />}
      tone="bg-cloud-soft text-cloud"
      title="A cloud service"
      badge={recommended ? <Pill className="bg-cloud-soft text-cloud">Recommended here</Pill> : undefined}
      detail="Faster and smarter, but your messages are sent to the service you pick. You'll need an account with them."
    />
  );
}

function DetectedServer({ name, count, baseUrl }: { name: string; count: number; baseUrl: string }) {
  const [busy, setBusy] = useState(false);
  return (
    <div className="surface flex items-center gap-4 px-5 py-4">
      <IconTile className="bg-private-soft text-private">
        <Radar />
      </IconTile>
      <div className="min-w-0 flex-1">
        <p className="type-body font-medium">{name} is on this computer</p>
        <p className="type-subhead text-muted-foreground">
          It has {count} model{count === 1 ? "" : "s"}. Connect it to use {count === 1 ? "it" : "them"} privately.
        </p>
      </div>
      <Button
        variant="secondary"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          try {
            await api.addProvider({ name, kind: "openai_compatible", base_url: baseUrl, api_key: null, locality: null });
          } catch (e) {
            toast.error((e as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy && <Loader2 className="animate-spin" />} Connect
      </Button>
    </div>
  );
}

/** Development builds without the bundled runtime: point at Ollama instead. */
function NoLocalRuntime() {
  return (
    <div className="surface flex items-center gap-4 px-5 py-4">
      <IconTile className="bg-fill text-muted-foreground">
        <Sparkles />
      </IconTile>
      <div className="min-w-0 flex-1">
        <p className="type-body font-medium">Private models need a model runner</p>
        <p className="type-subhead text-muted-foreground">
          This copy of Mimi doesn't include one. Install Ollama and Mimi finds it on its own.
        </p>
      </div>
      <Button variant="secondary" onClick={() => openExternal("https://ollama.com/download")}>
        Get Ollama
      </Button>
    </div>
  );
}

// --- 3. Connections ----------------------------------------------------------------

function Connect({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const connections = useConnections().data ?? [];
  const [open, setOpen] = useState<ConnectKind | null>(null);
  const has = (ids: string[]) => connections.some((c) => ids.includes(c.integration));
  const items: { kinds: ConnectKind[]; icon: typeof Send; tone: string; title: string; detail: string }[] = [
    {
      kinds: ["google_calendar", "caldav"],
      icon: CalendarDays,
      tone: "bg-event-soft text-event",
      title: "Your calendar",
      detail: "So I know your plans and can add events when you ask. Google, iCloud and others.",
    },
    {
      kinds: ["email"],
      icon: Mail,
      tone: "bg-[#efe9fb] text-[#6146ad]",
      title: "Your email",
      detail: "So I can sort what needs a reply and draft answers. Nothing is sent without your OK.",
    },
    {
      kinds: ["telegram"],
      icon: Send,
      tone: "bg-[#def3f7] text-[#136c86]",
      title: "Telegram",
      detail: "Chat with me from your phone, through a bot only you can use.",
    },
  ];
  const [calendarChoice, setCalendarChoice] = useState(false);

  return (
    <div className="flex flex-col gap-8">
      <StepTitle
        title="Connect your apps"
        subtitle="Optional. I only see what you connect, and I always ask before I send or change anything."
      />
      <div className="flex flex-col gap-2.5">
        {items.map((it) => {
          const done = has(it.kinds);
          const choosing = calendarChoice && it.kinds.length > 1 && !done;
          return (
            <div key={it.title} className="surface flex flex-col px-5 py-4">
              <div className="flex items-center gap-4">
                <IconTile className={it.tone}>
                  <it.icon />
                </IconTile>
                <div className="min-w-0 flex-1">
                  <p className="type-body font-medium">{it.title}</p>
                  <p className="type-subhead text-muted-foreground">{it.detail}</p>
                </div>
                {done ? (
                  <span className="flex items-center gap-1.5 type-subhead font-medium text-private">
                    <Check className="size-4" /> Connected
                  </span>
                ) : it.kinds.length > 1 ? (
                  <Button variant="secondary" onClick={() => setCalendarChoice((c) => !c)}>
                    Connect
                  </Button>
                ) : (
                  <Button variant="secondary" onClick={() => setOpen(it.kinds[0])}>
                    Connect
                  </Button>
                )}
              </div>
              <AnimatePresence initial={false}>
                {choosing && (
                  <motion.div
                    initial={{ opacity: 0, height: 0 }}
                    animate={{ opacity: 1, height: "auto" }}
                    exit={{ opacity: 0, height: 0 }}
                    transition={{ type: "spring", stiffness: 420, damping: 36 }}
                    className="overflow-hidden"
                  >
                    <div className="flex flex-wrap gap-2 pt-3 pl-[52px]">
                      <Button variant="secondary" size="sm" onClick={() => setOpen("google_calendar")} autoFocus>
                        Google Calendar
                      </Button>
                      <Button variant="secondary" size="sm" onClick={() => setOpen("caldav")}>
                        iCloud, Fastmail, Nextcloud…
                      </Button>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          );
        })}
      </div>
      <Footer onBack={onBack}>
        <Button size="lg" onClick={onNext} className="rounded-full px-6">
          {connections.length > 0 ? "Continue" : "Skip for now"} <ArrowRight />
        </Button>
      </Footer>
      <ConnectDialog kind={open} onClose={() => setOpen(null)} />
    </div>
  );
}

// --- 4. About you ------------------------------------------------------------------

function AboutYou({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const [name, setName] = useState("");
  const [place, setPlace] = useState("");
  const [more, setMore] = useState("");
  const [busy, setBusy] = useState(false);
  const anything = name.trim() || place.trim() || more.trim();

  const save = async () => {
    if (!anything) return onNext();
    setBusy(true);
    try {
      const lines = [
        name.trim() && `Name: ${name.trim()}.`,
        place.trim() && `Lives in ${place.trim()}.`,
        more.trim(),
      ].filter(Boolean);
      const existing = (await api.memory()).profile.trim();
      await api.saveMemoryProfile([existing, lines.join("\n")].filter(Boolean).join("\n"));
      onNext();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="flex flex-col gap-8"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <StepTitle
        title="A little about you"
        subtitle="Optional. It helps me be useful from the first message. It stays on this computer, and you can change it any time in Memory."
      />
      <div className="surface flex flex-col gap-5 p-6">
        <div className="grid grid-cols-2 gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ob-name">What should I call you?</Label>
            <Input id="ob-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus placeholder="Your first name" />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ob-place">Where do you live?</Label>
            <Input id="ob-place" value={place} onChange={(e) => setPlace(e.target.value)} placeholder="City" />
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="ob-more">Anything I should know?</Label>
          <Textarea
            id="ob-more"
            value={more}
            onChange={(e) => setMore(e.target.value)}
            rows={4}
            placeholder="Who's in your family, what you do, what you like… a few lines is plenty."
          />
        </div>
      </div>
      <Footer onBack={onBack}>
        <Button type="submit" size="lg" disabled={busy} className="rounded-full px-6">
          {busy && <Loader2 className="animate-spin" />}
          {anything ? "Continue" : "Skip for now"} {!busy && <ArrowRight />}
        </Button>
      </Footer>
    </form>
  );
}

// --- 5. Finish ---------------------------------------------------------------------

function Finish({ onDone, onChooseAgain }: { onDone: () => void; onChooseAgain: () => void }) {
  const settings = useSettings().data;
  const pullsQuery = usePulls();
  const pulls = pullsQuery.data ?? [];
  const providers = useProviders().data ?? [];
  const ready = !!settings?.default_model;
  const pending = settings?.pending_model ?? null;
  const pull = pending ? pulls.find((p) => p.provider_id === pending.provider_id && p.model === pending.model) : undefined;
  // Stopped, failed, or no longer running (e.g. Mimi restarted midway): offer to resume.
  const failed = pull ? pull.state === "failed" || pull.state === "cancelled" : !!pending && pullsQuery.isSuccess;
  const pct =
    pull?.total_bytes && pull.completed_bytes != null ? Math.round((pull.completed_bytes / pull.total_bytes) * 100) : null;
  const cloud = providers.find((p) => p.id === settings?.default_model?.provider_id)?.locality === "cloud";

  const chooseAgain = () => {
    if (pending && pull?.state === "running" && providers.find((p) => p.id === pending.provider_id)?.kind === "builtin")
      api.cancelPull(pending.provider_id, pending.model).catch(() => {});
    onChooseAgain();
  };

  useEffect(() => {
    if (!ready) return;
    const onKey = (e: KeyboardEvent) => e.key === "Enter" && onDone();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ready, onDone]);

  return (
    <div className="flex flex-col items-center pt-[8vh] text-center">
      <motion.div
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 20 }}
      >
        <IconTile size="lg" className={ready ? "bg-lime text-lime-ink" : "bg-fill text-muted-foreground"}>
          {ready ? <Check /> : <Download />}
        </IconTile>
      </motion.div>
      <h1 className="mt-6 type-large-title">{ready ? "You're all set" : "Almost there"}</h1>
      <p className="mt-2 max-w-[440px] type-body text-muted-foreground">
        {ready
          ? cloud
            ? "Your messages will go to the cloud service you chose. You can switch to a private model any time in Models."
            : "Everything runs on this computer. Ask me anything."
          : failed
            ? "The download stopped before it finished. It continues where it left off."
            : "I'm downloading the model I'll think with. You can leave this window open, or close Mimi and come back later."}
      </p>
      {!ready && pull && pull.state === "running" && (
        <div className="mt-8 flex w-full max-w-[380px] flex-col gap-2">
          <Progress value={pct ?? 0} />
          <span className="type-footnote text-muted-foreground">
            {pull.status}
            {pct != null && ` · ${pct}%`}
          </span>
        </div>
      )}
      {!ready && failed && pull?.error && <p className="mt-4 type-subhead text-destructive">{pull.error}</p>}
      <div className="mt-10 flex gap-2">
        {ready ? (
          <Button size="lg" variant="lime" className="rounded-full px-8" onClick={onDone} autoFocus>
            Start chatting <ArrowRight />
          </Button>
        ) : failed && pending ? (
          <>
            <Button variant="ghost" onClick={chooseAgain}>
              Choose another model
            </Button>
            <Button
              size="lg"
              className="rounded-full px-6"
              onClick={() => api.pull(pending.provider_id, pending.model).catch((e) => toast.error(e.message))}
            >
              Continue download
            </Button>
          </>
        ) : (
          <Button variant="ghost" onClick={chooseAgain}>
            Choose another model
          </Button>
        )}
      </div>
    </div>
  );
}

/** Largest download that still sets up quickly on a first run. */
const QUICK_START_BYTES = 6e9;

/**
 * The model to start with: the best one that downloads quickly (a first run shouldn't
 * wait on a 10+ GB download), or the smallest if none is that small.
 */
function quickStart(models: CatalogModel[]): CatalogModel {
  const small = models.filter((m) => m.download_bytes <= QUICK_START_BYTES);
  const pool = small.length > 0 ? small : models;
  return pool.reduce((a, b) =>
    small.length > 0
      ? b.download_bytes > a.download_bytes ? b : a
      : b.download_bytes < a.download_bytes ? b : a,
  );
}
