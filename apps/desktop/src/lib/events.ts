import { useEffect, useSyncExternalStore } from "react";
import type { QueryClient } from "@tanstack/react-query";

import type { Conversation } from "@/bindings/Conversation";
import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { Event } from "@/bindings/Event";
import type { Message } from "@/bindings/Message";
import type { ModelPull } from "@/bindings/ModelPull";
import { keys } from "@/lib/api";
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
      }
      break;
    }
    case "resync":
      qc.invalidateQueries();
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
