import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDown, CalendarDays, ChevronDown, Lightbulb, PenLine, Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";

import type { ConversationDetail } from "@/bindings/ConversationDetail";
import type { Message } from "@/bindings/Message";
import { LogoMark } from "@/components/brand";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Composer, type ComposerHandle } from "@/features/chat/composer";
import { MessageView } from "@/features/chat/message";
import type { Section } from "@/features/shell/top-bar";
import { DaemonError, api, keys } from "@/lib/api";
import { greeting } from "@/lib/format";
import { useActiveModel } from "@/lib/queries";
import { useScrollEdge } from "@/lib/scroll-edge";

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
  const dock = useRef<HTMLDivElement>(null);
  const [dockHeight, setDockHeight] = useState(120);
  const detail = useQuery({
    queryKey: keys.conversation(conversationId ?? ""),
    queryFn: () => api.conversation(conversationId!),
    enabled: !!conversationId,
  });
  const messages = detail.data?.messages ?? [];
  const replying = messages.at(-1)?.status === "streaming";

  // The composer floats over the thread; keep the thread's bottom padding in step
  // with its height as it grows.
  useLayoutEffect(() => {
    const el = dock.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setDockHeight(el.offsetHeight));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

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
    <div className="relative h-full">
      {empty ? (
        <EmptyState bottom={dockHeight} onStarter={(t) => composer.current?.setText(t)} />
      ) : (
        <MessageList
          messages={messages}
          loading={detail.isLoading}
          bottom={dockHeight}
          header={
            detail.data && (
              <ConversationTitle
                id={detail.data.conversation.id}
                title={detail.data.conversation.title}
                onNew={onNewConversation}
                onDeleted={onNewConversation}
              />
            )
          }
        />
      )}
      <div ref={dock} className="pointer-events-none absolute inset-x-0 bottom-0 z-10">
        {/* A soft fade so the thread dissolves under the composer. */}
        <div className="h-8 bg-gradient-to-b from-transparent to-canvas" />
        <div className="pointer-events-auto bg-canvas">
          <Composer
            ref={composer}
            replying={replying}
            onSend={send}
            onStop={() => conversationId && api.cancel(conversationId)}
            onNewConversation={onNewConversation}
            onSection={onSection}
          />
        </div>
      </div>
    </div>
  );
}

/** The conversation's name at the top of the thread, with its few actions. */
function ConversationTitle({
  id,
  title,
  onNew,
  onDeleted,
}: {
  id: string;
  title: string;
  onNew: () => void;
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
    <div className="flex justify-center pb-2">
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
          aria-label="Conversation name"
          className="h-8 w-full max-w-[420px] rounded-[9px] bg-background px-3 text-center type-subhead font-medium shadow-[0_0_0_1px_rgb(143_186_42/0.9)] ring-[4px] ring-ring/25 outline-none"
        />
      ) : (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              className="pressable flex h-8 max-w-[480px] items-center gap-1 rounded-full px-3 type-subhead font-medium text-muted-foreground hover:bg-fill hover:text-foreground data-[state=open]:bg-fill data-[state=open]:text-foreground"
              onDoubleClick={() => {
                setDraft(title);
                setEditing(true);
              }}
            >
              <span className="truncate">{title}</span>
              <ChevronDown className="size-3.5 shrink-0 opacity-70" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" className="min-w-[200px]">
            <DropdownMenuItem
              onSelect={() => {
                setDraft(title);
                setEditing(true);
              }}
            >
              <Pencil /> Rename
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onNew}>
              <PenLine /> New chat
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onSelect={() => setConfirmDelete(true)}>
              <Trash2 /> Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this conversation?</AlertDialogTitle>
            <AlertDialogDescription>
              “{title}” and everything in it will be deleted. This can't be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
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
    tint: "bg-event-soft text-event",
    prompt: "Help me plan my week. Ask me what's coming up and what matters most.",
  },
  {
    icon: PenLine,
    title: "Write a message",
    tint: "bg-lime-soft text-lime-deep",
    prompt: "Help me write a message to ",
  },
  {
    icon: Lightbulb,
    title: "Think something through",
    tint: "bg-cloud-soft text-cloud",
    prompt: "I'm trying to decide ",
  },
];

function EmptyState({ bottom, onStarter }: { bottom: number; onStarter: (text: string) => void }) {
  const active = useActiveModel();
  const locality = active?.provider.locality;
  return (
    <div
      className="flex h-full flex-col items-center justify-center overflow-y-auto px-6 pt-[60px]"
      style={{ paddingBottom: bottom }}
    >
      <motion.div
        className="flex w-full max-w-[640px] flex-col items-center gap-9"
        initial="hidden"
        animate="shown"
        variants={{ shown: { transition: { staggerChildren: 0.06 } } }}
      >
        <motion.div variants={rise} className="flex flex-col items-center gap-3 text-center">
          <LogoMark className="mb-2 size-12 rounded-[14px] [&_svg]:size-6" />
          <h1 className="text-[32px] leading-[38px] font-semibold tracking-[-0.03em]">{greeting()}.</h1>
          <p className="type-body text-muted-foreground">What can I do for you?</p>
          {active && locality && (
            <LocalityBadge
              locality={locality}
              label={
                locality === "cloud"
                  ? `Uses ${active.provider.name}, a cloud service`
                  : locality === "device"
                    ? "Private, runs on this computer"
                    : "Private, runs on your own network"
              }
              className="mt-1"
            />
          )}
        </motion.div>
        <div className="grid w-full grid-cols-3 gap-3">
          {starters.map((s) => (
            <motion.button
              key={s.title}
              variants={rise}
              onClick={() => onStarter(s.prompt)}
              className="pressable surface flex flex-col items-start gap-3.5 p-4 text-left transition-shadow hover:shadow-[var(--shadow-raised)]"
            >
              <span className={`flex size-8 items-center justify-center rounded-[9px] ${s.tint}`}>
                <s.icon className="size-[17px]" />
              </span>
              <span className="type-callout font-medium">{s.title}</span>
            </motion.button>
          ))}
        </div>
      </motion.div>
    </div>
  );
}

const rise = {
  hidden: { opacity: 0, y: 8 },
  shown: { opacity: 1, y: 0, transition: { type: "spring" as const, stiffness: 380, damping: 32 } },
};

function MessageList({
  messages,
  loading,
  bottom,
  header,
}: {
  messages: Message[];
  loading: boolean;
  bottom: number;
  header: React.ReactNode;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [away, setAway] = useState(false);
  const reportEdge = useScrollEdge();
  const last = messages.at(-1);

  const toBottom = (smooth: boolean) => {
    const el = scroller.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  };

  // Follow new text unless the user has scrolled up to read.
  useEffect(() => {
    if (pinned.current) toBottom(false);
  }, [messages.length, last?.content, last?.actions.length, bottom]);

  return (
    <>
      <div
        ref={scroller}
        className="h-full overflow-y-auto"
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
          setAway(!pinned.current);
          reportEdge(e);
        }}
      >
        <div
          className="mx-auto flex w-full max-w-[720px] flex-col gap-8 px-6 pt-[72px]"
          style={{ paddingBottom: bottom + 8 }}
        >
          {header}
          {loading &&
            [0, 1, 2].map((i) => <Skeleton key={i} className={i % 2 ? "ml-auto h-11 w-2/5" : "h-20 w-4/5"} />)}
          <AnimatePresence initial={false}>
            {messages.map((m) => (
              <motion.div
                key={m.id}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ type: "spring", stiffness: 420, damping: 34 }}
              >
                <MessageView message={m} />
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      </div>
      <div
        className="pointer-events-none absolute inset-x-0 z-20 flex justify-center"
        style={{ bottom: bottom + 12 }}
      >
        <AnimatePresence>
          {away && (
            <motion.button
              initial={{ opacity: 0, scale: 0.85 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.85 }}
              transition={{ type: "spring", stiffness: 500, damping: 30 }}
              onClick={() => toBottom(true)}
              aria-label="Jump to the latest message"
              className="material-thick pointer-events-auto flex size-9 items-center justify-center rounded-full text-muted-foreground shadow-[var(--shadow-raised)] hover:text-foreground"
            >
              <ArrowDown className="size-4" strokeWidth={2.2} />
            </motion.button>
          )}
        </AnimatePresence>
      </div>
    </>
  );
}

function insertMissing(list: Message[], items: Message[]) {
  const missing = items.filter((i) => !list.some((m) => m.id === i.id));
  return missing.length ? [...list, ...missing] : list;
}
