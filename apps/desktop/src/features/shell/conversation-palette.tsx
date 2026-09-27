import { Bell, Blocks, BookOpen, MessageSquare, SquarePen, Sparkles, Users } from "lucide-react";

import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import type { Section } from "@/features/shell/top-bar";
import { mod } from "@/lib/platform";
import { useConversations } from "@/lib/queries";

/** Spotlight-like search over conversations, plus quick jumps. */
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
      title="Search"
      description="Find a conversation or jump somewhere"
      showCloseButton={false}
      className="top-[18%] translate-y-0 gap-0 rounded-[20px] sm:max-w-[600px]"
    >
      <CommandInput placeholder="Search conversations" />
      <CommandList className="max-h-[440px]">
        <CommandEmpty>No conversations found.</CommandEmpty>
        {conversations.length > 0 && (
          <CommandGroup heading="Recent">
            {conversations.map((c) => (
              <CommandItem
                key={c.id}
                value={`${c.title} ${c.id}`}
                onSelect={run(() => onOpenConversation(c.id))}
              >
                <MessageSquare />
                <span className="truncate">{c.title}</span>
                <CommandShortcut>{relativeTime(c.updated_at)}</CommandShortcut>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        <CommandGroup heading="Go to">
          <CommandItem onSelect={run(onNewConversation)}>
            <SquarePen /> New chat
            <CommandShortcut>{mod}N</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("connections"))}>
            <Blocks /> Connections
            <CommandShortcut>{mod}2</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("models"))}>
            <Sparkles /> Models
            <CommandShortcut>{mod}3</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("memory"))}>
            <BookOpen /> Memory
          </CommandItem>
          <CommandItem onSelect={run(() => onSection("people"))}>
            <Users /> People
          </CommandItem>
          <CommandItem keywords={["routines", "alarm", "schedule"]} onSelect={run(() => onSection("reminders"))}>
            <Bell /> Reminders and routines
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  );
}

function relativeTime(ms: number) {
  const diff = Date.now() - ms;
  const min = Math.round(diff / 60_000);
  if (min < 1) return "Just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 24) return h === 1 ? "An hour ago" : `${h} hours ago`;
  const d = Math.round(h / 24);
  if (d === 1) return "Yesterday";
  if (d < 7) return new Date(ms).toLocaleDateString(undefined, { weekday: "long" });
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
