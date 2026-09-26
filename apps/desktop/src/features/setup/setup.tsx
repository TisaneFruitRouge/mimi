import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowRight, Cloud, Flame, Loader2, RefreshCw, ShieldCheck } from "lucide-react";
import { toast } from "sonner";

import type { InstalledModel } from "@/bindings/InstalledModel";
import { Button } from "@/components/ui/button";
import { AddProviderDialog } from "@/features/settings/add-provider-dialog";
import {
  ComputerSummary,
  FitsBadge,
  SuggestedModels,
  useRecommendations,
} from "@/features/settings/computer-panel";
import { api, keys } from "@/lib/api";
import { useSettings } from "@/lib/queries";

/** First run: pick the model the assistant will use. */
export function Setup() {
  const qc = useQueryClient();
  const settings = useSettings().data;
  const rec = useRecommendations();
  const [adding, setAdding] = useState(false);
  const [connecting, setConnecting] = useState<string | null>(null);

  const r = rec.data;
  const newServers = r?.detected_servers.filter((s) => !s.already_added) ?? [];
  const usable = r?.installed.filter((m) => m.fits) ?? [];

  const choose = async (m: InstalledModel) => {
    if (!settings) return;
    try {
      await api.putSettings({ ...settings, default_model: m.model });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  const connect = async (preset_id: string, name: string, base_url: string) => {
    setConnecting(preset_id);
    try {
      await api.addProvider({ name, kind: "openai_compatible", base_url, api_key: null, locality: null });
      await qc.invalidateQueries({ queryKey: keys.recommendations });
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setConnecting(null);
    }
  };

  const cloudOption = (
    <Option
      icon={<Cloud className="size-5 text-amber-600" />}
      title="Use a cloud service"
      description={
        r?.prefer_cloud
          ? "Recommended for this computer. Answers are faster and smarter, but your messages are sent to the service you choose."
          : "Faster and smarter answers, but your messages are sent to the service you choose."
      }
      action={
        <Button variant="outline" onClick={() => setAdding(true)}>
          Choose a service
        </Button>
      }
    />
  );

  return (
    <div className="flex h-screen flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-8 px-6 py-12">
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="rounded-2xl bg-orange-500/10 p-3">
            <Flame className="size-8 text-orange-500" />
          </div>
          <h1 className="text-2xl font-semibold tracking-tight">Welcome to Hearth</h1>
          <p className="max-w-md text-muted-foreground">
            A personal assistant that runs on your computer. Let's pick the model it will use to
            think.
          </p>
        </div>

        <ComputerSummary />

        {rec.isLoading ? (
          <div className="flex justify-center py-6">
            <Loader2 className="size-5 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {r?.prefer_cloud && cloudOption}

            {newServers.map((s) => (
              <Option
                key={s.preset_id}
                icon={<ShieldCheck className="size-5 text-emerald-600" />}
                title={`${s.name} is running on this computer`}
                description={`It has ${s.model_count} model${s.model_count === 1 ? "" : "s"} ready. Connect it to keep everything private.`}
                action={
                  <Button onClick={() => connect(s.preset_id, s.name, s.base_url)} disabled={!!connecting}>
                    {connecting === s.preset_id && <Loader2 className="animate-spin" />} Connect
                  </Button>
                }
              />
            ))}

            {usable.length > 0 && (
              <div className="flex flex-col gap-2 rounded-xl border p-4">
                <div className="flex items-center gap-2">
                  <ShieldCheck className="size-5 text-emerald-600" />
                  <h2 className="font-medium">Private models ready to use</h2>
                </div>
                {usable.map((m, i) => (
                  <button
                    key={`${m.model.provider_id}/${m.model.model}`}
                    onClick={() => choose(m)}
                    className="flex items-center gap-3 rounded-lg border p-3 text-left transition-colors hover:bg-accent"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium">
                        {m.model.model}
                        {i === 0 && (
                          <span className="ml-2 rounded-full bg-orange-500/10 px-2 py-0.5 text-xs text-orange-700 dark:text-orange-400">
                            Recommended
                          </span>
                        )}
                      </div>
                      <div className="text-xs text-muted-foreground">via {m.provider_name}</div>
                    </div>
                    <FitsBadge fits />
                    <ArrowRight className="size-4 text-muted-foreground" />
                  </button>
                ))}
              </div>
            )}

            {usable.length === 0 && newServers.length === 0 && (
              <div className="flex flex-col gap-3 rounded-xl border p-4">
                <div className="flex items-center gap-2">
                  <ShieldCheck className="size-5 text-emerald-600" />
                  <h2 className="font-medium">Run models privately on this computer</h2>
                </div>
                <p className="text-sm text-muted-foreground">
                  Install Ollama, then download one of these models. Hearth will find it
                  automatically.
                </p>
                <SuggestedModels models={r?.suggested ?? []} />
                <div className="flex gap-2">
                  <Button onClick={() => openUrl("https://ollama.com/download")}>Get Ollama</Button>
                  <Button variant="outline" onClick={() => rec.refetch()} disabled={rec.isFetching}>
                    <RefreshCw className={rec.isFetching ? "animate-spin" : ""} /> Check again
                  </Button>
                </div>
              </div>
            )}

            {!r?.prefer_cloud && cloudOption}

            <button
              onClick={() => setAdding(true)}
              className="text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
            >
              Connect to a server on your network instead
            </button>
          </div>
        )}
      </div>
      <AddProviderDialog open={adding} onOpenChange={setAdding} />
    </div>
  );
}

function Option({
  icon,
  title,
  description,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  action: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 rounded-xl border p-4">
      {icon}
      <div className="min-w-0 flex-1">
        <h2 className="font-medium">{title}</h2>
        <p className="text-sm text-muted-foreground">{description}</p>
      </div>
      {action}
    </div>
  );
}
