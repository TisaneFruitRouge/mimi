import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, CircleCheck, Cloud, Loader2, Monitor, Server } from "lucide-react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api, keys } from "@/lib/api";
import { localityLabel } from "@/lib/format";
import { useSettings } from "@/lib/queries";

type Draft = { presetId: string | null; name: string; baseUrl: string; apiKey: string; needsKey: boolean };

/** Adds a model source: a local server, one on the network, or a cloud service. */
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
      // First source: start with its first model so the user can chat right away.
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
      <DialogContent className="sm:max-w-[520px]">
        {!draft ? (
          <>
            <DialogHeader>
              <DialogTitle>{only === "cloud" ? "Add a cloud service" : "Add a model source"}</DialogTitle>
              <DialogDescription>
                {only === "cloud"
                  ? "Cloud models are faster and smarter, but your messages are sent to the service you pick."
                  : "Where should your assistant's models run?"}
              </DialogDescription>
            </DialogHeader>
            {only !== "cloud" && (
              <PresetGroup icon={<Monitor />} title="On this computer" presets={local} onPick={pick} />
            )}
            <PresetGroup
              icon={<Cloud />}
              title="Cloud services"
              hint={only === "cloud" ? undefined : "These receive your messages."}
              presets={cloud}
              onPick={pick}
            />
            {only !== "cloud" && (
              <button
                onClick={() => pick(null)}
                className="pressable flex items-center gap-3 rounded-[14px] bg-subtle p-3.5 text-left hover:bg-fill"
              >
                <Server className="size-4 text-muted-foreground" />
                <div>
                  <div className="type-callout font-medium">A server on your network</div>
                  <div className="type-subhead text-muted-foreground">
                    For example a computer with a big graphics card at home.
                  </div>
                </div>
              </button>
            )}
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Button variant="ghost" size="icon-sm" onClick={() => setDraft(null)} aria-label="Back">
                  <ArrowLeft />
                </Button>
                {draft.presetId ? draft.name : "A server on your network"}
              </DialogTitle>
            </DialogHeader>
            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault();
                if (probe) add();
                else test();
              }}
            >
              {!draft.presetId && (
                <Field label="Name">
                  <Input
                    value={draft.name}
                    onChange={(e) => update({ name: e.target.value })}
                    placeholder="Home server"
                  />
                </Field>
              )}
              {!draft.presetId && (
                <Field label="Address">
                  <Input
                    value={draft.baseUrl}
                    onChange={(e) => update({ baseUrl: e.target.value })}
                    className="text-[14px]"
                    autoFocus
                  />
                </Field>
              )}
              {(draft.needsKey || !draft.presetId) && (
                <Field label={draft.needsKey ? "API key" : "API key (optional)"}>
                  <Input
                    type="password"
                    value={draft.apiKey}
                    onChange={(e) => update({ apiKey: e.target.value })}
                    autoFocus={!!draft.presetId}
                    placeholder={draft.needsKey ? "Paste your key" : "Only if the server needs one"}
                  />
                  <p className="type-footnote text-muted-foreground">
                    Stored encrypted on this computer and never shown again.
                  </p>
                </Field>
              )}
              {draft.presetId && !draft.needsKey && (
                <p className="type-callout text-muted-foreground">
                  Make sure {draft.name} is running, then connect.
                </p>
              )}
              {error && <p className="type-subhead text-destructive">{error}</p>}
              {probe && (
                <div className="flex items-center gap-2.5 rounded-[14px] bg-private-soft px-4 py-3 type-callout">
                  <CircleCheck className="size-4 text-private" />
                  <span className="flex-1">
                    Connected · {probe.models.length} model{probe.models.length === 1 ? "" : "s"}
                  </span>
                  <LocalityBadge locality={probe.locality} />
                </div>
              )}
              <DialogFooter>
                <Button type="submit" disabled={busy || (!probe && draft.needsKey && !draft.apiKey.trim())}>
                  {busy && <Loader2 className="animate-spin" />}
                  {probe ? "Add" : "Connect"}
                </Button>
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function PresetGroup({
  icon,
  title,
  hint,
  presets,
  onPick,
}: {
  icon: React.ReactNode;
  title: string;
  hint?: string;
  presets: ProviderPreset[];
  onPick: (p: ProviderPreset) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="section-label flex items-center gap-2 px-0 [&_svg]:size-4">
        {icon}
        {title}
        {hint && <span className="font-normal text-muted-foreground">· {hint}</span>}
      </div>
      <div className="grid grid-cols-3 gap-2">
        {presets.map((p) => (
          <button
            key={p.id}
            onClick={() => onPick(p)}
            className="pressable rounded-[14px] bg-background p-3 text-left shadow-[var(--shadow-card)] transition-shadow hover:shadow-[var(--shadow-raised)]"
          >
            <div className="type-callout font-medium">{p.name}</div>
            <div className="mt-0.5 line-clamp-2 type-footnote text-muted-foreground">{p.description}</div>
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
          <Field label="Address">
            <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} className="text-[14px]" />
          </Field>
          <Field label="API key">
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={provider?.has_api_key ? "Leave empty to keep the saved key" : "None"}
            />
          </Field>
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
