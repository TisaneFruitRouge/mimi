import { forwardRef, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Check, Cloud, Download, Loader2, Plus, Radar, Trash2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { CatalogModel } from "@/bindings/CatalogModel";
import type { HardwareTier } from "@/bindings/HardwareTier";
import type { ModelRef } from "@/bindings/ModelRef";
import type { Provider } from "@/bindings/Provider";
import { LocalityBadge } from "@/components/locality-badge";
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
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { AddSourceDialog, EditSourceDialog } from "@/features/models/source-dialogs";
import { api, keys } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import {
  sameModel,
  useActiveModel,
  useAllModels,
  useModelInfo,
  useProviders,
  usePulls,
  useRecommendations,
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
  const rec = useRecommendations().data;
  const yourModels = useRef<HTMLElement>(null);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[960px] flex-col gap-9 px-6 pt-6 pb-12">
        <div className="flex flex-col gap-1.5">
          <h1 className="text-[30px] font-semibold tracking-[-0.03em]">
            {setup ? "Welcome to Hearth" : "Models"}
          </h1>
          <p className="max-w-xl text-[15px] text-muted-foreground">
            {setup
              ? "First, choose how your assistant thinks. Models on this computer keep everything private."
              : "What your assistant thinks with, and where it runs."}
          </p>
        </div>

        <div className="grid grid-cols-3 gap-3.5">
          {setup ? (
            <SetupCard onAdd={() => setAdding("any")} />
          ) : (
            <ActiveModelCard
              onChange={() => yourModels.current?.scrollIntoView({ behavior: "smooth", block: "start" })}
              onChat={onChat}
            />
          )}
          <ComputerCard />
        </div>

        {!setup && <DetectedServers />}
        <YourModels ref={yourModels} />

        <section className="flex flex-col gap-3">
          <SectionLabel>Good fits for this computer</SectionLabel>
          <div className="grid grid-cols-3 gap-3.5">
            {rec?.prefer_cloud && <CloudCard recommended onAdd={() => setAdding("cloud")} />}
            {rec?.suggested.map((m, i) => (
              <SuggestedCard key={m.id} model={m} best={i === 0 && !rec.prefer_cloud} providerId={rec.download_provider_id} />
            ))}
            {!rec && [0, 1, 2].map((i) => <Skeleton key={i} className="h-44 rounded-2xl" />)}
            {rec && !rec.prefer_cloud && <CloudCard onAdd={() => setAdding("cloud")} />}
          </div>
        </section>

        <Sources onAdd={() => setAdding("any")} />
      </div>
      <AddSourceDialog
        open={adding !== null}
        onOpenChange={(o) => !o && setAdding(null)}
        only={adding === "cloud" ? "cloud" : undefined}
      />
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="font-mono text-[11.5px] font-medium tracking-[0.06em] text-faint uppercase">
      {children}
    </h2>
  );
}

function Card({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("rounded-2xl border bg-background p-5 shadow-xs", className)}>{children}</div>;
}

/** First run: the one thing to do next, front and centre. */
function SetupCard({ onAdd }: { onAdd: () => void }) {
  const rec = useRecommendations().data;
  const server = rec?.detected_servers.find((s) => !s.already_added);
  const [busy, setBusy] = useState(false);
  return (
    <Card className="col-span-2 flex flex-col gap-5 p-6">
      <SectionLabel>Get started</SectionLabel>
      {server ? (
        <>
          <div className="flex flex-col gap-1.5">
            <span className="text-[26px] leading-tight font-semibold tracking-[-0.02em]">
              {server.name} is already on this computer
            </span>
            <span className="text-[14px] text-muted-foreground">
              It has {server.model_count} model{server.model_count === 1 ? "" : "s"} ready. Connect it and
              everything stays private.
            </span>
          </div>
          <div className="mt-auto flex gap-2">
            <Button
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
              {busy ? <Loader2 className="animate-spin" /> : <Radar />} Connect {server.name}
            </Button>
            <Button variant="ghost" onClick={onAdd}>
              Use something else
            </Button>
          </div>
        </>
      ) : (
        <>
          <div className="flex flex-col gap-1.5">
            <span className="text-[26px] leading-tight font-semibold tracking-[-0.02em]">
              Choose a model below
            </span>
            <span className="text-[14px] text-muted-foreground">
              Download one that fits this computer, or connect a cloud service or a server you own.
            </span>
          </div>
          <div className="mt-auto">
            <Button variant="outline" onClick={onAdd}>
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
      <div className="flex items-center justify-between">
        <SectionLabel>Active model</SectionLabel>
        {active && <LocalityBadge locality={active.provider.locality} />}
      </div>
      {active ? (
        <div className="flex flex-col gap-1.5">
          <span className="text-[32px] leading-none font-semibold tracking-[-0.03em]">{active.name}</span>
          <span className="text-[14px] text-muted-foreground">
            {active.description ?? "A model from " + active.provider.name + "."}{" "}
            <span className="text-faint">via {active.provider.name}</span>
          </span>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          <span className="text-[26px] leading-none font-semibold tracking-[-0.02em] text-faint">
            No model yet
          </span>
          <span className="text-[14px] text-muted-foreground">
            Pick one of the models below, or connect a model source.
          </span>
        </div>
      )}
      <div className="mt-auto flex gap-2">
        <Button variant="outline" size="sm" onClick={onChange}>
          Change model
        </Button>
        {onChat && active && (
          <Button variant="ghost" size="sm" onClick={onChat}>
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
  if (!rec) return <Skeleton className="h-full min-h-44 rounded-2xl" />;
  const hw = rec.hardware;
  const gpu = hw.gpus.find((g) => g.kind === "discrete") ?? hw.gpus[0];
  const activeSize =
    active?.provider.locality === "device"
      ? options.find((o) => sameModel(o.ref, active.ref))?.sizeBytes
      : null;
  const budgetGb = rec.model_budget_bytes / 1e9;
  const usedPct = activeSize ? Math.min(100, (activeSize / rec.model_budget_bytes) * 100) : 0;

  return (
    <Card className="flex flex-col gap-3.5">
      <SectionLabel>This computer</SectionLabel>
      <p className="text-[15px] leading-snug font-medium">{tierLine[rec.tier]}</p>
      <div className="flex flex-col gap-1.5">
        <div className="flex justify-between font-mono text-[11.5px] text-muted-foreground">
          <span>model memory</span>
          <span>
            {activeSize ? `${Math.round(activeSize / 1e9)} / ` : ""}
            {Math.round(budgetGb)} GB
          </span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-subtle">
          <div className="h-full rounded-full bg-lime" style={{ width: `${usedPct}%` }} />
        </div>
      </div>
      <div className="mt-auto text-[12.5px] leading-relaxed text-muted-foreground">
        {hw.cpu_name}
        <br />
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
        <div
          key={s.preset_id}
          className="flex items-center gap-4 rounded-2xl border border-[#bfe3d4] bg-private-soft px-5 py-4"
        >
          <Radar className="size-5 text-private" />
          <div className="flex-1">
            <p className="text-[14.5px] font-medium">{s.name} is running on this computer</p>
            <p className="text-[13px] text-muted-foreground">
              {s.model_count} model{s.model_count === 1 ? "" : "s"} ready. Connect it to keep
              everything private.
            </p>
          </div>
          <Button
            disabled={busy !== null}
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

const YourModels = forwardRef<HTMLElement>(function YourModels(_, ref) {
  const { options, loading } = useAllModels();
  const settings = useSettings().data;
  const rec = useRecommendations().data;
  const info = useModelInfo();
  if (!loading && options.length === 0) return null;

  const fits = (ref: ModelRef) =>
    rec?.installed.find((i) => sameModel(i.model, ref))?.fits ?? true;
  const use = async (ref: ModelRef) => {
    if (!settings) return;
    await api.putSettings({ ...settings, default_model: ref }).catch((e) => toast.error(e.message));
  };

  return (
    <section ref={ref} className="flex scroll-mt-4 flex-col gap-3">
      <SectionLabel>Your models</SectionLabel>
      <div className="flex flex-col divide-y rounded-2xl border bg-background shadow-xs">
        {options.map((o) => {
          const current = sameModel(o.ref, settings?.default_model);
          const { name, description } = info(o.ref.model);
          return (
            <div key={`${o.ref.provider_id}/${o.ref.model}`} className="flex items-center gap-4 px-5 py-3.5">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-[14.5px] font-medium">
                  {name}
                  {o.sizeBytes ? (
                    <span className="font-mono text-[11.5px] font-normal text-faint">
                      {formatBytes(o.sizeBytes)}
                    </span>
                  ) : null}
                </div>
                <div className="truncate text-[13px] text-muted-foreground">
                  {description ?? o.ref.model} · via {o.providerName}
                  {o.locality !== "cloud" && !fits(o.ref) && (
                    <span className="text-cloud"> · may be slow on this computer</span>
                  )}
                </div>
              </div>
              <LocalityBadge locality={o.locality} />
              {current ? (
                <span className="flex h-8 w-20 items-center justify-center gap-1 text-[12.5px] font-medium text-private">
                  <Check className="size-4" /> In use
                </span>
              ) : (
                <Button variant="outline" size="sm" className="w-20" onClick={() => use(o.ref)}>
                  Use
                </Button>
              )}
            </div>
          );
        })}
        {loading && options.length === 0 && <Skeleton className="m-4 h-10" />}
      </div>
    </section>
  );
});

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
  const running = pull?.state === "running";
  const pct =
    pull?.total_bytes && pull.completed_bytes != null
      ? Math.round((pull.completed_bytes / pull.total_bytes) * 100)
      : null;

  const download = async () => {
    if (!providerId) return;
    try {
      const started = await api.pull(providerId, model.id);
      qc.setQueryData(keys.pulls, (list: typeof pull[] = []) => [
        ...list.filter((p) => p?.model !== started.model),
        started,
      ]);
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Card className={cn("flex flex-col gap-3", best && "border-[#cfe39a]")}>
      <div className="flex items-center justify-between">
        <span className="text-[15.5px] font-medium">{model.name}</span>
        {best && (
          <span className="rounded-md bg-lime-soft px-1.5 py-0.5 font-mono text-[10.5px] font-medium text-[#4d6b00]">
            BEST FIT
          </span>
        )}
      </div>
      <span className="text-[13px] leading-snug text-muted-foreground">{model.description}</span>
      <div className="mt-auto flex flex-col gap-2 pt-1">
        {running ? (
          <>
            <Progress value={pct ?? 0} className="h-1.5 [&>*]:bg-lime" />
            <span className="font-mono text-[11.5px] text-muted-foreground">
              {pull.status}
              {pct != null && ` · ${pct}%`}
              {pull.total_bytes ? ` of ${formatBytes(pull.total_bytes)}` : ""}
            </span>
          </>
        ) : pull?.state === "failed" ? (
          <span className="text-[12.5px] text-destructive">{pull.error}</span>
        ) : null}
        {!running &&
          (providerId ? (
            <button
              onClick={download}
              className={cn(
                "flex h-9 items-center justify-center gap-1.5 rounded-[10px] text-[13px] font-medium transition",
                best
                  ? "bg-lime text-lime-ink shadow-[inset_0_0_0_1px_rgba(0,0,0,0.06)] hover:brightness-95"
                  : "bg-subtle hover:bg-secondary",
              )}
            >
              <Download className="size-4" />
              {pull?.state === "failed" ? "Try again" : `Download · ${formatBytes(model.download_bytes)}`}
            </button>
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
        <button className="flex h-9 items-center justify-center gap-1.5 rounded-[10px] bg-subtle text-[13px] font-medium hover:bg-secondary">
          <Download className="size-4" /> Get it · {formatBytes(size)}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-72 p-4">
        <p className="text-sm font-medium">Install Ollama first</p>
        <p className="mt-1 text-[13px] leading-relaxed text-muted-foreground">
          Ollama runs models on this computer. Once it's installed, Hearth finds it and downloads
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
    <div className="flex flex-col gap-3 rounded-2xl border border-dashed border-[#e8c99a] bg-cloud-soft/50 p-5">
      <span className="flex items-center gap-2 text-[15.5px] font-medium text-cloud">
        <Cloud className="size-4" /> Cloud models
        {recommended && (
          <span className="rounded-md bg-cloud-soft px-1.5 py-0.5 font-mono text-[10.5px]">RECOMMENDED</span>
        )}
      </span>
      <span className="text-[13px] leading-snug text-muted-foreground">
        Smarter and faster, but your messages leave this computer.
      </span>
      <button
        onClick={onAdd}
        className="mt-auto h-9 rounded-[10px] border border-[#e8c99a] text-[13px] font-medium text-cloud transition hover:bg-cloud-soft"
      >
        Add a service
      </button>
    </div>
  );
}

function Sources({ onAdd }: { onAdd: () => void }) {
  const providers = useProviders().data ?? [];
  const [editing, setEditing] = useState<Provider | null>(null);
  const [removing, setRemoving] = useState<Provider | null>(null);
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-end justify-between">
        <SectionLabel>Model sources</SectionLabel>
        <Button variant="ghost" size="sm" onClick={onAdd} className="-mr-2 text-muted-foreground">
          <Plus /> Add a source
        </Button>
      </div>
      {providers.length === 0 ? (
        <p className="text-[13.5px] text-muted-foreground">No model sources yet.</p>
      ) : (
        <div className="flex flex-col divide-y rounded-2xl border bg-background shadow-xs">
          {providers.map((p) => (
            <div key={p.id} className="flex items-center gap-4 px-5 py-3">
              <div className="min-w-0 flex-1">
                <div className="text-[14.5px] font-medium">{p.name}</div>
                <div className="truncate font-mono text-[11.5px] text-faint">
                  {p.base_url}
                  {p.has_api_key && " · key saved"}
                </div>
              </div>
              <LocalityBadge locality={p.locality} />
              <Button variant="ghost" size="sm" onClick={() => setEditing(p)}>
                Edit
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setRemoving(p)}
                aria-label={`Remove ${p.name}`}
                className="text-faint"
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
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
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() => removing && api.removeProvider(removing.id).catch((e) => toast.error(e.message))}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
