import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Flame } from "lucide-react";
import { toast } from "sonner";

import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { Message } from "@/bindings/Message";
import { ModelPicker } from "@/components/model-picker";
import { Skeleton } from "@/components/ui/skeleton";
import { MessageView } from "@/features/chat/message";
import { Composer } from "@/features/chat/composer";
import { DaemonError, api, keys } from "@/lib/api";
import { localityExplanation } from "@/lib/format";
import { useProviders, useSettings } from "@/lib/queries";

export function ChatView({
  conversationId,
  onCreated,
  onManageModels,
}: {
  conversationId: string | null;
  onCreated: (id: string) => void;
  onManageModels: () => void;
}) {
  const qc = useQueryClient();
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const detail = useQuery({
    queryKey: keys.conversation(conversationId ?? ""),
    queryFn: () => api.conversation(conversationId!),
    enabled: !!conversationId,
  });
  const messages = detail.data?.messages ?? [];
  const replying = messages.at(-1)?.status === "streaming";
  const provider = providers.find((p) => p.id === settings?.default_model?.provider_id);

  const send = async (content: string): Promise<boolean> => {
    try {
      let id = conversationId;
      if (!id) {
        const conversation = await api.createConversation();
        id = conversation.id;
        // Seed the cache so streaming events have somewhere to land.
        qc.setQueryData<ConversationDetail>(keys.conversation(id), {
          conversation,
          messages: [],
        });
        onCreated(id);
      }
      const sent = await api.send(id, { content, model: null });
      // Events usually get here first; only fill in what's missing.
      qc.setQueryData<ConversationDetail>(keys.conversation(id), (d) =>
        d ? { ...d, messages: insertMissing(d.messages, [sent.user_message, sent.assistant_message]) } : d,
      );
      return true;
    } catch (e) {
      const err = e as DaemonError;
      if (err.code === "no_model") {
        toast.error(err.message, { action: { label: "Choose", onClick: onManageModels } });
      } else {
        toast.error(err.message);
      }
      return false;
    }
  };

  const note = provider ? (
    provider.locality === "cloud" ? (
      <span className="text-amber-700 dark:text-amber-400">
        Messages are sent to {provider.name}, a cloud service.
      </span>
    ) : (
      localityExplanation[provider.locality]
    )
  ) : null;

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center justify-between gap-4 border-b px-4">
        <h1 className="truncate text-sm font-medium">
          {detail.data?.conversation.title ?? "New chat"}
        </h1>
        <ModelPicker onManage={onManageModels} />
      </header>
      <MessageList
        messages={messages}
        loading={detail.isLoading && !!conversationId}
        assistantName={settings?.assistant_name ?? "Hearth"}
      />
      <Composer
        replying={replying}
        disabled={!settings?.default_model}
        note={note}
        onSend={send}
        onStop={() => conversationId && api.cancel(conversationId)}
      />
    </div>
  );
}

function MessageList({
  messages,
  loading,
  assistantName,
}: {
  messages: Message[];
  loading: boolean;
  assistantName: string;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const last = messages.at(-1);

  // Follow new text unless the user has scrolled up to read.
  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [messages.length, last?.content, last?.reasoning]);

  return (
    <div
      ref={scroller}
      className="min-h-0 flex-1 overflow-y-auto"
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
    >
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 px-4 py-6">
        {loading &&
          [0, 1, 2].map((i) => <Skeleton key={i} className="h-16 w-full rounded-xl" />)}
        {!loading && messages.length === 0 && (
          <div className="flex flex-col items-center gap-3 pt-[18vh] text-center">
            <div className="rounded-2xl bg-orange-500/10 p-3">
              <Flame className="size-7 text-orange-500" />
            </div>
            <h2 className="text-xl font-semibold tracking-tight">
              Hi, I'm {assistantName}. What can I help with?
            </h2>
          </div>
        )}
        {messages.map((m) => (
          <MessageView key={m.id} message={m} />
        ))}
      </div>
    </div>
  );
}

function insertMissing(list: Message[], items: Message[]) {
  const missing = items.filter((i) => !list.some((m) => m.id === i.id));
  return missing.length ? [...list, ...missing] : list;
}
