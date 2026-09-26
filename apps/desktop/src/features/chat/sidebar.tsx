import { useState } from "react";
import { Flame, MoreHorizontal, Pencil, Plus, Settings, Trash2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Conversation } from "@/bindings/Conversation";
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
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api } from "@/lib/api";
import { useConversations, useSettings } from "@/lib/queries";

export function Sidebar({
  activeId,
  settingsOpen,
  onSelect,
  onNew,
  onSettings,
}: {
  activeId: string | null;
  settingsOpen: boolean;
  onSelect: (id: string) => void;
  onNew: () => void;
  onSettings: () => void;
}) {
  const conversations = useConversations().data ?? [];
  const name = useSettings().data?.assistant_name ?? "Hearth";
  const [renaming, setRenaming] = useState<Conversation | null>(null);
  const [deleting, setDeleting] = useState<Conversation | null>(null);

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r bg-muted/40">
      <div className="flex items-center gap-2 px-4 pt-4 pb-3">
        <Flame className="size-5 text-orange-500" />
        <span className="font-semibold tracking-tight">{name}</span>
      </div>
      <div className="px-3 pb-2">
        <Button variant="outline" className="w-full justify-start" onClick={onNew}>
          <Plus /> New chat
        </Button>
      </div>
      <ScrollArea className="min-h-0 flex-1 px-3">
        <nav className="flex flex-col gap-0.5 pb-3">
          {conversations.map((c) => (
            <div
              key={c.id}
              className={cn(
                "group flex items-center rounded-md text-sm transition-colors hover:bg-accent",
                c.id === activeId && !settingsOpen && "bg-accent font-medium",
              )}
            >
              <button
                className="min-w-0 flex-1 truncate px-3 py-2 text-left"
                onClick={() => onSelect(c.id)}
                title={c.title}
              >
                {c.title}
              </button>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="mr-1 opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100"
                    aria-label="Conversation actions"
                  >
                    <MoreHorizontal />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  <DropdownMenuItem onSelect={() => setRenaming(c)}>
                    <Pencil /> Rename
                  </DropdownMenuItem>
                  <DropdownMenuItem variant="destructive" onSelect={() => setDeleting(c)}>
                    <Trash2 /> Delete
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          ))}
          {conversations.length === 0 && (
            <p className="px-3 py-2 text-sm text-muted-foreground">No conversations yet.</p>
          )}
        </nav>
      </ScrollArea>
      <div className="border-t p-3">
        <Button
          variant={settingsOpen ? "secondary" : "ghost"}
          className="w-full justify-start"
          onClick={onSettings}
        >
          <Settings /> Settings
        </Button>
      </div>

      <RenameDialog conversation={renaming} onClose={() => setRenaming(null)} />
      <AlertDialog open={!!deleting} onOpenChange={(open) => !open && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this conversation?</AlertDialogTitle>
            <AlertDialogDescription>
              “{deleting?.title}” and all its messages will be permanently deleted.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() =>
                deleting &&
                api
                  .deleteConversation(deleting.id)
                  .then(() => deleting.id === activeId && onNew())
                  .catch((e) => toast.error(e.message))
              }
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </aside>
  );
}

function RenameDialog({
  conversation,
  onClose,
}: {
  conversation: Conversation | null;
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [lastId, setLastId] = useState<string | null>(null);
  if (conversation && conversation.id !== lastId) {
    setLastId(conversation.id);
    setTitle(conversation.title);
  }
  const save = async () => {
    if (!conversation) return;
    try {
      await api.renameConversation(conversation.id, title);
      onClose();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  return (
    <Dialog open={!!conversation} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rename conversation</DialogTitle>
        </DialogHeader>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <Input value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
          <DialogFooter className="mt-4">
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!title.trim()}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
