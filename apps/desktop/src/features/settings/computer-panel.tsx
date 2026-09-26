import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Cpu, MemoryStick, MonitorSmartphone } from "lucide-react";
import { toast } from "sonner";

import type { CatalogModel } from "@/bindings/CatalogModel";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { api, keys } from "@/lib/api";
import { formatBytes } from "@/lib/format";

export const useRecommendations = () =>
  useQuery({ queryKey: keys.recommendations, queryFn: api.recommendations, staleTime: 30_000 });

/** What this computer is and what it can run. */
export function ComputerSummary() {
  const rec = useRecommendations();
  if (rec.isLoading) return <Skeleton className="h-28 w-full rounded-xl" />;
  if (!rec.data) return null;
  const hw = rec.data.hardware;
  const gpu = hw.gpus.find((g) => g.kind === "discrete") ?? hw.gpus[0];
  return (
    <div className="flex flex-col gap-3 rounded-xl border p-4">
      <div className="flex flex-wrap gap-x-5 gap-y-1 text-sm text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <Cpu className="size-4" /> {hw.cpu_name || hw.arch}
        </span>
        <span className="flex items-center gap-1.5">
          <MemoryStick className="size-4" /> {Math.round(hw.total_memory_bytes / 2 ** 30)} GB memory
        </span>
        {gpu && (
          <span className="flex items-center gap-1.5">
            <MonitorSmartphone className="size-4" /> {gpu.name}
            {gpu.vram_bytes ? ` · ${Math.round(gpu.vram_bytes / 2 ** 30)} GB` : ""}
          </span>
        )}
      </div>
      <p className="text-sm leading-relaxed">{rec.data.summary}</p>
    </div>
  );
}

/** Models worth downloading, with the command to get them through Ollama. */
export function SuggestedModels({ models }: { models: CatalogModel[] }) {
  if (models.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {models.map((m) => (
        <div key={m.id} className="flex items-center gap-3 rounded-lg border p-3">
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium">
              {m.name}{" "}
              <span className="font-normal text-muted-foreground">
                · {formatBytes(m.download_bytes)} download
              </span>
            </div>
            <div className="text-xs text-muted-foreground">{m.description}</div>
          </div>
          <CopyCommand command={`ollama pull ${m.id}`} />
        </div>
      ))}
    </div>
  );
}

function CopyCommand({ command }: { command: string }) {
  return (
    <Button
      variant="outline"
      size="sm"
      className="font-mono text-xs"
      onClick={() =>
        navigator.clipboard
          .writeText(command)
          .then(() => toast.success("Copied. Paste it in a terminal to download the model."))
      }
    >
      <Copy /> {command}
    </Button>
  );
}

export function FitsBadge({ fits }: { fits: boolean }) {
  return fits ? (
    <span className="inline-flex items-center gap-1 text-xs text-emerald-700 dark:text-emerald-400">
      <Check className="size-3" /> Runs well here
    </span>
  ) : (
    <span className="text-xs text-amber-700 dark:text-amber-400">Too large for this computer</span>
  );
}
