import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, CircleCheck, Loader2, Server } from "lucide-react";
import { toast } from "sonner";

import type { ProbeResult } from "@/bindings/ProbeResult";
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
import { api, keys } from "@/lib/api";
import { useSettings } from "@/lib/queries";

type Draft = { presetId: string | null; name: string; baseUrl: string; apiKey: string; needsKey: boolean };

export function AddProviderDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const presets = useQuery({ queryKey: keys.presets, queryFn: api.presets }).data ?? [];
  const settings = useSettings().data;
  const [draft, setDraft] = useState<Draft | null>(null);
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const close = (next: boolean) => {
    onOpenChange(next);
    if (!next) {
      setDraft(null);
      setProbe(null);
      setError(null);
    }
  };

  const pick = (p: ProviderPreset | null) => {
    setProbe(null);
    setError(null);
    setDraft(
      p
        ? { presetId: p.id, name: p.name, baseUrl: p.base_url, apiKey: "", needsKey: p.needs_api_key }
        : { presetId: null, name: "", baseUrl: "http://", apiKey: "", needsKey: false },
    );
  };

  const update = (patch: Partial<Draft>) => {
    setDraft((d) => (d ? { ...d, ...patch } : d));
    setProbe(null);
    setError(null);
  };

  const test = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      setProbe(await api.probe(draft.baseUrl, draft.apiKey || null));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    if (!draft || !probe) return;
    setBusy(true);
    try {
      const provider = await api.addProvider({
        name: draft.name.trim() || new URL(probe.base_url).host,
        kind: "openai_compatible",
        base_url: probe.base_url,
        api_key: draft.apiKey || null,
        locality: null,
      });
      // First provider: start with its first model so the user can chat right away.
      if (settings && !settings.default_model && probe.models.length > 0) {
        await api.putSettings({
          ...settings,
          default_model: { provider_id: provider.id, model: probe.models[0].id },
        });
      }
      toast.success(`${provider.name} added`);
      close(false);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const local = presets.filter((p) => p.locality !== "cloud");
  const cloud = presets.filter((p) => p.locality === "cloud");

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="sm:max-w-lg">
        {!draft ? (
          <>
            <DialogHeader>
              <DialogTitle>Add a model provider</DialogTitle>
              <DialogDescription>Where should your assistant's models run?</DialogDescription>
            </DialogHeader>
            <PresetGroup title="On this computer" presets={local} onPick={pick} />
            <PresetGroup
              title="Cloud services"
              hint="These receive your messages. Pick one whose privacy policy you trust."
              presets={cloud}
              onPick={pick}
            />
            <button
              onClick={() => pick(null)}
              className="flex items-center gap-3 rounded-lg border border-dashed p-3 text-left text-sm hover:bg-accent"
            >
              <Server className="size-4 text-muted-foreground" />
              <div>
                <div className="font-medium">Another server</div>
                <div className="text-muted-foreground">
                  Any OpenAI-compatible address, e.g. a GPU machine on your network.
                </div>
              </div>
            </button>
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Button variant="ghost" size="icon-sm" onClick={() => setDraft(null)} aria-label="Back">
                  <ArrowLeft />
                </Button>
                {draft.presetId ? draft.name : "Another server"}
              </DialogTitle>
            </DialogHeader>
            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault();
                probe ? add() : test();
              }}
            >
              {!draft.presetId && (
                <Field label="Name">
                  <Input
                    value={draft.name}
                    onChange={(e) => update({ name: e.target.value })}
                    placeholder="My GPU server"
                  />
                </Field>
              )}
              <Field label="Address">
                <Input
                  value={draft.baseUrl}
                  onChange={(e) => update({ baseUrl: e.target.value })}
                  className="font-mono text-xs"
                  autoFocus={!draft.presetId}
                />
              </Field>
              <Field label={draft.needsKey ? "API key" : "API key (optional)"}>
                <Input
                  type="password"
                  value={draft.apiKey}
                  onChange={(e) => update({ apiKey: e.target.value })}
                  autoFocus={!!draft.presetId && draft.needsKey}
                  placeholder={draft.needsKey ? "Paste your key" : "Only if the server needs one"}
                />
                <p className="text-xs text-muted-foreground">
                  Stored encrypted on this computer and never shown again.
                </p>
              </Field>
              {error && <p className="text-sm text-destructive">{error}</p>}
              {probe && (
                <div className="flex items-center gap-2 rounded-lg bg-muted p-3 text-sm">
                  <CircleCheck className="size-4 text-emerald-600" />
                  <span className="flex-1">
                    Connected · {probe.models.length} model{probe.models.length === 1 ? "" : "s"}
                  </span>
                  <LocalityBadge locality={probe.locality} />
                </div>
              )}
              <DialogFooter>
                {probe ? (
                  <Button type="submit" disabled={busy}>
                    {busy && <Loader2 className="animate-spin" />} Add provider
                  </Button>
                ) : (
                  <Button
                    type="submit"
                    disabled={busy || (draft.needsKey && !draft.apiKey.trim())}
                  >
                    {busy && <Loader2 className="animate-spin" />} Test connection
                  </Button>
                )}
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function PresetGroup({
  title,
  hint,
  presets,
  onPick,
}: {
  title: string;
  hint?: string;
  presets: ProviderPreset[];
  onPick: (p: ProviderPreset) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div>
        <h3 className="text-sm font-medium">{title}</h3>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="grid grid-cols-3 gap-2">
        {presets.map((p) => (
          <button
            key={p.id}
            onClick={() => onPick(p)}
            className="rounded-lg border p-3 text-left text-sm transition-colors hover:bg-accent"
          >
            <div className="font-medium">{p.name}</div>
            <div className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{p.description}</div>
          </button>
        ))}
      </div>
    </div>
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
