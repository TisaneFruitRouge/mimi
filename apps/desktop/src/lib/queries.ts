import { useQueries, useQuery } from "@tanstack/react-query";

import type { Locality } from "@/bindings/Locality";
import type { ModelRef } from "@/bindings/ModelRef";
import { api, keys } from "@/lib/api";

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settings });
export const useProviders = () => useQuery({ queryKey: keys.providers, queryFn: api.providers });
export const useConversations = () =>
  useQuery({ queryKey: keys.conversations, queryFn: api.conversations });
export const useRecommendations = () =>
  useQuery({ queryKey: keys.recommendations, queryFn: api.recommendations, staleTime: 30_000 });
export const usePulls = () => useQuery({ queryKey: keys.pulls, queryFn: api.pulls });
export const useIntegrations = () =>
  useQuery({ queryKey: keys.integrations, queryFn: api.integrations });
export const useConnections = () =>
  useQuery({ queryKey: keys.connections, queryFn: api.connections });
const useCatalog = () =>
  useQuery({ queryKey: keys.catalog, queryFn: api.catalog, staleTime: Infinity });

export interface ModelOption {
  ref: ModelRef;
  providerName: string;
  locality: Locality;
  sizeBytes: number | null;
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
      providerName: p.name,
      locality: p.locality,
      sizeBytes: m.size_bytes,
    })),
  );
  const failed = providers.filter((_, i) => results[i]?.isError);
  return { options, failed, loading: results.some((r) => r.isLoading) };
}

export const sameModel = (a: ModelRef | null | undefined, b: ModelRef | null | undefined) =>
  !!a && !!b && a.provider_id === b.provider_id && a.model === b.model;

/** Friendly name and description for a model id, from the catalog when it's known. */
export function useModelInfo() {
  const catalog = useCatalog().data ?? [];
  return (id: string) => {
    const known = catalog.find((m) => m.id === id || m.id === id.replace(/:latest$/, ""));
    return {
      name: known?.name ?? prettify(id),
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
  if (!ref || !provider) return null;
  return { ref, provider, ...info(ref.model) };
}
