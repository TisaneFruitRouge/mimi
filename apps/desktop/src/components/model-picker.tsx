import { Check, ChevronDown, Settings2 } from "lucide-react";
import { toast } from "sonner";

import { LocalityBadge } from "@/components/locality-badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { api } from "@/lib/api";
import { sameModel, useAllModels, useProviders, useSettings } from "@/lib/queries";

/** Chooses the default model. Always shows where the current one runs. */
export function ModelPicker({ onManage }: { onManage: () => void }) {
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const { options, failed } = useAllModels();
  const current = settings?.default_model;
  const currentProvider = providers.find((p) => p.id === current?.provider_id);

  const choose = async (ref: (typeof options)[number]["ref"]) => {
    if (!settings) return;
    try {
      await api.putSettings({ ...settings, default_model: ref });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-2 font-normal">
          <span className="max-w-48 truncate">{current?.model ?? "Choose a model"}</span>
          {currentProvider && <LocalityBadge locality={currentProvider.locality} compact />}
          <ChevronDown className="text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        {providers.map((p) => {
          const models = options.filter((o) => o.ref.provider_id === p.id);
          return (
            <DropdownMenuGroup key={p.id}>
              <DropdownMenuLabel className="flex items-center justify-between gap-2">
                <span className="truncate">{p.name}</span>
                <LocalityBadge locality={p.locality} />
              </DropdownMenuLabel>
              {models.map((o) => (
                <DropdownMenuItem key={o.ref.model} onSelect={() => choose(o.ref)}>
                  <span className="truncate">{o.ref.model}</span>
                  {sameModel(o.ref, current) && <Check className="ml-auto" />}
                </DropdownMenuItem>
              ))}
              {failed.some((f) => f.id === p.id) && (
                <DropdownMenuItem disabled>Unavailable right now</DropdownMenuItem>
              )}
              <DropdownMenuSeparator />
            </DropdownMenuGroup>
          );
        })}
        <DropdownMenuItem onSelect={onManage}>
          <Settings2 /> Manage models…
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
