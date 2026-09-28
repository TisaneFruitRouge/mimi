import { useState } from "react";
import { motion } from "motion/react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ArrowUpRight, ChevronRight, CircleCheck, Loader2 } from "lucide-react";
import { toast } from "sonner";

import type { Locality } from "@/bindings/Locality";
import type { ProbeResult } from "@/bindings/ProbeResult";
import type { Provider } from "@/bindings/Provider";
import type { ProviderPreset } from "@/bindings/ProviderPreset";
import { LocalityBadge } from "@/components/locality-badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ModelList, type ModelEntry } from "@/features/models/model-list";
import { SourceMark } from "@/features/models/source-mark";
import { api, keys } from "@/lib/api";
import { localityLabel } from "@/lib/format";
import { useModelInfo, useSettings } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

/** What's being connected: a preset, or (without one) a server on the user's network. */
type Draft = {
  preset: ProviderPreset | null;
  name: string;
  baseUrl: string;
  apiKey: string;
};

type Step =
  | { step: "choose" }
  | { step: "details"; draft: Draft }
  | { step: "model"; draft: Draft; probe: ProbeResult };

const rise = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0 },
  transition: { type: "spring", stiffness: 460, damping: 34 },
} as const;

/**
 * Adds a model source in three steps: pick where models run, connect (checked before
 * anything is saved), then choose the model to use.
 */
export function AddSourceDialog({
  open,
  onOpenChange,
  only,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Show just the cloud services, e.g. from the "cloud models" card. */
  only?: "cloud";
}) {
  const [state, setState] = useState<Step>({ step: "choose" });

  const close = (next: boolean) => {
    onOpenChange(next);
    if (!next) setState({ step: "choose" });
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="gap-5 sm:max-w-[500px]">
        {state.step === "choose" && (
          <ChooseStep
            only={only}
            onPick={(preset) =>
              setState({
                step: "details",
                draft: preset
                  ? {
                      preset,
                      name: preset.name,
                      baseUrl: preset.base_url,
                      apiKey: "",
                    }
                  : { preset: null, name: "", baseUrl: "http://", apiKey: "" },
              })
            }
          />
        )}
        {state.step === "details" && (
          <DetailsStep
            key="details"
            initial={state.draft}
            onBack={() => setState({ step: "choose" })}
            onConnected={(draft, probe) => setState({ step: "model", draft, probe })}
          />
        )}
        {state.step === "model" && (
          <ModelStep
            draft={state.draft}
            probe={state.probe}
            onBack={() => setState({ step: "details", draft: state.draft })}
            onDone={() => close(false)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function ChooseStep({ only, onPick }: { only?: "cloud"; onPick: (p: ProviderPreset | null) => void }) {
  const presets = useQuery({ queryKey: keys.presets, queryFn: api.presets }).data ?? [];
  const local = presets.filter((p) => p.locality !== "cloud");
  const cloud = presets.filter((p) => p.locality === "cloud");
  return (
    <>
      <DialogHeader>
        <DialogTitle>{only === "cloud" ? "Add a cloud service" : "Add a model source"}</DialogTitle>
        <DialogDescription>
          {only === "cloud"
            ? "Cloud models are smarter and faster, but your messages are sent to the service you pick."
            : "Where should your assistant's models run?"}
        </DialogDescription>
      </DialogHeader>
      <motion.div {...rise} className="flex flex-col gap-5">
        {only !== "cloud" && (
          <PresetList title="On this computer" presets={local} onPick={onPick}>
            <PickRow
              mark={<SourceMark locality="network" />}
              title="A server on your network"
              detail="For example a computer with a big graphics card at home."
              onClick={() => onPick(null)}
            />
          </PresetList>
        )}
        <PresetList
          title="Cloud services"
          hint={only === "cloud" ? undefined : "These receive your messages"}
          presets={cloud}
          onPick={onPick}
        />
      </motion.div>
    </>
  );
}

function PresetList({
  title,
  hint,
  presets,
  onPick,
  children,
}: {
  title: string;
  hint?: string;
  presets: ProviderPreset[];
  onPick: (p: ProviderPreset) => void;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <h3 className="section-label">
        {title}
        {hint && <span className="font-normal text-faint"> · {hint}</span>}
      </h3>
      <div className="flex flex-col overflow-hidden rounded-[14px] bg-subtle [&>*+*]:shadow-[inset_0_0.5px_0_var(--separator)]">
        {presets.map((p) => (
          <PickRow
            key={p.id}
            mark={<SourceMark presetId={p.id} locality={p.locality} />}
            title={p.name}
            detail={p.description}
            onClick={() => onPick(p)}
          />
        ))}
        {children}
      </div>
    </div>
  );
}

function PickRow({
  mark,
  title,
  detail,
  onClick,
}: {
  mark: React.ReactNode;
  title: string;
  detail: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="group flex items-center gap-3 px-3.5 py-2.5 text-left transition-colors hover:bg-[rgb(118_118_128/0.08)] focus-visible:bg-[rgb(118_118_128/0.08)] focus-visible:outline-none"
    >
      {mark}
      <div className="min-w-0 flex-1">
        <div className="type-callout font-medium">{title}</div>
        <div className="type-subhead text-muted-foreground">{detail}</div>
      </div>
      <ChevronRight className="size-4 shrink-0 text-faint transition-transform group-hover:translate-x-0.5" />
    </button>
  );
}

function StepHeader({ draft, onBack }: { draft: Draft; onBack: () => void }) {
  const locality = draft.preset?.locality ?? "network";
  return (
    <DialogHeader>
      <DialogTitle className="flex items-center gap-2.5">
        <Button variant="ghost" size="icon-sm" onClick={onBack} aria-label="Back" className="-ml-1.5">
          <ArrowLeft />
        </Button>
        <SourceMark presetId={draft.preset?.id} locality={locality} size="sm" />
        <span className="flex-1">{draft.preset ? draft.preset.name : "A server on your network"}</span>
        <LocalityBadge locality={locality} className="mr-8" />
      </DialogTitle>
      <DialogDescription className="sr-only">Connect {draft.preset?.name ?? "a server"}</DialogDescription>
    </DialogHeader>
  );
}

function DetailsStep({
  initial,
  onBack,
  onConnected,
}: {
  initial: Draft;
  onBack: () => void;
  onConnected: (draft: Draft, probe: ProbeResult) => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const preset = draft.preset;
  const cloud = preset?.locality === "cloud";
  const needsKey = preset?.needs_api_key ?? false;

  const update = (patch: Partial<Draft>) => {
    setDraft((d) => ({ ...d, ...patch }));
    setError(null);
  };

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const probe = await api.probe({
        kind: preset?.kind ?? "openai_compatible",
        base_url: draft.baseUrl,
        api_key: draft.apiKey.trim() || null,
      });
      onConnected(draft, probe);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <StepHeader draft={draft} onBack={onBack} />
      <motion.form
        {...rise}
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          connect();
        }}
      >
        {cloud && (
          <p className="type-callout text-muted-foreground">
            Your messages, and what your assistant reads to answer them (like emails or notes), are sent to{" "}
            {preset.name}. {preset.name} charges your account for what you use.
          </p>
        )}
        {preset && !cloud && (
          <p className="type-callout text-muted-foreground">
            Make sure {preset.name} is open on this computer, then connect. Nothing leaves this computer.
          </p>
        )}
        {!preset && (
          <>
            <Field label="Name">
              <Input
                value={draft.name}
                onChange={(e) => update({ name: e.target.value })}
                placeholder="Home server"
              />
            </Field>
            <Field label="Address">
              <Input
                value={draft.baseUrl}
                onChange={(e) => update({ baseUrl: e.target.value })}
                className="text-[14px]"
                autoFocus
              />
            </Field>
          </>
        )}
        {(needsKey || !preset) && (
          <Field label={needsKey ? "API key" : "API key (optional)"}>
            <Input
              type="password"
              value={draft.apiKey}
              onChange={(e) => update({ apiKey: e.target.value })}
              autoFocus={!!preset}
              autoComplete="off"
              spellCheck={false}
              placeholder={needsKey ? "Paste your key" : "Only if the server needs one"}
            />
            <p className="type-footnote text-muted-foreground">
              Kept encrypted on this computer, and never shown again.
            </p>
          </Field>
        )}
        {error && <p className="type-subhead text-destructive">{error}</p>}
        <div className="flex items-center justify-between gap-2 pt-1">
          {preset?.key_url ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="-ml-2 text-muted-foreground"
              onClick={() => openExternal(preset.key_url!)}
            >
              Get a key from {preset.name} <ArrowUpRight />
            </Button>
          ) : (
            <span />
          )}
          <Button type="submit" disabled={busy || (needsKey && !draft.apiKey.trim())}>
            {busy && <Loader2 className="animate-spin" />}
            Connect
          </Button>
        </div>
      </motion.form>
    </>
  );
}

function ModelStep({
  draft,
  probe,
  onBack,
  onDone,
}: {
  draft: Draft;
  probe: ProbeResult;
  onBack: () => void;
  onDone: () => void;
}) {
  const settings = useSettings().data;
  const info = useModelInfo();
  const recommended = draft.preset?.recommended_model ?? null;
  const locality = draft.preset?.locality ?? probe.locality;
  // Not saved yet, so the entries point at a placeholder source.
  const entries: ModelEntry[] = probe.models.map((m) => {
    const { name, maker } = info(m.id, m.name);
    return {
      ref: { provider_id: "new", model: m.id },
      name,
      maker,
      sourceName: draft.preset?.name ?? draft.name,
      locality,
      sizeBytes: m.size_bytes,
      supportsTools: m.supports_tools,
      price: m.price,
      recommended: m.id === recommended,
    };
  });
  const [chosen, setChosen] = useState<string | null>(
    (probe.models.find((m) => m.id === recommended) ?? probe.models[0])?.id ?? null,
  );
  const [busy, setBusy] = useState<"use" | "add" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const chosenEntry = entries.find((e) => e.ref.model === chosen) ?? null;

  const add = async (use: boolean, entry = chosenEntry) => {
    setBusy(use ? "use" : "add");
    setError(null);
    try {
      const provider = await api.addProvider({
        name: draft.name.trim() || new URL(probe.base_url).host,
        kind: draft.preset?.kind ?? "openai_compatible",
        base_url: probe.base_url,
        api_key: draft.apiKey.trim() || null,
        locality: null,
      });
      if (use && entry && settings) {
        await api.putSettings({
          ...settings,
          default_model: { provider_id: provider.id, model: entry.ref.model },
        });
        toast.success(`Your assistant now uses ${entry.name}`);
      } else {
        toast.success(`${provider.name} added`);
      }
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <StepHeader draft={draft} onBack={onBack} />
      <motion.div {...rise} className="flex min-h-0 flex-col gap-4">
        <div className="flex items-center gap-2.5 rounded-[12px] bg-private-soft px-3.5 py-2.5 type-callout">
          <CircleCheck className="size-4 shrink-0 text-private" />
          <span className="flex-1">
            Connected · {probe.models.length} model{probe.models.length === 1 ? "" : "s"} available
          </span>
        </div>

        {entries.length > 0 ? (
          <ModelList
            entries={entries}
            selected={chosenEntry?.ref ?? null}
            onSelect={(ref) => setChosen(ref.model)}
            onChoose={(ref) => {
              setChosen(ref.model);
              add(true, entries.find((e) => e.ref.model === ref.model));
            }}
            className={entries.length > 6 ? "h-[380px]" : undefined}
          />
        ) : (
          <p className="type-callout text-muted-foreground">
            It has no models yet. Add some there, then choose one in Models.
          </p>
        )}

        {error && <p className="type-subhead text-destructive">{error}</p>}
        <div className="flex items-center justify-end gap-2">
          {settings?.default_model && chosenEntry && (
            <Button variant="ghost" disabled={busy !== null} onClick={() => add(false)}>
              {busy === "add" && <Loader2 className="animate-spin" />}
              Add without switching
            </Button>
          )}
          <Button disabled={busy !== null} onClick={() => add(!!chosenEntry)}>
            {busy === "use" && <Loader2 className="animate-spin" />}
            {chosenEntry ? `Use ${chosenEntry.name}` : "Add"}
          </Button>
        </div>
      </motion.div>
    </>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label>{label}</Label>
      {children}
    </div>
  );
}

export function EditSourceDialog({ provider, onClose }: { provider: Provider | null; onClose: () => void }) {
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [locality, setLocality] = useState<Locality>("device");
  const [loadedId, setLoadedId] = useState<string | null>(null);
  if (provider && provider.id !== loadedId) {
    setLoadedId(provider.id);
    setName(provider.name);
    setBaseUrl(provider.base_url);
    setApiKey("");
    setLocality(provider.locality);
  }
  const close = () => {
    setLoadedId(null);
    onClose();
  };
  // Anthropic has one address, in the cloud; only the name and key can change.
  const fixed = provider?.kind === "anthropic";

  const save = async () => {
    if (!provider) return;
    try {
      await api.updateProvider(provider.id, {
        name,
        base_url: baseUrl === provider.base_url ? null : baseUrl,
        api_key: apiKey ? apiKey : null,
        locality,
      });
      close();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Dialog open={!!provider} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{provider?.name}</DialogTitle>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <Field label="Name">
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          {!fixed && (
            <Field label="Address">
              <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} className="text-[14px]" />
            </Field>
          )}
          <Field label="API key">
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={provider?.has_api_key ? "Leave empty to keep the saved key" : "None"}
            />
          </Field>
          {!fixed && (
            <Field label="Where it runs">
              <Select value={locality} onValueChange={(v) => setLocality(v as Locality)}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(["device", "network", "cloud"] as const).map((l) => (
                    <SelectItem key={l} value={l}>
                      {localityLabel[l]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="type-footnote text-muted-foreground">
                Detected from the address. Change it only for your own server behind a public name.
              </p>
            </Field>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim()}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
