import { useQueries, useQuery } from "@tanstack/react-query";

import type { Locality } from "@/bindings/Locality";
import type { ModelPrice } from "@/bindings/ModelPrice";
import type { ModelRef } from "@/bindings/ModelRef";
import { api, keys } from "@/lib/api";

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settings });
export const useUpdates = () => useQuery({ queryKey: keys.updates, queryFn: api.updates });
/** Speech recognition and voices: what's here, what's downloading. */
export const useVoice = () => useQuery({ queryKey: keys.voice, queryFn: api.voice });
/** What the user calls their assistant, e.g. for "Ask Mimi about this". */
export const useAssistantName = () => useSettings().data?.assistant_name || "Mimi";
export const useProviders =() => useQuery({ queryKey: keys.providers, queryFn: api.providers });
export const useConversations = () =>
  useQuery({ queryKey: keys.conversations, queryFn: api.conversations });
export const useRecommendations = () =>
  useQuery({ queryKey: keys.recommendations, queryFn: api.recommendations, staleTime: 30_000 });
export const usePulls = () => useQuery({ queryKey: keys.pulls, queryFn: api.pulls });
export const useRuntime = () => useQuery({ queryKey: keys.runtime, queryFn: api.runtime });
export const useIntegrations = () =>
  useQuery({ queryKey: keys.integrations, queryFn: api.integrations });
export const useConnections = () =>
  useQuery({ queryKey: keys.connections, queryFn: api.connections });
const useCatalog = () =>
  useQuery({ queryKey: keys.catalog, queryFn: api.catalog, staleTime: Infinity });

export interface ModelOption {
  ref: ModelRef;
  /** The source's own name for the model, e.g. "Claude Sonnet 5". */
  name: string | null;
  providerName: string;
  locality: Locality;
  sizeBytes: number | null;
  /** Whether it can use tools, when the source says. */
  supportsTools: boolean | null;
  /** Whether it can see photos, when the source says. */
  seesImages: boolean | null;
  price: ModelPrice | null;
}

/** Every model from every configured model source, in source order. */
export function useAllModels() {
  const providers = useProviders().data ?? [];
  const results = useQueries({
    queries: providers.map((p) => ({
      queryKey: keys.models(p.id),
      queryFn: () => api.models(p.id),
      staleTime: 60_000,
      retry: false,
    })),
  });
  const options: ModelOption[] = providers.flatMap((p, i) =>
    (results[i]?.data ?? []).map((m) => ({
      ref: { provider_id: p.id, model: m.id },
      name: m.name,
      providerName: p.name,
      locality: p.locality,
      sizeBytes: m.size_bytes,
      supportsTools: m.supports_tools,
      seesImages: m.sees_images,
      price: m.price,
    })),
  );
  const failed = providers.filter((_, i) => results[i]?.isError);
  return { options, failed, loading: results.some((r) => r.isLoading) };
}

export const sameModel = (a: ModelRef | null | undefined, b: ModelRef | null | undefined) =>
  !!a && !!b && a.provider_id === b.provider_id && a.model === b.model;

/**
 * Friendly name and description for a model id: from the catalog when it's known, else
 * the name its source gave it, else a tidied id.
 */
export function useModelInfo() {
  const catalog = useCatalog().data ?? [];
  return (id: string, sourceName?: string | null) => {
    const known = catalog.find((m) => m.id === id || m.id === id.replace(/:latest$/, ""));
    // OpenRouter names say who made the model: "DeepSeek: DeepSeek V4 Flash".
    const [maker, name] = sourceName?.includes(": ")
      ? [sourceName.slice(0, sourceName.indexOf(": ")), sourceName.slice(sourceName.indexOf(": ") + 2)]
      : [null, sourceName];
    return {
      name: known?.name ?? name ?? prettify(id),
      maker,
      description: known?.description ?? null,
    };
  };
}

function prettify(id: string) {
  // "llama3.2:3b" -> "llama3.2 3B"; unknown ids stay recognizable.
  const [base, tag] = id.split("/").pop()!.split(":");
  return tag && tag !== "latest" ? `${base} ${tag.toUpperCase()}` : base;
}

/** The active model with where it runs, or null before setup. */
export function useActiveModel() {
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const info = useModelInfo();
  const ref = settings?.default_model ?? null;
  const provider = providers.find((p) => p.id === ref?.provider_id) ?? null;
  const models = useQuery({
    queryKey: keys.models(ref?.provider_id ?? ""),
    queryFn: () => api.models(ref!.provider_id),
    enabled: !!ref,
    staleTime: 60_000,
    retry: false,
  }).data;
  if (!ref || !provider) return null;
  const sourceName = models?.find((m) => m.id === ref.model)?.name;
  return { ref, provider, ...info(ref.model, sourceName) };
}

/** Whether a model can see photos; `null` without one or while that isn't known yet. */
function useModelSees(ref: ModelRef | null): boolean | null {
  const vision = useQuery({
    queryKey: keys.vision(ref?.provider_id ?? "", ref?.model ?? ""),
    queryFn: () => api.vision(ref!.provider_id, ref!.model),
    enabled: !!ref,
    staleTime: 5 * 60_000,
    retry: false,
  });
  if (!ref) return null;
  return vision.data?.sees_images ?? null;
}

/** Whether the default model itself can see photos; `null` while that isn't known yet. */
export function useDefaultSeesImages(): boolean | null {
  return useModelSees(useSettings().data?.default_model ?? null);
}

/**
 * Whether photos sent now are seen: by the default model, or else by the model for
 * photos. `null` while that isn't known yet.
 */
export function useSeesImages(): boolean | null {
  const settings = useSettings().data;
  const byDefault = useModelSees(settings?.default_model ?? null);
  const byPhotoModel = useModelSees(byDefault === false ? (settings?.photo_model ?? null) : null);
  if (byDefault !== false) return byDefault;
  return settings?.photo_model ? byPhotoModel : false;
}
