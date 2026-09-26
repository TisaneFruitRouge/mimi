import { useQueries, useQuery } from "@tanstack/react-query";

import type { Locality } from "@/bindings/Locality";
import type { ModelRef } from "@/bindings/ModelRef";
import { api, keys } from "@/lib/api";

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settings });
export const useProviders = () => useQuery({ queryKey: keys.providers, queryFn: api.providers });
export const useConversations = () =>
  useQuery({ queryKey: keys.conversations, queryFn: api.conversations });

export interface ModelOption {
  ref: ModelRef;
  providerName: string;
  locality: Locality;
}

/** Every model from every configured provider, grouped by provider order. */
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
    })),
  );
  const failed = providers.filter((_, i) => results[i]?.isError);
  return { options, failed, loading: results.some((r) => r.isLoading) };
}

export const sameModel = (a: ModelRef | null | undefined, b: ModelRef | null | undefined) =>
  !!a && !!b && a.provider_id === b.provider_id && a.model === b.model;
