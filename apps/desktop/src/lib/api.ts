import type { Conversation } from "@/bindings/Conversation";
import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { CatalogModel } from "@/bindings/CatalogModel";
import type { Connection } from "@/bindings/Connection";
import type { ConnectionSetup } from "@/bindings/ConnectionSetup";
import type { HardwareInfo } from "@/bindings/HardwareInfo";
import type { DuplicateSuggestion } from "@/bindings/DuplicateSuggestion";
import type { Integration } from "@/bindings/Integration";
import type { MemoryNote } from "@/bindings/MemoryNote";
import type { MemoryOverview } from "@/bindings/MemoryOverview";
import type { MentionCandidate } from "@/bindings/MentionCandidate";
import type { NewHandle } from "@/bindings/NewHandle";
import type { NewPerson } from "@/bindings/NewPerson";
import type { Person } from "@/bindings/Person";
import type { PersonSummary } from "@/bindings/PersonSummary";
import type { PersonUpdate } from "@/bindings/PersonUpdate";
import type { ModelPull } from "@/bindings/ModelPull";
import type { RuntimeStatus } from "@/bindings/RuntimeStatus";
import type { ModelInfo } from "@/bindings/ModelInfo";
import type { NewProvider } from "@/bindings/NewProvider";
import type { ProbeResult } from "@/bindings/ProbeResult";
import type { Provider } from "@/bindings/Provider";
import type { ProviderPreset } from "@/bindings/ProviderPreset";
import type { ProviderUpdate } from "@/bindings/ProviderUpdate";
import type { Recommendations } from "@/bindings/Recommendations";
import type { SendMessage } from "@/bindings/SendMessage";
import type { SendMessageResult } from "@/bindings/SendMessageResult";
import type { Delivery } from "@/bindings/Delivery";
import type { NewScheduleItem } from "@/bindings/NewScheduleItem";
import type { ScheduleItem } from "@/bindings/ScheduleItem";
import type { ScheduleUpdate } from "@/bindings/ScheduleUpdate";
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
  cancelPull: (providerId: string, model: string) =>
    call<null>("POST", `/providers/${providerId}/pull/cancel`, { model }),
  deleteModel: (providerId: string, model: string) =>
    call<null>("DELETE", `/providers/${providerId}/models/${encodeURIComponent(model)}`),
  runtime: () => call<RuntimeStatus>("GET", "/runtime"),
  catalog: () => call<CatalogModel[]>("GET", "/catalog"),
  integrations: () => call<Integration[]>("GET", "/integrations"),
  connections: () => call<Connection[]>("GET", "/connections"),
  connect: (setup: ConnectionSetup) => call<Connection>("POST", "/connections", setup),
  disconnect: (id: string) => call<null>("DELETE", `/connections/${id}`),

  people: (q = "") => call<PersonSummary[]>("GET", `/people?q=${encodeURIComponent(q)}`),
  person: (id: string) => call<Person>("GET", `/people/${id}`),
  addPerson: (p: NewPerson) => call<Person>("POST", "/people", p),
  updatePerson: (id: string, u: PersonUpdate) => call<Person>("PATCH", `/people/${id}`, u),
  removePerson: (id: string) => call<null>("DELETE", `/people/${id}`),
  addHandle: (id: string, h: NewHandle) => call<Person>("POST", `/people/${id}/handles`, h),
  removeHandle: (id: string, handle: string) =>
    call<Person>("DELETE", `/people/${id}/handles/${handle}`),
  mergePeople: (keep: string, other: string) =>
    call<Person>("POST", `/people/${keep}/merge`, { other }),
  splitPerson: (id: string, source_id: string, record: string) =>
    call<Person>("POST", `/people/${id}/split`, { source_id, record }),
  duplicates: () => call<DuplicateSuggestion[]>("GET", "/people/duplicates"),
  dismissDuplicate: (a: string, b: string) =>
    call<null>("POST", "/people/duplicates/dismiss", { a, b }),
  syncPeople: () => call<null>("POST", "/people/sync"),
  mentions: (q: string) => call<MentionCandidate[]>("GET", `/mentions?q=${encodeURIComponent(q)}`),

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

  memory: () => call<MemoryOverview>("GET", "/memory"),
  memoryNote: (path: string) =>
    call<MemoryNote>("GET", `/memory/note?path=${encodeURIComponent(path)}`),
  saveMemoryNote: (path: string, body: string, title?: string) =>
    call<MemoryNote>("PUT", `/memory/note?path=${encodeURIComponent(path)}`, {
      body,
      title: title ?? null,
    }),
  deleteMemoryNote: (path: string) =>
    call<null>("DELETE", `/memory/note?path=${encodeURIComponent(path)}`),
  saveMemoryProfile: (text: string) => call<null>("PUT", "/memory/profile", { text }),
  setMemoryLearning: (learning: boolean) => call<null>("PUT", "/memory/learning", { learning }),
  undoMemory: (revision: number) => call<null>("POST", `/memory/undo/${revision}`),
  forgetEverything: () => call<null>("POST", "/memory/forget-all"),

  schedule: () => call<ScheduleItem[]>("GET", "/schedule"),
  addSchedule: (item: NewScheduleItem) => call<ScheduleItem>("POST", "/schedule", item),
  /** Only the fields given change. */
  updateSchedule: (id: string, update: Partial<ScheduleUpdate>) =>
    call<ScheduleItem>("PATCH", `/schedule/${id}`, {
      title: null,
      instruction: null,
      schedule: null,
      paused: null,
      ...update,
    } satisfies ScheduleUpdate),
  deleteSchedule: (id: string) => call<null>("DELETE", `/schedule/${id}`),
  runRoutine: (id: string) => call<null>("POST", `/schedule/${id}/run`),
  deliveries: () => call<Delivery[]>("GET", "/schedule/deliveries?limit=30"),
  reminderDone: (deliveryId: string) =>
    call<null>("POST", `/schedule/deliveries/${deliveryId}/done`),
  snoozeReminder: (deliveryId: string, minutes: number) =>
    call<null>("POST", `/schedule/deliveries/${deliveryId}/snooze`, { minutes }),
  undoSchedule: (revision: number) => call<null>("POST", `/schedule/undo/${revision}`),
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
  runtime: ["runtime"] as const,
  catalog: ["catalog"] as const,
  integrations: ["integrations"] as const,
  connections: ["connections"] as const,
  /** Everything about people: lists, details, duplicates, @ suggestions. */
  people: ["people"] as const,
  peopleList: (q: string) => ["people", "list", q] as const,
  person: (id: string) => ["people", "person", id] as const,
  duplicates: ["people", "duplicates"] as const,
  mentions: (q: string) => ["people", "mentions", q] as const,
  conversations: ["conversations"] as const,
  conversation: (id: string) => ["conversation", id] as const,
  memory: ["memory"] as const,
  memoryNote: (path: string) => ["memory", "note", path] as const,
  /** Reminders and routines, and their recent deliveries. */
  schedule: ["schedule"] as const,
  deliveries: ["schedule", "deliveries"] as const,
};
