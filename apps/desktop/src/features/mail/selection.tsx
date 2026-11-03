import { useCallback } from "react";
import { keepPreviousData, type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Layers, Mail, MailOpen, Trash2, X } from "lucide-react";
import { toast } from "sonner";

import type { MailBatchAction } from "@/bindings/MailBatchAction";
import type { MailBox } from "@/bindings/MailBox";
import type { MailFolder } from "@/bindings/MailFolder";
import type { MailThread } from "@/bindings/MailThread";
import type { MailThreadDetail } from "@/bindings/MailThreadDetail";
import { IconTile } from "@/components/page";
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
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { FlagStar } from "@/features/mail/flag";
import { FolderGlyph } from "@/features/mail/folder-looks";
import { api, keys, type MailScope } from "@/lib/api";
import { isMac } from "@/lib/platform";

/**
 * Choosing several conversations and acting on them at once. ⌘/Ctrl-click adds or
 * removes one, Shift-click takes the range from the open one, ⌘/Ctrl A in the list (or X
 * on each) takes them all; with more than one chosen the reader shows what's chosen and
 * what can be done. Each action is one request (`POST /v1/mail/threads/batch`), done in
 * one session per mail account; what fails is said in plain words.
 */

const plural = (n: number) => (n === 1 ? "1 conversation" : `${n} conversations`);

/**
 * The conversations the list shows: the same query as the list (so the same cache), for
 * moving through it and choosing from it outside the list.
 */
export function useShownThreads(
  view: MailBox,
  query: string,
  scope: MailScope,
  folder: number | null,
  removing: ReadonlySet<number>,
  /** Only once the panel shows the list (an account is connected). */
  enabled: boolean,
): MailThread[] {
  const searching = query.trim().length > 0;
  const threads = useQuery({
    queryKey: keys.mailThreads(searching ? null : view, query, scope, searching ? null : folder),
    queryFn: () => api.mailThreads(searching ? null : view, query, scope, searching ? null : folder),
    placeholderData: keepPreviousData,
    enabled,
  });
  return (threads.data ?? []).filter((t) => !removing.has(t.id));
}

/** A row click with the keys held. */
interface Click {
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
}

/**
 * What a click on a row does, as in any list: ⌘/Ctrl-click adds or removes it,
 * Shift-click takes everything from the open conversation to it, a plain click opens
 * it alone. `chosen` stays empty unless at least two are chosen (or X started a choice);
 * `open` is the conversation in view, the anchor of Shift-click.
 */
export function clickChoice(
  list: number[],
  chosen: number[],
  open: number | null,
  id: number,
  e: Click | null,
): { chosen: number[]; open: number | null } {
  if (e && (e.metaKey || e.ctrlKey)) {
    const base = chosen.length > 0 ? chosen : open !== null ? [open] : [];
    const next = base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
    if (next.length > 1) return { chosen: next, open: id };
    return { chosen: [], open: next[0] ?? null };
  }
  const anchor = open ?? chosen[0] ?? null;
  if (e?.shiftKey && anchor !== null) {
    const from = list.indexOf(anchor);
    const to = list.indexOf(id);
    if (from !== -1 && to !== -1 && from !== to) {
      const [lo, hi] = from < to ? [from, to] : [to, from];
      return { chosen: list.slice(lo, hi + 1), open: anchor };
    }
  }
  return { chosen: [], open: id };
}

/** Whether a row shows as selected: the chosen ones, and the one open beside them. */
export function rowChosen(id: number, chosen: number[], open: number | null) {
  return chosen.includes(id) || (chosen.length < 2 && id === open);
}

// --- Acting on them --------------------------------------------------------------------

const LISTS = ["mail", "threads"] as const;

/** Changes the cached lists and conversations at once, before the daemon answers. */
function patchCached(qc: QueryClient, ids: number[], patch: Partial<MailThread>) {
  const set = new Set(ids);
  qc.setQueriesData<MailThread[]>({ queryKey: LISTS }, (old) => old?.map((t) => (set.has(t.id) ? { ...t, ...patch } : t)));
  for (const id of ids)
    qc.setQueryData<MailThreadDetail>(keys.mailThread(id), (old) =>
      old ? { ...old, thread: { ...old.thread, ...patch } } : old,
    );
}

/** What was done, for the toast; `null` where the list shows it plainly. */
function doneText(action: MailBatchAction, n: number, folders: MailFolder[]): string | null {
  const name = action.kind === "folder" ? (folders.find((f) => f.id === action.folder)?.name ?? "the folder") : "";
  switch (action.kind) {
    case "archive":
      return n === 1 ? "Archived" : `Archived ${plural(n)}`;
    case "delete":
      return n === 1 ? "Moved to the Trash" : `Moved ${plural(n)} to the Trash`;
    case "folder":
      return action.member
        ? `Added ${n === 1 ? "" : `${plural(n)} `}to “${name}”`
        : `Removed ${n === 1 ? "" : `${plural(n)} `}from “${name}”`;
    default:
      return null;
  }
}

/**
 * Runs one action on several conversations. Archived and deleted ones leave the list at
 * once (`hide`) and come back if the server refuses them (`show`); read and flag changes
 * show at once and go back the same way.
 */
export function useBulkMail({
  hide,
  show,
  folders,
}: {
  hide: (ids: number[]) => void;
  show: (ids: number[]) => void;
  folders: MailFolder[];
}) {
  const qc = useQueryClient();
  return useCallback(
    async (ids: number[], action: MailBatchAction) => {
      if (ids.length === 0) return;
      const leaves = action.kind === "archive" || action.kind === "delete";
      // A refresh already on its way could land with the old state.
      await qc.cancelQueries({ queryKey: keys.mail });
      if (leaves) hide(ids);
      else if (action.kind === "read") patchCached(qc, ids, { unread: !action.read });
      else if (action.kind === "flag") patchCached(qc, ids, { flagged: action.flagged });
      try {
        const result = await api.mailBatch(ids, action);
        const failed = result.failed.flatMap((f) => f.ids);
        if (failed.length > 0) {
          if (leaves) show(failed);
          toast.error(result.message ?? "Some conversations couldn't be changed.");
        } else {
          const text = doneText(action, ids.length, folders);
          if (text) toast.success(text);
        }
      } catch (e) {
        if (leaves) show(ids);
        toast.error((e as Error).message);
      }
      await qc.invalidateQueries({ queryKey: keys.mail });
      // Gone from the lists now, or back in them (archiving from Flagged keeps them there).
      if (leaves) show(ids);
    },
    [qc, hide, show, folders],
  );
}

/** What can be done to the chosen conversations, for the summary and the menus. */
export interface Bulk {
  ids: number[];
  /** The chosen conversations the list shows. */
  threads: MailThread[];
  run: (action: MailBatchAction) => void;
  askDelete: () => void;
  clear: () => void;
  /** Archiving means something here (not in the Archive itself). */
  canArchive: boolean;
}

const anyUnread = (b: Bulk) => b.threads.some((t) => t.unread);
const anyUnflagged = (b: Bulk) => b.threads.some((t) => !t.flagged);

/** The reader's place while several conversations are chosen. */
export function SelectionSummary({
  bulk,
  folders,
  folder,
}: {
  bulk: Bulk;
  folders: MailFolder[];
  /** The smart folder shown, which they can be taken out of. */
  folder: MailFolder | null;
}) {
  const unread = anyUnread(bulk);
  const unflagged = anyUnflagged(bulk);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-6 pt-[60px] text-center">
      <IconTile size="lg" className="bg-fill text-muted-foreground">
        <Layers />
      </IconTile>
      <div className="flex flex-col gap-1">
        <h2 className="type-headline">{plural(bulk.ids.length)} selected</h2>
        <p className="type-callout text-muted-foreground">
          {isMac ? "⌘" : "Ctrl"}-click to add or remove one, Shift-click to take a range. Esc clears.
        </p>
      </div>
      <div className="flex max-w-[460px] flex-wrap justify-center gap-2">
        {bulk.canArchive && (
          <Button variant="secondary" size="sm" onClick={() => bulk.run({ kind: "archive" })}>
            <Archive /> Archive
          </Button>
        )}
        <Button variant="secondary" size="sm" onClick={() => bulk.run({ kind: "read", read: unread })}>
          {unread ? <MailOpen /> : <Mail />} {unread ? "Mark as read" : "Mark as unread"}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => bulk.run({ kind: "flag", flagged: unflagged })}>
          <FlagStar flagged={false} /> {unflagged ? "Flag" : "Remove flag"}
        </Button>
        {(folders.length > 0 || folder) && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="secondary" size="sm">
                <FolderGlyph icon="folder" color="gray" /> Add to folder
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="center">
              {folders.map((f) => (
                <DropdownMenuItem key={f.id} onSelect={() => bulk.run({ kind: "folder", folder: f.id, member: true })}>
                  <FolderGlyph icon={f.icon} color={f.color} /> {f.name}
                </DropdownMenuItem>
              ))}
              {folder && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onSelect={() => bulk.run({ kind: "folder", folder: folder.id, member: false })}>
                    <X /> Remove from “{folder.name}”
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
        <Button variant="secondary" size="sm" className="text-destructive" onClick={bulk.askDelete}>
          <Trash2 /> Delete…
        </Button>
      </div>
      <Button variant="ghost" size="sm" className="text-muted-foreground" onClick={bulk.clear}>
        Clear selection
      </Button>
    </div>
  );
}

/** A chosen row's right-click menu: the same actions, on all of them. */
export function BulkMenuItems({ bulk, folders, folder }: { bulk: Bulk; folders: MailFolder[]; folder: MailFolder | null }) {
  const unread = anyUnread(bulk);
  const unflagged = anyUnflagged(bulk);
  const n = bulk.ids.length;
  return (
    <>
      {bulk.canArchive && (
        <ContextMenuItem onSelect={() => bulk.run({ kind: "archive" })}>
          <Archive /> Archive {plural(n)}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={() => bulk.run({ kind: "read", read: unread })}>
        {unread ? <MailOpen /> : <Mail />} {unread ? "Mark as read" : "Mark as unread"}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => bulk.run({ kind: "flag", flagged: unflagged })}>
        <FlagStar flagged={false} /> {unflagged ? "Flag" : "Remove flag"}
      </ContextMenuItem>
      {folders.length > 0 && (
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <FolderGlyph icon="folder" color="gray" /> Add to folder
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {folders.map((f) => (
              <ContextMenuItem key={f.id} onSelect={() => bulk.run({ kind: "folder", folder: f.id, member: true })}>
                <FolderGlyph icon={f.icon} color={f.color} /> {f.name}
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
      )}
      {folder && (
        <ContextMenuItem onSelect={() => bulk.run({ kind: "folder", folder: folder.id, member: false })}>
          <X /> Remove from “{folder.name}”
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={bulk.clear}>
        <X /> Clear selection
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem variant="destructive" onSelect={bulk.askDelete}>
        <Trash2 /> Delete {plural(n)}…
      </ContextMenuItem>
    </>
  );
}

/** Confirms deleting one or several conversations: they go to the account's Trash. */
export function DeleteConversations({
  ids,
  subject,
  onDelete,
  onCancel,
}: {
  /** What's being deleted; `null` when nothing is. */
  ids: number[] | null;
  /** The subject, when it's one conversation. */
  subject: string | null;
  onDelete: (ids: number[]) => void;
  onCancel: () => void;
}) {
  const many = (ids?.length ?? 0) > 1;
  return (
    <AlertDialog open={ids !== null} onOpenChange={(o) => !o && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{many ? `Delete ${plural(ids!.length)}?` : "Delete this conversation?"}</AlertDialogTitle>
          <AlertDialogDescription>
            {many
              ? "They move to the Trash in your mail account, where you can still get them back from your usual mail app."
              : `“${subject || "(no subject)"}” moves to the Trash in your mail account, where you can still get it back from your usual mail app.`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => ids && onDelete(ids)}>
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
