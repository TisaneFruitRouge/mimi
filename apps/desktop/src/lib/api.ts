import type { Conversation } from "@/bindings/Conversation";
import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { CatalogModel } from "@/bindings/CatalogModel";
import type { Connection } from "@/bindings/Connection";
import type { ConnectionSetup } from "@/bindings/ConnectionSetup";
import type { HardwareInfo } from "@/bindings/HardwareInfo";
import type { Integration } from "@/bindings/Integration";
import type { ModelPull } from "@/bindings/ModelPull";
import type { ModelInfo } from "@/bindings/ModelInfo";
import type { NewProvider } from "@/bindings/NewProvider";
import type { ProbeResult } from "@/bindings/ProbeResult";
import type { Provider } from "@/bindings/Provider";
import type { ProviderPreset } from "@/bindings/ProviderPreset";
import type { ProviderUpdate } from "@/bindings/ProviderUpdate";
import type { Recommendations } from "@/bindings/Recommendations";
import type { SendMessage } from "@/bindings/SendMessage";
import type { SendMessageResult } from "@/bindings/SendMessageResult";
import type { Settings } from "@/bindings/Settings";
import type { Status } from "@/bindings/Status";
import { type TransportError, request } from "@/lib/transport";

export class DaemonError extends Error {
  readonly kind: TransportError["kind"];
  readonly code?: string;

  constructor(err: TransportError) {
    super(
      err.kind === "not_running"
        ? "The assistant isn't running."
        : err.kind === "unauthorized"
          ? "This browser isn't signed in to the assistant."
          : err.message,
    );
    this.kind = err.kind;
    this.code = err.kind === "api" ? err.code : undefined;
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  try {
    return await request<T>(method, path, body);
  } catch (err) {
    throw new DaemonError(err as TransportError);
  }
}

export const api = {
  status: () => call<Status>("GET", "/status"),
  settings: () => call<Settings>("GET", "/settings"),
  putSettings: (s: Settings) => call<Settings>("PUT", "/settings", s),

  providers: () => call<Provider[]>("GET", "/providers"),
  presets: () => call<ProviderPreset[]>("GET", "/providers/presets"),
  probe: (base_url: string, api_key: string | null) =>
    call<ProbeResult>("POST", "/providers/probe", { base_url, api_key }),
  addProvider: (p: NewProvider) => call<Provider>("POST", "/providers", p),
  updateProvider: (id: string, u: ProviderUpdate) =>
    call<Provider>("PATCH", `/providers/${id}`, u),
  removeProvider: (id: string) => call<null>("DELETE", `/providers/${id}`),
  models: (providerId: string) => call<ModelInfo[]>("GET", `/providers/${providerId}/models`),

  pull: (providerId: string, model: string) =>
    call<ModelPull>("POST", `/providers/${providerId}/pull`, { model }),
  pulls: () => call<ModelPull[]>("GET", "/pulls"),
  catalog: () => call<CatalogModel[]>("GET", "/catalog"),
  integrations: () => call<Integration[]>("GET", "/integrations"),
  connections: () => call<Connection[]>("GET", "/connections"),
  connect: (setup: ConnectionSetup) => call<Connection>("POST", "/connections", setup),
  disconnect: (id: string) => call<null>("DELETE", `/connections/${id}`),

  hardware: () => call<HardwareInfo>("GET", "/hardware"),
  recommendations: () => call<Recommendations>("GET", "/recommendations"),

  conversations: () => call<Conversation[]>("GET", "/conversations"),
  createConversation: () => call<Conversation>("POST", "/conversations", {}),
  conversation: (id: string) => call<ConversationDetail>("GET", `/conversations/${id}`),
  renameConversation: (id: string, title: string) =>
    call<Conversation>("PATCH", `/conversations/${id}`, { title }),
  deleteConversation: (id: string) => call<null>("DELETE", `/conversations/${id}`),
  send: (id: string, msg: SendMessage) =>
    call<SendMessageResult>("POST", `/conversations/${id}/messages`, msg),
  cancel: (id: string) => call<null>("POST", `/conversations/${id}/cancel`),
  approveAction: (id: string, args?: Record<string, unknown>) =>
    call<null>("POST", `/actions/${id}/approve`, { arguments: args ?? null }),
  rejectAction: (id: string) => call<null>("POST", `/actions/${id}/reject`),
};

/** Query keys, shared by queries and the event sync so they stay in step. */
export const keys = {
  settings: ["settings"] as const,
  status: ["status"] as const,
  providers: ["providers"] as const,
  presets: ["presets"] as const,
  models: (providerId: string) => ["models", providerId] as const,
  allModels: ["models"] as const,
  recommendations: ["recommendations"] as const,
  pulls: ["pulls"] as const,
  catalog: ["catalog"] as const,
  integrations: ["integrations"] as const,
  connections: ["connections"] as const,
  conversations: ["conversations"] as const,
  conversation: (id: string) => ["conversation", id] as const,
};
