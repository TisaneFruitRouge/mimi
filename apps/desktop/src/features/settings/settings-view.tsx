import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { KeyRound, Lock, Plus, Trash2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Locality } from "@/bindings/Locality";
import type { Provider } from "@/bindings/Provider";
import { LocalityBadge } from "@/components/locality-badge";
import { ModelPicker } from "@/components/model-picker";
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
  Dialog,
  DialogContent,
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
import { AddProviderDialog } from "@/features/settings/add-provider-dialog";
import {
  ComputerSummary,
  FitsBadge,
  SuggestedModels,
  useRecommendations,
} from "@/features/settings/computer-panel";
import { api, keys } from "@/lib/api";
import { localityExplanation, localityLabel } from "@/lib/format";
import { useProviders, useSettings } from "@/lib/queries";

const sections = ["Models", "This computer", "General", "Privacy"] as const;
type Section = (typeof sections)[number];

export function SettingsView() {
  const [section, setSection] = useState<Section>("Models");
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b px-6">
        <h1 className="text-sm font-medium">Settings</h1>
      </header>
      <div className="flex min-h-0 flex-1">
        <nav className="flex w-44 shrink-0 flex-col gap-0.5 p-3">
          {sections.map((s) => (
            <button
              key={s}
              onClick={() => setSection(s)}
              className={cn(
                "rounded-md px-3 py-1.5 text-left text-sm hover:bg-accent",
                s === section && "bg-accent font-medium",
              )}
            >
              {s}
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-2xl flex-col gap-8 px-6 py-6">
            {section === "Models" && <ModelsSection />}
            {section === "This computer" && <ComputerSection />}
            {section === "General" && <GeneralSection />}
            {section === "Privacy" && <PrivacySection />}
          </div>
        </div>
      </div>
    </div>
  );
}

function SectionTitle({ title, description }: { title: string; description?: string }) {
  return (
    <div>
      <h2 className="text-base font-semibold">{title}</h2>
      {description && <p className="text-sm text-muted-foreground">{description}</p>}
    </div>
  );
}

function ModelsSection() {
  const providers = useProviders().data ?? [];
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [removing, setRemoving] = useState<Provider | null>(null);

  return (
    <>
      <section className="flex flex-col gap-3">
        <SectionTitle title="Default model" description="Used for new messages." />
        <div>
          <ModelPicker onManage={() => setAdding(true)} />
        </div>
      </section>
      <section className="flex flex-col gap-3">
        <div className="flex items-end justify-between">
          <SectionTitle title="Providers" description="Where your models run." />
          <Button size="sm" onClick={() => setAdding(true)}>
            <Plus /> Add
          </Button>
        </div>
        <div className="flex flex-col divide-y rounded-xl border">
          {providers.map((p) => (
            <div key={p.id} className="flex items-center gap-3 p-3">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-sm font-medium">
                  {p.name}
                  <LocalityBadge locality={p.locality} />
                </div>
                <div className="flex items-center gap-2 truncate font-mono text-xs text-muted-foreground">
                  {p.base_url}
                  {p.has_api_key && (
                    <span className="inline-flex items-center gap-1 font-sans">
                      <KeyRound className="size-3" /> key saved
                    </span>
                  )}
                </div>
              </div>
              <Button variant="ghost" size="sm" onClick={() => setEditing(p)}>
                Edit
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setRemoving(p)}
                aria-label={`Remove ${p.name}`}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
          {providers.length === 0 && (
            <p className="p-4 text-sm text-muted-foreground">No providers yet.</p>
          )}
        </div>
      </section>

      <AddProviderDialog open={adding} onOpenChange={setAdding} />
      <EditProviderDialog provider={editing} onClose={() => setEditing(null)} />
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
              onClick={() =>
                removing && api.removeProvider(removing.id).catch((e) => toast.error(e.message))
              }
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function EditProviderDialog({ provider, onClose }: { provider: Provider | null; onClose: () => void }) {
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

  const save = async () => {
    if (!provider) return;
    try {
      await api.updateProvider(provider.id, {
        name,
        base_url: baseUrl === provider.base_url ? null : baseUrl,
        api_key: apiKey ? apiKey : null,
        locality,
      });
      onClose();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Dialog open={!!provider} onOpenChange={(o) => !o && (onClose(), setLoadedId(null))}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit {provider?.name}</DialogTitle>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>Address</Label>
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              className="font-mono text-xs"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>API key</Label>
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={provider?.has_api_key ? "Leave empty to keep the saved key" : "None"}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>Where it runs</Label>
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
            <p className="text-xs text-muted-foreground">
              Detected from the address. Change it only if you know better, e.g. for your own
              server behind a public domain.
            </p>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
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

function ComputerSection() {
  const rec = useRecommendations().data;
  return (
    <>
      <section className="flex flex-col gap-3">
        <SectionTitle title="This computer" />
        <ComputerSummary />
      </section>
      {rec && rec.installed.length > 0 && (
        <section className="flex flex-col gap-3">
          <SectionTitle title="Models you have" />
          <div className="flex flex-col divide-y rounded-xl border">
            {rec.installed.map((m) => (
              <div key={`${m.model.provider_id}/${m.model.model}`} className="flex items-center gap-3 p-3 text-sm">
                <span className="flex-1 font-medium">{m.model.model}</span>
                <span className="text-xs text-muted-foreground">{m.provider_name}</span>
                <FitsBadge fits={m.fits} />
              </div>
            ))}
          </div>
        </section>
      )}
      {rec && rec.suggested.length > 0 && (
        <section className="flex flex-col gap-3">
          <SectionTitle
            title="Worth downloading"
            description="The best models for this computer. Download them with Ollama."
          />
          <SuggestedModels models={rec.suggested} />
        </section>
      )}
    </>
  );
}

function GeneralSection() {
  const settings = useSettings().data;
  const [name, setName] = useState<string | null>(null);
  const value = name ?? settings?.assistant_name ?? "";
  const save = async () => {
    if (!settings) return;
    try {
      await api.putSettings({ ...settings, assistant_name: value });
      setName(null);
      toast.success("Saved");
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  return (
    <section className="flex flex-col gap-3">
      <SectionTitle title="Assistant" />
      <form
        className="flex max-w-sm flex-col gap-1.5"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <Label htmlFor="assistant-name">Name</Label>
        <div className="flex gap-2">
          <Input id="assistant-name" value={value} onChange={(e) => setName(e.target.value)} />
          <Button type="submit" disabled={name === null || !value.trim()}>
            Save
          </Button>
        </div>
      </form>
    </section>
  );
}

function PrivacySection() {
  const status = useQuery({ queryKey: keys.status, queryFn: api.status }).data;
  const providers = useProviders().data ?? [];
  return (
    <>
      <section className="flex flex-col gap-3">
        <SectionTitle title="Your data" />
        <div className="flex flex-col gap-2 rounded-xl border p-4 text-sm">
          <p className="flex items-start gap-2">
            <Lock className="mt-0.5 size-4 shrink-0 text-emerald-600" />
            <span>
              Conversations, settings and API keys are stored encrypted on this computer.
              {status?.key_storage === "keychain"
                ? " The encryption key is kept in your system keychain."
                : " No system keychain was found, so the encryption key is kept in a file only your user account can read."}
            </span>
          </p>
          {status && (
            <p className="font-mono text-xs break-all text-muted-foreground">{status.data_dir}</p>
          )}
        </div>
      </section>
      <section className="flex flex-col gap-3">
        <SectionTitle
          title="Where your messages go"
          description="Each message is sent only to the provider of the model that answers it."
        />
        <div className="flex flex-col divide-y rounded-xl border">
          {providers.map((p) => (
            <div key={p.id} className="flex items-center gap-3 p-3 text-sm">
              <span className="w-32 shrink-0 truncate font-medium">{p.name}</span>
              <span className="flex-1 text-muted-foreground">{localityExplanation[p.locality]}</span>
              <LocalityBadge locality={p.locality} compact />
            </div>
          ))}
          {providers.length === 0 && (
            <p className="p-3 text-sm text-muted-foreground">No providers configured.</p>
          )}
        </div>
      </section>
    </>
  );
}
