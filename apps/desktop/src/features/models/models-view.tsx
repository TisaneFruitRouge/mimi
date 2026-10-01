import { forwardRef, useRef, useState } from "react";
import { motion } from "motion/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowRight,
  Check,
  ChevronRight,
  Cloud,
  Cpu,
  Download,
  ImageIcon,
  Loader2,
  MoreHorizontal,
  Plus,
  Radar,
  Search,
  Sparkles,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { CatalogModel } from "@/bindings/CatalogModel";
import type { HardwareTier } from "@/bindings/HardwareTier";
import type { ModelRef } from "@/bindings/ModelRef";
import type { Provider } from "@/bindings/Provider";
import { LocalityBadge } from "@/components/locality-badge";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { ModelPickerDialog, costLabel } from "@/features/models/model-list";
import { AddSourceDialog, EditSourceDialog } from "@/features/models/source-dialogs";
import { SourceMark, presetOf } from "@/features/models/source-mark";
import { api, keys } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import {
  type ModelOption,
  sameModel,
  useActiveModel,
  useAllModels,
  useDefaultSeesImages,
  useModelInfo,
  useProviders,
  usePulls,
  useRecommendations,
  useRuntime,
  useSettings,
} from "@/lib/queries";
import { openExternal } from "@/lib/transport";

const tierLine: Record<HardwareTier, string> = {
  minimal: "Best with cloud models. Local ones will be slow here.",
  light: "Runs small models well.",
  standard: "Runs mid-sized models comfortably.",
  strong: "Runs large models comfortably.",
  workstation: "Runs very large models.",
};

/** Choosing and managing models. Doubles as first-run setup until a model is chosen. */
export function ModelsView({ setup = false, onChat }: { setup?: boolean; onChat?: () => void }) {
  const [adding, setAdding] = useState<null | "any" | "cloud">(null);
  // The model picker, open on one source's models or (null) all of them.
  const [browsing, setBrowsing] = useState<{ source: string | null } | null>(null);
  const [choosingPhotoModel, setChoosingPhotoModel] = useState(false);
  const rec = useRecommendations().data;
  const yourModels = useRef<HTMLElement>(null);
  const providers = useProviders().data ?? [];
  const presets = useQuery({ queryKey: keys.presets, queryFn: api.presets }).data ?? [];
  const recommended = (ref: ModelRef) => {
    const provider = providers.find((p) => p.id === ref.provider_id);
    return !!provider && presetOf(provider, presets)?.recommended_model === ref.model;
  };

  return (
    <Page>
      <PageHeader
        title={setup ? "Welcome to Mimi" : "Models"}
        subtitle={
          setup
            ? "First, choose how your assistant thinks. Models on this computer keep everything private."
            : "What your assistant thinks with, and where it runs."
        }
      />

      <div className="grid grid-cols-3 gap-3">
        {setup ? (
          <SetupCard onAdd={() => setAdding("any")} />
        ) : (
          <ActiveModelCard
            onChange={() => setBrowsing({ source: null })}
            onChat={onChat}
          />
        )}
        <ComputerCard />
      </div>

      {!setup && <DetectedServers />}
      <YourModels ref={yourModels} onBrowse={(source) => setBrowsing({ source })} />
      {!setup && <PhotoModel onChoose={() => setChoosingPhotoModel(true)} />}

      <Section title="Good fits for this computer">
        <div className="grid grid-cols-3 gap-3">
          {rec?.prefer_cloud && <CloudCard recommended onAdd={() => setAdding("cloud")} />}
          {rec?.suggested.map((m, i) => (
            <SuggestedCard
              key={m.id}
              model={m}
              best={i === 0 && !rec.prefer_cloud}
              providerId={rec.download_provider_id}
            />
          ))}
          {!rec && [0, 1, 2].map((i) => <Skeleton key={i} className="h-[196px] rounded-[18px]" />)}
          {rec && !rec.prefer_cloud && <CloudCard onAdd={() => setAdding("cloud")} />}
        </div>
      </Section>

      <Sources onAdd={() => setAdding("any")} />

      <ModelPickerDialog
        open={browsing !== null}
        onOpenChange={(o) => !o && setBrowsing(null)}
        initialSource={browsing?.source ?? null}
        recommended={recommended}
      />
      <ModelPickerDialog open={choosingPhotoModel} onOpenChange={setChoosingPhotoModel} forPhotos />
      <AddSourceDialog
        open={adding !== null}
        onOpenChange={(o) => !o && setAdding(null)}
        only={adding === "cloud" ? "cloud" : undefined}
      />
    </Page>
  );
}

function Card({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("surface p-5", className)}>{children}</div>;
}

/** First run: the one thing to do next, front and centre. */
function SetupCard({ onAdd }: { onAdd: () => void }) {
  const rec = useRecommendations().data;
  const server = rec?.detected_servers.find((s) => !s.already_added);
  const [busy, setBusy] = useState(false);
  return (
    <Card className="col-span-2 flex flex-col gap-5 p-6">
      <IconTile className="bg-lime-soft text-lime-deep">
        {server ? <Radar /> : <Sparkles />}
      </IconTile>
      {server ? (
        <>
          <div className="flex flex-col gap-1.5">
            <span className="type-title">{server.name} is already on this computer</span>
            <span className="type-callout text-muted-foreground">
              It has {server.model_count} model{server.model_count === 1 ? "" : "s"} ready. Connect it and
              everything stays private.
            </span>
          </div>
          <div className="mt-auto flex gap-2">
            <Button
              size="lg"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await api.addProvider({
                    name: server.name,
                    kind: "openai_compatible",
                    base_url: server.base_url,
                    api_key: null,
                    locality: null,
                  });
                } catch (e) {
                  toast.error((e as Error).message);
                } finally {
                  setBusy(false);
                }
              }}
            >
              {busy && <Loader2 className="animate-spin" />} Connect {server.name}
            </Button>
            <Button size="lg" variant="ghost" onClick={onAdd}>
              Use something else
            </Button>
          </div>
        </>
      ) : (
        <>
          <div className="flex flex-col gap-1.5">
            <span className="type-title">Choose a model below</span>
            <span className="type-callout text-muted-foreground">
              Download one that fits this computer, or connect a cloud service or a server you own.
            </span>
          </div>
          <div className="mt-auto">
            <Button size="lg" variant="secondary" onClick={onAdd}>
              <Plus /> Connect a model source
            </Button>
          </div>
        </>
      )}
    </Card>
  );
}

function ActiveModelCard({ onChange, onChat }: { onChange: () => void; onChat?: () => void }) {
  const active = useActiveModel();
  return (
    <Card className="col-span-2 flex flex-col gap-5 p-6">
      <div className="flex items-start justify-between">
        <IconTile className="bg-lime-soft text-lime-deep">
          <Sparkles />
        </IconTile>
        {active && <LocalityBadge locality={active.provider.locality} />}
      </div>
      {active ? (
        <div className="flex flex-col gap-1.5">
          <span className="type-footnote font-medium text-muted-foreground">Your assistant uses</span>
          <span className="text-[28px] leading-[34px] font-semibold tracking-[-0.026em]">{active.name}</span>
          <span className="type-callout text-muted-foreground">
            {active.description ?? "Your assistant's current model."}
          </span>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          <span className="type-title text-muted-foreground">No model yet</span>
          <span className="type-callout text-muted-foreground">
            Pick one of the models below, or connect a model source.
          </span>
        </div>
      )}
      <div className="mt-auto flex gap-2">
        <Button variant="secondary" onClick={onChange}>
          Change model
        </Button>
        {onChat && active && (
          <Button variant="ghost" onClick={onChat}>
            Start chatting <ArrowRight />
          </Button>
        )}
      </div>
    </Card>
  );
}

function ComputerCard() {
  const rec = useRecommendations().data;
  const active = useActiveModel();
  const { options } = useAllModels();
  if (!rec) return <Skeleton className="h-full min-h-[220px] rounded-[18px]" />;
  const hw = rec.hardware;
  const gpu = hw.gpus.find((g) => g.kind === "discrete") ?? hw.gpus[0];
  const activeSize =
    active?.provider.locality === "device"
      ? options.find((o) => sameModel(o.ref, active.ref))?.sizeBytes
      : null;
  const budgetGb = rec.model_budget_bytes / 1e9;
  const usedPct = activeSize ? Math.min(100, (activeSize / rec.model_budget_bytes) * 100) : 0;

  return (
    <Card className="flex flex-col gap-4">
      <IconTile className="bg-fill text-muted-foreground">
        <Cpu />
      </IconTile>
      <div className="flex flex-col gap-1">
        <span className="type-footnote font-medium text-muted-foreground">This computer</span>
        <p className="type-headline">{tierLine[rec.tier]}</p>
      </div>
      <div className="flex flex-col gap-2">
        <div className="h-1.5 overflow-hidden rounded-full bg-fill">
          <motion.div
            className="h-full rounded-full bg-lime"
            initial={{ width: 0 }}
            animate={{ width: `${usedPct}%` }}
            transition={{ type: "spring", stiffness: 120, damping: 24, delay: 0.1 }}
          />
        </div>
        <div className="type-footnote text-muted-foreground">
          {activeSize
            ? `Your model uses about ${Math.round(usedPct)}% of the room available.`
            : `Room for models up to about ${Math.round(budgetGb)} GB.`}
        </div>
      </div>
      <div className="mt-auto type-footnote text-faint">
        {Math.round(hw.total_memory_bytes / 2 ** 30)} GB memory{gpu ? ` · ${gpu.name}` : ""}
      </div>
    </Card>
  );
}

function DetectedServers() {
  const rec = useRecommendations().data;
  const [busy, setBusy] = useState<string | null>(null);
  const servers = rec?.detected_servers.filter((s) => !s.already_added) ?? [];
  if (servers.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {servers.map((s) => (
        <div key={s.preset_id} className="surface flex items-center gap-4 px-5 py-4">
          <IconTile className="bg-private-soft text-private">
            <Radar />
          </IconTile>
          <div className="flex-1">
            <p className="type-body font-medium">{s.name} is running on this computer</p>
            <p className="type-subhead text-muted-foreground">
              {s.model_count} model{s.model_count === 1 ? "" : "s"} ready. Connect it to keep everything
              private.
            </p>
          </div>
          <Button
            disabled={busy !== null}
            className="rounded-full px-4"
            onClick={async () => {
              setBusy(s.preset_id);
              try {
                await api.addProvider({
                  name: s.name,
                  kind: "openai_compatible",
                  base_url: s.base_url,
                  api_key: null,
                  locality: null,
                });
              } catch (e) {
                toast.error((e as Error).message);
              } finally {
                setBusy(null);
              }
            }}
          >
            {busy === s.preset_id && <Loader2 className="animate-spin" />} Connect
          </Button>
        </div>
      ))}
    </div>
  );
}

/**
 * The model in use, the models on the user's own machines, and a way into each cloud
 * service's (often long) list through the model picker.
 */
const YourModels = forwardRef<HTMLElement, { onBrowse: (source: string | null) => void }>(function YourModels(
  { onBrowse },
  ref,
) {
  const { options, loading } = useAllModels();
  const settings = useSettings().data;
  const rec = useRecommendations().data;
  const providers = useProviders().data ?? [];
  const presets = useQuery({ queryKey: keys.presets, queryFn: api.presets }).data ?? [];
  const info = useModelInfo();
  const [removing, setRemoving] = useState<ModelRef | null>(null);
  if (!loading && options.length === 0) return null;
  const builtin = (r: ModelRef) => providers.find((p) => p.id === r.provider_id)?.kind === "builtin";

  const fits = (ref: ModelRef) => rec?.installed.find((i) => sameModel(i.model, ref))?.fits ?? true;
  const use = async (ref: ModelRef) => {
    if (!settings) return;
    await api.putSettings({ ...settings, default_model: ref }).catch((e) => toast.error(e.message));
  };

  const inUse = options.find((o) => sameModel(o.ref, settings?.default_model));
  const own = options.filter((o) => o.locality !== "cloud" && o !== inUse);
  const cloud = providers
    .filter((p) => p.locality === "cloud")
    .map((p) => ({ provider: p, count: options.filter((o) => o.ref.provider_id === p.id).length }))
    .filter((c) => c.count > 0);

  const modelRow = (o: ModelOption) => {
    const current = o === inUse;
    const { name, maker, description } = info(o.ref.model, o.name);
    const detail =
      description ??
      [maker, o.price ? costLabel(o.price) : null, `From ${o.providerName}`].filter(Boolean).join(" · ");
    return (
      <Row
        key={`${o.ref.provider_id}/${o.ref.model}`}
        onClick={current ? undefined : () => use(o.ref)}
        title={name}
        detail={
          <>
            {detail}
            {o.locality !== "cloud" && !fits(o.ref) && (
              <span className="text-cloud"> · may be slow on this computer</span>
            )}
          </>
        }
        trailing={
          <>
            <LocalityBadge locality={o.locality} />
            <span className="flex w-7 justify-center">
              {current ? (
                <Check className="size-[18px] text-lime-deep" strokeWidth={2.6} />
              ) : (
                <span className="size-[18px] rounded-full shadow-[inset_0_0_0_1.5px_rgb(0_0_0/0.18)]" />
              )}
            </span>
          </>
        }
        accessory={
          builtin(o.ref) ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Options for ${name}`}
                  className="rounded-full text-muted-foreground"
                >
                  <MoreHorizontal />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem variant="destructive" disabled={current} onSelect={() => setRemoving(o.ref)}>
                  {current ? "In use, choose another first" : "Remove from this computer"}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : undefined
        }
      />
    );
  };

  return (
    <section ref={ref} className="flex scroll-mt-24 flex-col gap-2.5">
      <div className="flex min-h-7 items-end justify-between gap-4">
        <h2 className="section-label">Your models</h2>
        {options.length > 1 && (
          <Button variant="ghost" size="sm" onClick={() => onBrowse(null)} className="-mr-2 text-muted-foreground">
            <Search /> Browse all
          </Button>
        )}
      </div>
      <Grouped>
        {inUse && modelRow(inUse)}
        {own.map(modelRow)}
        {cloud.map(({ provider, count }) => (
          <Row
            key={`browse-${provider.id}`}
            onClick={() => onBrowse(provider.id)}
            icon={<SourceMark presetId={presetOf(provider, presets)?.id} locality="cloud" size="sm" />}
            title={`Choose from ${count} ${provider.name} model${count === 1 ? "" : "s"}`}
            detail="Search, and see what each one costs"
            trailing={<ChevronRight className="size-4 text-faint" />}
          />
        ))}
        {loading && options.length === 0 && <Skeleton className="m-4 h-10" />}
      </Grouped>
      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Remove {removing ? info(removing.model, options.find((o) => sameModel(o.ref, removing))?.name).name : ""}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              It's deleted from this computer to free up space. You can download it again any time.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() =>
                removing &&
                api.deleteModel(removing.provider_id, removing.model).catch((e) => toast.error(e.message))
              }
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
});

/**
 * The optional model for photos: it answers messages with photos when the model in use
 * can't see them. Greyed out while the model in use sees photos itself.
 */
function PhotoModel({ onChoose }: { onChoose: () => void }) {
  const settings = useSettings().data;
  const defaultSees = useDefaultSeesImages();
  const { options } = useAllModels();
  const info = useModelInfo();
  if (!settings?.default_model) return null;
  const chosen = settings.photo_model;
  const option = options.find((o) => sameModel(o.ref, chosen));
  const notNeeded = defaultSees === true;
  const stop = () => api.putSettings({ ...settings, photo_model: null }).catch((e) => toast.error(e.message));

  const detail = notNeeded
    ? "Not needed: the model in use already sees photos."
    : chosen
      ? [option?.price ? costLabel(option.price) : null, option ? `From ${option.providerName}` : null]
          .filter(Boolean)
          .join(" · ") || "Answers messages with photos"
      : "Optional. The model in use can't see photos; choose one that can, just for them.";
  return (
    <Section title="Model for photos">
      <Grouped>
        <div aria-disabled={notNeeded} className={cn(notNeeded && "opacity-50")}>
          <Row
            onClick={notNeeded ? undefined : onChoose}
            icon={
              <IconTile size="sm" className="bg-fill text-muted-foreground">
                <ImageIcon />
              </IconTile>
            }
            title={chosen ? info(chosen.model, option?.name).name : "None"}
            detail={detail}
            trailing={
              <>
                {chosen && option && <LocalityBadge locality={option.locality} />}
                {!notNeeded && <ChevronRight className="size-4 text-faint" />}
              </>
            }
            accessory={
              chosen && !notNeeded ? (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="Options for the model for photos"
                      className="rounded-full text-muted-foreground"
                    >
                      <MoreHorizontal />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onSelect={stop}>Don't use a model for photos</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : undefined
            }
          />
        </div>
      </Grouped>
    </Section>
  );
}

function SuggestedCard({
  model,
  best,
  providerId,
}: {
  model: CatalogModel;
  best: boolean;
  providerId: string | null;
}) {
  const pull = usePulls().data?.find((p) => p.model === model.id && p.provider_id === providerId);
  const qc = useQueryClient();
  const canStop = useProviders().data?.find((p) => p.id === providerId)?.kind === "builtin";
  const running = pull?.state === "running";
  const paused = pull?.state === "cancelled";
  const pct =
    pull?.total_bytes && pull.completed_bytes != null
      ? Math.round((pull.completed_bytes / pull.total_bytes) * 100)
      : null;

  const download = async () => {
    if (!providerId) return;
    try {
      const started = await api.pull(providerId, model.id);
      qc.setQueryData(keys.pulls, (list: (typeof pull)[] = []) => [
        ...list.filter((p) => p?.model !== started.model),
        started,
      ]);
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Card
      className={cn(
        "flex flex-col gap-3",
        best && "shadow-[0_0_0_1px_rgb(200_242_93/0.9),0_1px_2px_rgb(0_0_0/0.04),0_4px_16px_-4px_rgb(86_118_13/0.15)]!",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="type-headline">{model.name}</span>
        {best && <Pill className="bg-lime-soft text-lime-deep">Best fit</Pill>}
      </div>
      <span className="type-subhead text-muted-foreground">{model.description}</span>
      <div className="mt-auto flex flex-col gap-2 pt-2">
        {running ? (
          <>
            <Progress value={pct ?? 0} />
            <div className="flex items-center justify-between gap-2">
              <span className="type-footnote text-muted-foreground">
                {pull.status}
                {pct != null && ` · ${pct}%`}
              </span>
              {canStop && providerId && (
                <Button
                  variant="ghost"
                  size="xs"
                  className="-mr-1 text-muted-foreground"
                  onClick={() => api.cancelPull(providerId, model.id).catch((e) => toast.error(e.message))}
                >
                  Stop
                </Button>
              )}
            </div>
          </>
        ) : pull?.state === "failed" ? (
          <span className="type-footnote text-destructive">{pull.error}</span>
        ) : paused ? (
          <span className="type-footnote text-muted-foreground">
            Paused{pct != null && ` at ${pct}%`}. It continues where it stopped.
          </span>
        ) : null}
        {!running &&
          (providerId ? (
            <Button variant={best ? "lime" : "secondary"} onClick={download}>
              <Download />
              {pull?.state === "failed"
                ? "Try again"
                : paused
                  ? "Resume"
                  : `Download · ${formatBytes(model.download_bytes)}`}
            </Button>
          ) : (
            <GetOllama size={model.download_bytes} />
          ))}
      </div>
    </Card>
  );
}

/** Downloads need a local model runner; explain how to get one. */
function GetOllama({ size }: { size: number }) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="secondary">
          <Download /> Get it · {formatBytes(size)}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[290px] p-4">
        <p className="type-callout font-medium">Install Ollama first</p>
        <p className="mt-1 type-subhead text-muted-foreground">
          Ollama runs models on this computer. Once it's installed, Mimi finds it and downloads
          models for you.
        </p>
        <Button size="sm" className="mt-3" onClick={() => openExternal("https://ollama.com/download")}>
          Get Ollama
        </Button>
      </PopoverContent>
    </Popover>
  );
}

function CloudCard({ recommended = false, onAdd }: { recommended?: boolean; onAdd: () => void }) {
  return (
    <Card className="flex flex-col gap-3 bg-[linear-gradient(180deg,#fff8ec,#ffffff_70%)]!">
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-2 type-headline">
          <Cloud className="size-[18px] text-cloud" /> Cloud models
        </span>
        {recommended && <Pill className="bg-cloud-soft text-cloud">Recommended</Pill>}
      </div>
      <span className="type-subhead text-muted-foreground">
        Smarter and faster, but your messages leave this computer.
      </span>
      <Button variant="secondary" onClick={onAdd} className="mt-auto text-cloud">
        Add a service
      </Button>
    </Card>
  );
}

function Sources({ onAdd }: { onAdd: () => void }) {
  const providers = useProviders().data ?? [];
  const presets = useQuery({ queryKey: keys.presets, queryFn: api.presets }).data ?? [];
  const runtime = useRuntime().data;
  const [editing, setEditing] = useState<Provider | null>(null);
  const [removing, setRemoving] = useState<Provider | null>(null);
  return (
    <Section
      title="Model sources"
      action={
        <Button variant="ghost" size="sm" onClick={onAdd} className="-mr-2 text-muted-foreground">
          <Plus /> Add
        </Button>
      }
    >
      {providers.length === 0 ? (
        <p className="px-1 type-callout text-muted-foreground">No model sources yet.</p>
      ) : (
        <Grouped>
          {providers.map((p) => {
            return (
              <Row
                key={p.id}
                icon={<SourceMark presetId={presetOf(p, presets)?.id} locality={p.locality} />}
                title={p.name}
                detail={
                  p.kind === "builtin"
                    ? `Runs models right here${runtime?.models_bytes ? ` · ${formatBytes(runtime.models_bytes)} of models` : ""}`
                    : { device: "On this computer", network: "On your network", cloud: "Cloud service" }[p.locality]
                }
                trailing={
                  p.kind === "builtin" ? undefined : (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Options for ${p.name}`}
                        className="rounded-full text-muted-foreground"
                      >
                        <MoreHorizontal />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={() => setEditing(p)}>Edit</DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem variant="destructive" onSelect={() => setRemoving(p)}>
                        Remove
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                  )
                }
              />
            );
          })}
        </Grouped>
      )}
      <EditSourceDialog provider={editing} onClose={() => setEditing(null)} />
      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {removing?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              Its models won't be available anymore. Your conversations are kept.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => removing && api.removeProvider(removing.id).catch((e) => toast.error(e.message))}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Section>
  );
}
