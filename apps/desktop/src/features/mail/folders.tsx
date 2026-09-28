import { useState } from "react";
import { FolderOpen, FolderPlus, Loader2, MoreHorizontal, Pencil, Trash2, X } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailFolder } from "@/bindings/MailFolder";
import type { MailOverview } from "@/bindings/MailOverview";
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import { colorOf, FolderGlyph, FolderLooksPicker } from "@/features/mail/folder-looks";

/**
 * Smart folders: the user names a folder and says what goes in it; their sorter (their
 * model or Jev) files conversations into it. Labels in Mimi only, nothing moves on the
 * mail server.
 */

/** The sidebar section: each folder, and a button to make one. */
export function FolderList({
  overview: o,
  current,
  onOpen,
}: {
  overview: MailOverview;
  current: number | null;
  onOpen: (id: number) => void;
}) {
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<MailFolder | null>(null);
  const [deleting, setDeleting] = useState<MailFolder | null>(null);
  return (
    <nav aria-label="Smart folders" className="flex flex-col gap-0.5">
      <div className="flex items-center justify-between pr-1 pb-1 pl-2.5">
        <span className="section-label !px-0">Smart folders</span>
        <button
          onClick={() => setCreating(true)}
          aria-label="New smart folder"
          title="New smart folder"
          className="rounded-full p-1 text-faint hover:bg-fill hover:text-foreground"
        >
          <FolderPlus className="size-3.5" />
        </button>
      </div>
      {o.folders.length === 0 && (
        <button
          onClick={() => setCreating(true)}
          className="rounded-[8px] px-2.5 py-1.5 text-left type-footnote text-faint hover:bg-[rgb(118_118_128/0.07)] hover:text-foreground"
        >
          Make a folder, say what goes in it, and your mail is filed for you.
        </button>
      )}
      {o.folders.map((f) => {
        const active = f.id === current;
        return (
          <ContextMenu key={f.id}>
            <ContextMenuTrigger asChild>
              <button
                onClick={() => onOpen(f.id)}
                aria-current={active ? "page" : undefined}
                title={f.description}
                className={cn(
                  "flex h-9 items-center gap-2.5 rounded-[8px] px-2.5 text-left type-callout transition-colors",
                  active ? "bg-fill font-medium" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
                )}
              >
                <FolderGlyph icon={f.icon} color={f.color} />
                <span className="min-w-0 flex-1 truncate">{f.name}</span>
                {f.to_check > 0 ? (
                  <Loader2 className="size-3 shrink-0 animate-spin text-faint" aria-label="Filing" />
                ) : (
                  f.unread > 0 && (
                    <span className="type-footnote font-medium text-muted-foreground tabular-nums">{f.unread}</span>
                  )
                )}
              </button>
            </ContextMenuTrigger>
            <ContextMenuContent className="w-[190px]">
              <ContextMenuItem onSelect={() => onOpen(f.id)}>
                <FolderOpen /> Open
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => setEditing(f)}>
                <Pencil /> Edit…
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem variant="destructive" onSelect={() => setDeleting(f)}>
                <Trash2 /> Delete folder…
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        );
      })}
      <FolderDialog open={creating} overview={o} onClose={() => setCreating(false)} onSaved={() => setCreating(false)} />
      {editing && (
        <FolderDialog
          key={editing.id}
          open
          folder={editing}
          overview={o}
          onClose={() => setEditing(null)}
          onSaved={() => setEditing(null)}
        />
      )}
      <DeleteFolder folder={deleting} onClose={() => setDeleting(null)} />
    </nav>
  );
}

/** Makes or edits a folder: a name and, in the user's words, what goes in it. */
export function FolderDialog({
  open,
  folder,
  overview: o,
  onClose,
  onSaved,
}: {
  open: boolean;
  folder?: MailFolder;
  overview: MailOverview;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(folder?.name ?? "");
  const [description, setDescription] = useState(folder?.description ?? "");
  const [icon, setIcon] = useState(folder?.icon ?? "sparkles");
  const [color, setColor] = useState(folder?.color ?? "violet");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const jev = o.sorter === "jev";
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      if (folder) await api.updateMailFolder(folder.id, { name, description, icon, color });
      else await api.createMailFolder({ name, description, icon, color });
      if (!folder) {
        setName("");
        setDescription("");
        setIcon("sparkles");
        setColor("violet");
      }
      toast.success(folder ? "Folder saved" : `“${name.trim()}” is being filled`);
      onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>{folder ? "Edit smart folder" : "New smart folder"}</DialogTitle>
          <DialogDescription>
            Say what belongs in it, the way you'd tell a person. Your mail is filed in the background,
            newest first. Folders only exist in Mimi: nothing changes in your mail account.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim() && description.trim()) void save();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <label htmlFor="folder-name" className="type-subhead font-medium">
              Name
            </label>
            <Input
              id="folder-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Receipts"
              maxLength={60}
              autoFocus
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <span className="type-subhead font-medium">Look</span>
            <FolderLooksPicker icon={icon} color={color} onIcon={setIcon} onColor={setColor} />
          </div>
          <div className="flex flex-col gap-1.5">
            <label htmlFor="folder-description" className="type-subhead font-medium">
              What goes in it?
            </label>
            <Textarea
              id="folder-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Receipts and invoices from shops and online services"
              maxLength={400}
              rows={3}
            />
            {folder && description.trim() !== folder.description && (
              <p className="type-footnote text-faint">
                A new description files your mail again. Conversations you moved yourself stay put.
              </p>
            )}
          </div>
          <p className="flex flex-wrap items-center gap-1.5 type-footnote text-muted-foreground">
            {jev ? "Filed by Jev" : "Filed by your model"}
            {o.sorter_locality && <LocalityBadge locality={o.sorter_locality} className="h-5" />}
            {jev && <span>· newsletters are sent to TypeSafe too, so they can be filed. Suspicious mail never is.</span>}
          </p>
          {error && <p className="type-subhead text-destructive">{error}</p>}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !name.trim() || !description.trim()}>
              {busy && <Loader2 className="animate-spin" />} {folder ? "Save" : "Make folder"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Asks before deleting a folder (the mail in it stays). */
function DeleteFolder({ folder, onClose, onDeleted }: { folder: MailFolder | null; onClose: () => void; onDeleted?: () => void }) {
  const remove = () =>
    folder &&
    api
      .deleteMailFolder(folder.id)
      .then(() => {
        toast.success(`“${folder.name}” deleted`);
        onDeleted?.();
      })
      .catch((e) => toast.error((e as Error).message));
  return (
    <AlertDialog open={folder !== null} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete “{folder?.name}”?</AlertDialogTitle>
          <AlertDialogDescription>The folder goes away; the mail in it stays where it is.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={remove}>
            Delete folder
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** A folder's title area in the list: its description, filing progress, edit/delete. */
export function FolderHeader({
  folder,
  overview,
  onDeleted,
}: {
  folder: MailFolder;
  overview: MailOverview;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="-mt-2 flex items-start gap-2 px-4 pb-3">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5 type-subhead text-muted-foreground">
        <span className="line-clamp-2">{folder.description}</span>
        {folder.to_check > 0 && (
          <span className="flex items-center gap-1.5 type-footnote text-faint">
            <Loader2 className="size-3 animate-spin" /> Filing your mail… {folder.to_check} to go
          </span>
        )}
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button aria-label="Folder options" className="rounded-full p-1 text-faint hover:bg-fill hover:text-foreground">
            <MoreHorizontal className="size-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => setEditing(true)}>
            <Pencil /> Edit
          </DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirming(true)}>
            <Trash2 /> Delete folder
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <FolderDialog
        key={`${folder.id}-${folder.name}-${folder.description}`}
        open={editing}
        folder={folder}
        overview={overview}
        onClose={() => setEditing(false)}
        onSaved={() => setEditing(false)}
      />
      <DeleteFolder folder={confirming ? folder : null} onClose={() => setConfirming(false)} onDeleted={onDeleted} />
    </div>
  );
}

const move = (thread: number, folder: MailFolder, member: boolean) =>
  api
    .setMailThreadFolder(thread, folder.id, member)
    .then(() => toast.success(member ? `Added to “${folder.name}”` : `Removed from “${folder.name}”`))
    .catch((e) => toast.error((e as Error).message));

/** The folders a conversation is in, each removable. */
export function FolderChips({ thread, ids, folders }: { thread: number; ids: number[]; folders: MailFolder[] }) {
  const shown = folders.filter((f) => ids.includes(f.id));
  if (shown.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      {shown.map((f) => (
        <span
          key={f.id}
          className="inline-flex h-[24px] items-center gap-1 rounded-full pr-1 pl-2.5 text-[12px] font-medium"
          style={{ background: colorOf(f.color).soft, color: colorOf(f.color).ink }}
        >
          <FolderGlyph icon={f.icon} color={f.color} className="size-3" />
          {f.name}
          <button
            onClick={() => move(thread, f, false)}
            aria-label={`Remove from ${f.name}`}
            className="rounded-full p-0.5 hover:bg-[rgb(0_0_0/0.06)]"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}
    </div>
  );
}

/** "Add to folder ▸" in a conversation's menu. */
export function AddToFolder({ thread, ids, folders }: { thread: number; ids: number[]; folders: MailFolder[] }) {
  const open = folders.filter((f) => !ids.includes(f.id));
  if (open.length === 0) return null;
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <FolderPlus /> Add to folder
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        {open.map((f) => (
          <DropdownMenuItem key={f.id} onSelect={() => move(thread, f, true)}>
            <FolderGlyph icon={f.icon} color={f.color} /> {f.name}
          </DropdownMenuItem>
        ))}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
