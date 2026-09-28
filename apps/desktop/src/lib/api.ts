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
import type { MemorySemantic } from "@/bindings/MemorySemantic";
import type { MentionCandidate } from "@/bindings/MentionCandidate";
import type { NewHandle } from "@/bindings/NewHandle";
import type { NewPerson } from "@/bindings/NewPerson";
import type { Person } from "@/bindings/Person";
import type { PersonSummary } from "@/bindings/PersonSummary";
import type { PersonUpdate } from "@/bindings/PersonUpdate";
import type { RemovedPerson } from "@/bindings/RemovedPerson";
import type { ModelPull } from "@/bindings/ModelPull";
import type { RuntimeStatus } from "@/bindings/RuntimeStatus";
import type { ModelInfo } from "@/bindings/ModelInfo";
import type { NewProvider } from "@/bindings/NewProvider";
import type { ProbeRequest } from "@/bindings/ProbeRequest";
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
import type { ScheduleOccurrence } from "@/bindings/ScheduleOccurrence";
import type { ScheduleUpdate } from "@/bindings/ScheduleUpdate";
import type { Settings } from "@/bindings/Settings";
import type { Status } from "@/bindings/Status";
import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { MailBox } from "@/bindings/MailBox";
import type { MailContent } from "@/bindings/MailContent";
import type { MailDraft } from "@/bindings/MailDraft";
import type { MailOverview } from "@/bindings/MailOverview";
import type { MailDiscovery } from "@/bindings/MailDiscovery";
import type { MailFolderInput } from "@/bindings/MailFolderInput";
import type { MailPreset } from "@/bindings/MailPreset";
import type { MailSummary } from "@/bindings/MailSummary";
import type { MailThread } from "@/bindings/MailThread";
import type { MailThreadDetail } from "@/bindings/MailThreadDetail";
import type { CalendarEvents } from "@/bindings/CalendarEvents";
import type { CalendarInfo } from "@/bindings/CalendarInfo";
import type { CreatedEvent } from "@/bindings/CreatedEvent";
import type { NewCalendarEvent } from "@/bindings/NewCalendarEvent";
import type { PersonConversation } from "@/bindings/PersonConversation";
import type { GoogleSignIn } from "@/bindings/GoogleSignIn";
import type { GoogleSignInInfo } from "@/bindings/GoogleSignInInfo";
import type { GoogleSignInStatus } from "@/bindings/GoogleSignInStatus";
import type { KindPermission } from "@/bindings/KindPermission";
import type { PermissionKind } from "@/bindings/PermissionKind";
import { type TransportError, request } from "@/lib/transport";

/**
 * The daemon didn't understand the request: a route it doesn't have, or a body it
 * can't read. Both come from an older daemon still running after an update.
 */
function isOutdated(err: TransportError) {
  return (
    err.kind === "api" &&
    (err.code === "unknown" || (err.code === "not_found" && err.message === "Route not found"))
  );
}

/** Part of the mail: one account, or one address mail arrived at. Empty is all of it. */
export interface MailScope {
  account?: string;
  address?: string;
}

function scopeParams(scope: MailScope) {
  const params = new URLSearchParams();
  if (scope.account) params.set("account", scope.account);
  if (scope.address) params.set("address", scope.address);
  return params;
}

/** What starts a mention: @ for people and events, # for email. */
export type MentionSigil = "@" | "#";

export class DaemonError extends Error {
  readonly kind: TransportError["kind"];
  readonly code?: string;

  constructor(err: TransportError) {
    super(
      err.kind === "not_running"
        ? "The assistant isn't running."
        : err.kind === "unauthorized"
          ? "This browser isn't signed in to the assistant."
          : isOutdated(err)
            ? "Mimi was updated, but the old version is still running in the background. Quit Mimi and open it again."
            : err.message,
    );
    this.kind = err.kind;
    this.code = isOutdated(err) ? "outdated" : err.kind === "api" ? err.code : undefined;
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
  /** What the assistant may do without asking, kind by kind, with the user's choices. */
  permissions: () => call<PermissionKind[]>("GET", "/permissions"),
  setPermission: (kind: string, choice: KindPermission) =>
    call<PermissionKind[]>("PUT", `/permissions/${encodeURIComponent(kind)}`, choice),

  providers: () => call<Provider[]>("GET", "/providers"),
  presets: () => call<ProviderPreset[]>("GET", "/providers/presets"),
  probe: (req: ProbeRequest) => call<ProbeResult>("POST", "/providers/probe", req),
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
  /** Whether "Sign in with Google" works in this build. */
  googleSignInInfo: () => call<GoogleSignInInfo>("GET", "/google/sign-in"),
  /** Starts signing in; open `url` in the browser, then poll the status. */
  startGoogleSignIn: (reconnect?: string) =>
    call<GoogleSignIn>("POST", "/google/sign-in", { reconnect: reconnect ?? null }),
  googleSignInStatus: (id: string) => call<GoogleSignInStatus>("GET", `/google/sign-in/${id}`),
  cancelGoogleSignIn: (id: string) => call<null>("DELETE", `/google/sign-in/${id}`),

  people: (q = "") => call<PersonSummary[]>("GET", `/people?q=${encodeURIComponent(q)}`),
  person: (id: string) => call<Person>("GET", `/people/${id}`),
  addPerson: (p: NewPerson) => call<Person>("POST", "/people", p),
  updatePerson: (id: string, u: PersonUpdate) => call<Person>("PATCH", `/people/${id}`, u),
  /** Deletes someone from Mimi only; says how to bring them back, or null if they were added by hand. */
  removePerson: (id: string) => call<RemovedPerson | null>("DELETE", `/people/${id}`),
  removedPeople: () => call<RemovedPerson[]>("GET", "/people/removed"),
  restorePerson: (id: string) => call<Person>("POST", `/people/removed/${id}/restore`),
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
  personEvents: (id: string, from: number, to: number) =>
    call<CalendarEvent[]>("GET", `/people/${id}/events?from=${from}&to=${to}`),
  personConversations: (id: string) =>
    call<PersonConversation[]>("GET", `/people/${id}/conversations`),
  /** What memory holds about someone. Answered by newer daemons only: callers handle 404. */
  personMemory: (id: string) => call<MemoryNote[]>("GET", `/people/${id}/memory`),
  /** Recent email conversations with someone (none without a connected mailbox). */
  personMail: (id: string) =>
    call<MailThread[]>("GET", `/mail/threads?person=${encodeURIComponent(id)}&limit=5`),

  mailDiscover: (email: string) => call<MailDiscovery>("POST", "/mail/discover", { email }),
  mailPresets: () => call<MailPreset[]>("GET", "/mail/presets"),
  /** Counts are for `scope` (all mail when empty); accounts are always all of them. */
  mailOverview: (scope: MailScope = {}) => call<MailOverview>("GET", `/mail?${scopeParams(scope)}`),
  mailThreads: (view: MailBox | null, q: string, scope: MailScope = {}, folder: number | null = null) => {
    const params = scopeParams(scope);
    params.set("limit", "80");
    if (folder !== null) params.set("folder", String(folder));
    else if (view) params.set("view", view);
    if (q.trim()) params.set("q", q.trim());
    return call<MailThread[]>("GET", `/mail/threads?${params}`);
  },
  mailThread: (id: number) => call<MailThreadDetail>("GET", `/mail/threads/${id}`),
  /** One email as it was sent (made safe) and as formatted text, pictures from other servers left out. */
  mailContent: (id: number) => call<MailContent>("GET", `/mail/messages/${id}/content`),
  /** The same with its pictures from other servers, fetched by the daemon now (the user asked). */
  mailContentWithImages: (id: number) => call<MailContent>("POST", `/mail/messages/${id}/images`),
  markMailRead: (id: number, read: boolean) =>
    call<null>("POST", `/mail/threads/${id}/read`, { read }),
  archiveMail: (id: number) => call<null>("POST", `/mail/threads/${id}/archive`),
  createMailFolder: (f: MailFolderInput) => call<null>("POST", "/mail/folders", f),
  updateMailFolder: (id: number, f: MailFolderInput) => call<null>("PATCH", `/mail/folders/${id}`, f),
  deleteMailFolder: (id: number) => call<null>("DELETE", `/mail/folders/${id}`),
  /** The user puts a conversation in a smart folder or takes it out. */
  setMailThreadFolder: (thread: number, folder: number, member: boolean) =>
    call<null>("POST", `/mail/threads/${thread}/folders`, { folder, member }),
  /** Moves a conversation to the Trash of its mail account. */
  deleteMail: (id: number) => call<null>("DELETE", `/mail/threads/${id}`),
  summarizeMail: (id: number) => call<MailSummary>("POST", `/mail/threads/${id}/summarize`),
  draftMailReply: (id: number, instructions: string | null) =>
    call<MailDraft>("POST", `/mail/threads/${id}/draft`, { instructions }),
  /** Sends a message the user wrote or checked: their click is the approval. */
  sendMail: (draft: MailDraft) => call<null>("POST", "/mail/send", draft),
  refreshMail: () => call<null>("POST", "/mail/refresh"),
  /** Checks the TypeSafe key with TypeSafe, then saves it (Jev as the mail sorter). */
  jevConnect: (api_key: string) => call<null>("PUT", "/mail/jev", { api_key }),
  jevDisconnect: () => call<null>("DELETE", "/mail/jev"),

  calendars: () => call<CalendarInfo[]>("GET", "/calendars"),
  events: (from: number, to: number) =>
    call<CalendarEvents>("GET", `/calendar/events?from=${from}&to=${to}`),
  addEvent: (e: NewCalendarEvent) => call<CreatedEvent>("POST", "/calendar/events", e),
  /** @ suggestions (people and events), or # suggestions (conversations and emails). */
  mentions: (q: string, kind: MentionSigil = "@") =>
    call<MentionCandidate[]>(
      "GET",
      `/mentions?q=${encodeURIComponent(q)}${kind === "#" ? "&kind=mail" : ""}`,
    ),

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
    call<null>("POST", `/actions/${id}/approve`, { arguments: args ?? null, always: false }),
  /** Approves, and stops asking for the person or calendar the card offered. */
  approveAlways: (id: string) =>
    call<null>("POST", `/actions/${id}/approve`, { arguments: null, always: true }),
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
  /** Finding memories by meaning; turning it on downloads what it needs. */
  setMemorySemantic: (enabled: boolean) =>
    call<MemorySemantic>("PUT", "/memory/semantic", { enabled }),
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
  /** Every time reminders and routines go off (or went off) in a range, for the calendar. */
  scheduleOccurrences: (from: number, to: number) =>
    call<ScheduleOccurrence[]>("GET", `/schedule/occurrences?from=${from}&to=${to}`),
  reminderDone: (deliveryId: string) =>
    call<null>("POST", `/schedule/deliveries/${deliveryId}/done`),
  snoozeReminder: (deliveryId: string, minutes: number) =>
    call<null>("POST", `/schedule/deliveries/${deliveryId}/snooze`, { minutes }),
  undoSchedule: (revision: number) => call<null>("POST", `/schedule/undo/${revision}`),
};

/** Query keys, shared by queries and the event sync so they stay in step. */
export const keys = {
  settings: ["settings"] as const,
  permissions: ["permissions"] as const,
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
  /** Events, conversations, notes and mail about someone. */
  personExtra: (id: string, what: "events" | "conversations" | "memory" | "mail") =>
    ["people", "person", id, what] as const,
  duplicates: ["people", "duplicates"] as const,
  removedPeople: ["people", "removed"] as const,
  // Under "mail" for #, so new mail refreshes the suggestions.
  mentions: (q: string, kind: MentionSigil = "@") =>
    kind === "#" ? (["mail", "mentions", q] as const) : (["people", "mentions", q] as const),
  conversations: ["conversations"] as const,
  conversation: (id: string) => ["conversation", id] as const,
  memory: ["memory"] as const,
  memoryNote: (path: string) => ["memory", "note", path] as const,
  /** Reminders and routines, their recent deliveries and their times in the calendar. */
  schedule: ["schedule"] as const,
  deliveries: ["schedule", "deliveries"] as const,
  occurrences: (from: number, to: number) => ["schedule", "occurrences", from, to] as const,
  /** Everything read from mail. */
  mail: ["mail"] as const,
  mailPresets: ["mail", "presets"] as const,
  mailOverview: (scope: MailScope = {}) =>
    ["mail", "overview", scope.account ?? null, scope.address ?? null] as const,
  mailThreads: (view: MailBox | null, q: string, scope: MailScope = {}, folder: number | null = null) =>
    ["mail", "threads", view, q, scope.account ?? null, scope.address ?? null, folder] as const,
  mailThread: (id: number) => ["mail", "thread", id] as const,
  // Not under "mail": an email's content never changes, and loading its pictures again at
  // every mail change would tell the sender each time.
  mailContent: (id: number, images: boolean) => ["mailContent", id, images] as const,
  /** Every calendar and event read. */
  calendar: ["calendar"] as const,
  calendars: ["calendar", "list"] as const,
  events: (from: number, to: number) => ["calendar", "events", from, to] as const,
};
