import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarDays, Lightbulb, MoreHorizontal, PenLine, Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";

import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { Message } from "@/bindings/Message";
import { LocalityBadge } from "@/components/locality-badge";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Composer, type ComposerHandle } from "@/features/chat/composer";
import { MessageView } from "@/features/chat/message";
import type { Section } from "@/features/shell/top-bar";
import { DaemonError, api, keys } from "@/lib/api";
import { greeting } from "@/lib/format";
import { useActiveModel } from "@/lib/queries";

export function ChatView({
  conversationId,
  onCreated,
  onNewConversation,
  onSection,
}: {
  conversationId: string | null;
  onCreated: (id: string) => void;
  onNewConversation: () => void;
  onSection: (s: Section) => void;
}) {
  const qc = useQueryClient();
  const composer = useRef<ComposerHandle>(null);
  const detail = useQuery({
    queryKey: keys.conversation(conversationId ?? ""),
    queryFn: () => api.conversation(conversationId!),
    enabled: !!conversationId,
  });
  const messages = detail.data?.messages ?? [];
  const replying = messages.at(-1)?.status === "streaming";

  const send = async (content: string): Promise<boolean> => {
    try {
      let id = conversationId;
      if (!id) {
        const conversation = await api.createConversation();
        id = conversation.id;
        // Seed the cache so streaming events have somewhere to land.
        qc.setQueryData<ConversationDetail>(keys.conversation(id), { conversation, messages: [] });
        onCreated(id);
      }
      const sent = await api.send(id, { content, model: null });
      // Events usually get here first; only fill in what's missing.
      qc.setQueryData<ConversationDetail>(keys.conversation(id), (d) =>
        d
          ? { ...d, messages: insertMissing(d.messages, [sent.user_message, sent.assistant_message]) }
          : d,
      );
      return true;
    } catch (e) {
      const err = e as DaemonError;
      if (err.code === "no_model") {
        toast.error(err.message, { action: { label: "Models", onClick: () => onSection("models") } });
      } else {
        toast.error(err.message);
      }
      return false;
    }
  };

  const empty = !conversationId || (!detail.isLoading && messages.length === 0);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {detail.data && (
        <ConversationHeader
          id={detail.data.conversation.id}
          title={detail.data.conversation.title}
          onDeleted={onNewConversation}
        />
      )}
      {empty ? (
        <EmptyState onStarter={(t) => composer.current?.setText(t)} />
      ) : (
        <MessageList messages={messages} loading={detail.isLoading} />
      )}
      <Composer
        ref={composer}
        replying={replying}
        onSend={send}
        onStop={() => conversationId && api.cancel(conversationId)}
        onNewConversation={onNewConversation}
        onSection={onSection}
      />
    </div>
  );
}

function ConversationHeader({
  id,
  title,
  onDeleted,
}: {
  id: string;
  title: string;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const rename = async () => {
    setEditing(false);
    if (draft.trim() && draft.trim() !== title) {
      await api.renameConversation(id, draft.trim()).catch((e) => toast.error(e.message));
    }
  };

  return (
    <div className="mx-auto flex h-11 w-full max-w-[760px] shrink-0 items-center gap-2 px-5">
      {editing ? (
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={rename}
          onKeyDown={(e) => {
            if (e.key === "Enter") rename();
            if (e.key === "Escape") setEditing(false);
          }}
          aria-label="Conversation title"
          className="h-8 flex-1 rounded-md bg-background px-2 text-sm font-medium outline-none ring-1 ring-ring"
        />
      ) : (
        <button
          onDoubleClick={() => {
            setDraft(title);
            setEditing(true);
          }}
          className="min-w-0 truncate text-sm font-medium text-muted-foreground"
          title={title}
        >
          {title}
        </button>
      )}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="Conversation actions" className="text-faint">
            <MoreHorizontal />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem
            onSelect={() => {
              setDraft(title);
              setEditing(true);
            }}
          >
            <Pencil /> Rename
          </DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirmDelete(true)}>
            <Trash2 /> Delete
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this conversation?</AlertDialogTitle>
            <AlertDialogDescription>
              “{title}” and all its messages will be permanently deleted.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() =>
                api
                  .deleteConversation(id)
                  .then(onDeleted)
                  .catch((e) => toast.error(e.message))
              }
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

const starters = [
  {
    icon: CalendarDays,
    title: "Plan my week",
    prompt: "Help me plan my week. Ask me what's coming up and what matters most.",
  },
  {
    icon: PenLine,
    title: "Write a message",
    prompt: "Help me write a message to ",
  },
  {
    icon: Lightbulb,
    title: "Think something through",
    prompt: "I'm trying to decide ",
  },
];

function EmptyState({ onStarter }: { onStarter: (text: string) => void }) {
  const active = useActiveModel();
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-8 overflow-y-auto px-5 pb-8">
      <div className="flex flex-col items-center gap-3 text-center">
        <h1 className="text-[34px] font-semibold tracking-[-0.03em]">{greeting()}.</h1>
        <p className="text-[15px] text-muted-foreground">What can I do for you?</p>
        {active && (
          <LocalityBadge
            locality={active.provider.locality}
            label={
              active.provider.locality === "cloud"
                ? `Uses ${active.provider.name}, a cloud service`
                : active.provider.locality === "device"
                  ? "Private · runs on this computer"
                  : "Private · runs on your own network"
            }
            className="mt-1"
          />
        )}
      </div>
      <div className="grid w-full max-w-[720px] grid-cols-3 gap-3">
        {starters.map((s) => (
          <button
            key={s.title}
            onClick={() => onStarter(s.prompt)}
            className="flex flex-col gap-3 rounded-2xl border bg-background p-4 text-left shadow-xs transition hover:-translate-y-px hover:shadow-md"
          >
            <s.icon className="size-[18px] text-muted-foreground" />
            <span className="text-[14px] font-medium">{s.title}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function MessageList({ messages, loading }: { messages: Message[]; loading: boolean }) {
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
      <div className="mx-auto flex w-full max-w-[760px] flex-col gap-7 px-5 pt-4 pb-8">
        {loading && [0, 1, 2].map((i) => <Skeleton key={i} className="h-16 w-full rounded-2xl" />)}
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
