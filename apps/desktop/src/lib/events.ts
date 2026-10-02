import { useEffect, useSyncExternalStore } from "react";
import type { QueryClient } from "@tanstack/react-query";

import type { Conversation } from "@/bindings/Conversation";
import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { Event } from "@/bindings/Event";
import type { GuestApproval } from "@/bindings/GuestApproval";
import type { Message } from "@/bindings/Message";
import type { ModelPull } from "@/bindings/ModelPull";
import { keys } from "@/lib/api";
import { showMailThread } from "@/features/mail/mail-view";
import { announceDelivery } from "@/features/reminders/announce";
import { announceOutbox } from "@/features/mail/send-later";
import { subscribe } from "@/lib/transport";

// Connection state, fed by the transport (Tauri relay or browser WebSocket).
let connected: boolean | null = null;
const listeners = new Set<() => void>();
function setConnected(value: boolean) {
  connected = value;
  listeners.forEach((l) => l());
}

/** `null` until the first answer from the relay. */
export function useConnected(): boolean | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => connected,
  );
}

/** Keeps the query cache in sync with daemon events. Mount once. */
export function useDaemonSync(qc: QueryClient) {
  useEffect(
    () =>
      subscribe(
        (event) => apply(qc, event),
        (value) => {
          const reconnected = value && connected === false;
          setConnected(value);
          // Anything could have changed while we were away.
          if (reconnected) qc.invalidateQueries();
        },
      ),
    [qc],
  );
}

function apply(qc: QueryClient, event: Event) {
  switch (event.type) {
    case "settings_changed":
      qc.setQueryData(keys.settings, event.settings);
      qc.invalidateQueries({ queryKey: keys.permissions });
      break;
    case "providers_changed":
      qc.setQueryData(keys.providers, event.providers);
      qc.invalidateQueries({ queryKey: keys.allModels });
      qc.invalidateQueries({ queryKey: keys.recommendations });
      break;
    case "conversation_updated":
      qc.setQueryData<Conversation[]>(keys.conversations, (list) =>
        list ? sortByActivity(upsert(list, event.conversation)) : list,
      );
      qc.setQueryData<ConversationDetail>(keys.conversation(event.conversation.id), (d) =>
        d ? { ...d, conversation: event.conversation } : d,
      );
      break;
    case "conversation_deleted":
      qc.setQueryData<Conversation[]>(keys.conversations, (list) =>
        list?.filter((c) => c.id !== event.id),
      );
      qc.removeQueries({ queryKey: keys.conversation(event.id) });
      break;
    case "message_updated":
      qc.setQueryData<ConversationDetail>(keys.conversation(event.message.conversation_id), (d) =>
        d ? { ...d, messages: upsert(d.messages, event.message) } : d,
      );
      break;
    case "message_delta":
      qc.setQueryData<ConversationDetail>(keys.conversation(event.conversation_id), (d) =>
        d
          ? {
              ...d,
              messages: d.messages.map((m): Message =>
                m.id === event.message_id
                  ? {
                      ...m,
                      content: m.content + event.content,
                      reasoning: m.reasoning + event.reasoning,
                    }
                  : m,
              ),
            }
          : d,
      );
      break;
    case "connections_changed":
      qc.setQueryData(keys.connections, event.connections);
      qc.invalidateQueries({ queryKey: keys.integrations });
      qc.invalidateQueries({ queryKey: keys.calendar });
      // Exceptions name calendars and Matrix groups.
      qc.invalidateQueries({ queryKey: keys.permissions });
      qc.invalidateQueries({ queryKey: keys.matrixGroups });
      qc.invalidateQueries({ queryKey: keys.mail });
      break;
    case "mail_changed":
      qc.invalidateQueries({ queryKey: keys.mail });
      // "Recent emails" on people's pages.
      qc.invalidateQueries({ predicate: (q) => q.queryKey[0] === "people" && q.queryKey[3] === "mail" });
      break;
    case "people_changed":
      qc.invalidateQueries({ queryKey: keys.people });
      break;
    case "person_access_changed":
      qc.invalidateQueries({ queryKey: keys.personExtra(event.person_id, "access") });
      break;
    case "guest_approval": {
      // Only the card: waiting ones are shown, settled ones go.
      const { approval } = event;
      qc.setQueryData<GuestApproval[]>(keys.guestApprovals, (list = []) => {
        const others = list.filter((a) => a.action.id !== approval.action.id);
        return approval.action.status === "pending_approval" ? [...others, approval] : others;
      });
      break;
    }
    case "model_pull": {
      const { pull } = event;
      const same = (p: ModelPull) => p.provider_id === pull.provider_id && p.model === pull.model;
      qc.setQueryData<ModelPull[]>(keys.pulls, (list = []) =>
        pull.state === "running"
          ? [...list.filter((p) => !same(p)), pull]
          : list.map((p) => (same(p) ? pull : p)),
      );
      if (pull.state === "done") {
        qc.invalidateQueries({ queryKey: keys.allModels });
        qc.invalidateQueries({ queryKey: keys.recommendations });
        qc.invalidateQueries({ queryKey: keys.runtime });
      }
      break;
    }
    case "update_changed":
      qc.setQueryData(keys.updates, event.update);
      break;
    case "voice_changed":
      qc.setQueryData(keys.voice, event.voice);
      break;
    case "runtime_changed":
      qc.setQueryData(keys.runtime, event.runtime);
      break;
    case "memory_changed":
      qc.invalidateQueries({ queryKey: keys.memory });
      // What each person's page shows as remembered about them.
      qc.invalidateQueries({
        predicate: (q) => q.queryKey[0] === "people" && q.queryKey[3] === "memory",
      });
      break;
    case "schedule_changed":
      qc.invalidateQueries({ queryKey: keys.schedule });
      break;
    case "schedule_delivered":
      qc.invalidateQueries({ queryKey: keys.schedule });
      announceDelivery(event.delivery);
      break;
    case "mail_outbox":
      qc.invalidateQueries({ queryKey: keys.mailOutbox });
      announceOutbox(event.item);
      break;
    case "resync":
      qc.invalidateQueries();
      break;
    case "open_mail":
      // The user clicked a new-mail notification (the app's window is already coming
      // forward): show Mail, on that conversation when there's one.
      if (event.thread_id !== null) showMailThread(event.thread_id);
      location.hash = "#/mail";
      break;
  }
}

function upsert<T extends { id: string }>(list: T[], item: T): T[] {
  const i = list.findIndex((x) => x.id === item.id);
  if (i === -1) return [...list, item];
  const next = list.slice();
  next[i] = item;
  return next;
}

function sortByActivity(list: Conversation[]) {
  return list.slice().sort((a, b) => b.updated_at - a.updated_at);
}
