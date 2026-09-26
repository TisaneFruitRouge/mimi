import { Blocks, BookOpen, MessageSquarePlus, Sparkles } from "lucide-react";

import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import type { Section } from "@/features/shell/top-bar";
import { useConversations } from "@/lib/queries";

export function ConversationPalette({
  open,
  onOpenChange,
  onOpenConversation,
  onNewConversation,
  onSection,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenConversation: (id: string) => void;
  onNewConversation: () => void;
  onSection: (s: Section) => void;
}) {
  const conversations = useConversations().data ?? [];
  const run = (fn: () => void) => () => {
    onOpenChange(false);
    fn();
  };
  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Conversations"
      description="Search your conversations or jump somewhere"
      className="top-[20%] translate-y-0 sm:max-w-xl"
    >
      <CommandInput placeholder="Search conversations…" />
      <CommandList className="max-h-[420px]">
        <CommandEmpty>No conversations match.</CommandEmpty>
        <CommandGroup heading="Go">
          <CommandItem onSelect={run(onNewConversation)}>
            <MessageSquarePlus /> New conversation
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("connections"))}>
            <Blocks /> Connections
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("models"))}>
            <Sparkles /> Models
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("memory"))}>
            <BookOpen /> Memory
          </CommandItem>
        </CommandGroup>
        {conversations.length > 0 && (
          <CommandGroup heading="Conversations">
            {conversations.map((c) => (
              <CommandItem
                key={c.id}
                value={`${c.title} ${c.id}`}
                onSelect={run(() => onOpenConversation(c.id))}
              >
                <span className="truncate">{c.title}</span>
                <span className="ml-auto shrink-0 font-mono text-[11px] text-faint">
                  {relativeTime(c.updated_at)}
                </span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}

function relativeTime(ms: number) {
  const diff = Date.now() - ms;
  const min = Math.round(diff / 60_000);
  if (min < 1) return "now";
  if (min < 60) return `${min}m`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d`;
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
