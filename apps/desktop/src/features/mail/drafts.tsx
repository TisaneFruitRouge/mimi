import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FilePen, MailOpen, Paperclip, Trash2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import type { MailDraftInfo } from "@/bindings/MailDraftInfo";
import { IconTile } from "@/components/page";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { textOfDraftHtml } from "@/features/mail/body-editor";
import { reopenDraft } from "@/features/mail/send-later";
import { api, keys } from "@/lib/api";

/*
 * Saved drafts. The daemon keeps them (see the Email doc, "Drafts"): saved as the user
 * writes, copied to the mail server's Drafts folder so other mail apps see them, and
 * drafts written in those apps listed here too. This file saves as the user writes
 * (`useDraftAutosave`), and shows the Drafts view.
 */

/** A new draft's id: a UUID (v4), made here so the editor can save from its first change. */
export function newDraftId(): string {
  // Not `crypto.randomUUID`: a browser reaching the daemon over plain http has none.
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

/** The draft with an id, so it's saved under one from the start. */
export function withDraftId(d: MailDraft): MailDraft;
export function withDraftId(d: MailDraft | null): MailDraft | null;
export function withDraftId(d: MailDraft | null): MailDraft | null {
  return d && !d.draft_id ? { ...d, draft_id: newDraftId() } : d;
}

export const useMailDrafts = () => useQuery({ queryKey: keys.mailDrafts, queryFn: api.mailDrafts });

/** The conversations with a reply being written, for their "Draft" mark in the list. */
export function useDraftReplies(): ReadonlySet<number> {
  const drafts = useMailDrafts().data ?? [];
  return new Set(drafts.flatMap((d) => (d.reply_to === null ? [] : [d.reply_to])));
}

/**
 * A saved draft, ready for the editor. One written in another mail app gets its text
 * from its HTML, as the editor reads it, so the formatting is kept.
 */
export async function continueDraft(info: MailDraftInfo): Promise<MailDraft> {
  const d = await api.mailDraft(info.id);
  if (info.from_elsewhere && d.html) return { ...d, body: textOfDraftHtml(d.html, d.attachments) };
  return d;
}

// --- Saving as the user writes ---------------------------------------------------------

/** How long the writing pauses before the draft is saved. */
const SAVE_AFTER_MS = 800;
/**
 * Changes this soon after a draft opens are the editor's own (a signature put in, the
 * text read in), not the user's writing: they don't make it a saved draft.
 */
const SETTLING_MS = 500;

/** What a draft says, without which saved draft it is. */
const contentOf = (d: MailDraft) => JSON.stringify({ ...d, draft_id: undefined });

/** Nothing written: a reply with no text yet, a new message with nothing in it. */
function blank(d: MailDraft) {
  const nobody = d.to.length + d.cc.length + d.bcc.length === 0;
  return (
    !d.body.trim() &&
    d.attachments.length === 0 &&
    d.forward_of === null &&
    (d.reply_to !== null || (nobody && !d.subject.trim()))
  );
}

export interface DraftSaving {
  /** "saved" once something written here is kept in Drafts. */
  state: "idle" | "saving" | "saved" | "error";
  /** Send was pressed: nothing is saved until it's known whether it went. */
  hold: () => void;
  /** It didn't go: saving carries on. */
  release: () => void;
  /** It's queued: the daemon took it out of Drafts, so nothing more is saved. */
  sent: () => void;
  /** Discard: the saved draft is deleted, with Undo. */
  discard: () => void;
}

/**
 * Saves a draft as it's written: a moment after each change, and when its editor closes
 * (the daemon then copies it to the server at once). Nothing is saved until something
 * changes, so opening a draft or a reply box saves nothing; a draft emptied out is
 * deleted when its editor closes. Drafts without an id get one through `onChange`.
 * `origin` is where Undo of a Discard puts it back (`useDraftHome`).
 */
export function useDraftAutosave(
  draft: MailDraft | null,
  onChange: (d: MailDraft) => void,
  origin: string,
): DraftSaving {
  const [state, setState] = useState<DraftSaving["state"]>("idle");
  const latest = useRef(draft);
  latest.current = draft;
  const change = useRef(onChange);
  change.current = onChange;
  // The draft this editor holds, what of it is saved (or was there to begin with),
  // whether anything was saved from here, and whether saving goes on.
  const track = useRef({ id: null as string | null, saved: "", wrote: false, on: true, since: 0 });
  const timer = useRef<number | undefined>(undefined);

  const save = (closing: boolean) => {
    window.clearTimeout(timer.current);
    const d = latest.current;
    const t = track.current;
    if (!d?.draft_id || d.draft_id !== t.id || !t.on) return;
    const id = d.draft_id;
    const content = contentOf(d);
    if (content === t.saved) {
      if (closing && t.wrote) api.mirrorMailDraft(id).catch(() => {});
      return;
    }
    if (blank(d)) {
      // Emptied out: nothing to keep once it's closed.
      if (closing) api.deleteMailDraft(id).catch(() => {});
      return;
    }
    t.saved = content;
    t.wrote = true;
    if (!closing) setState("saving");
    api.saveMailDraft(id, d, closing).then(
      () => {
        if (!closing && track.current.id === id) setState("saved");
      },
      (e) => {
        if (track.current.id === id) track.current.saved = "";
        if (!closing) setState("error");
        else
          toast.error("Your draft couldn't be saved", {
            description: (e as Error).message,
            action: { label: "Open it", onClick: () => reopenDraft(origin, d) },
            duration: 30_000,
          });
      },
    );
  };
  const saveRef = useRef(save);
  saveRef.current = save;

  useEffect(() => {
    if (!draft) return;
    if (!draft.draft_id) {
      change.current(withDraftId(draft));
      return;
    }
    const t = track.current;
    if (draft.draft_id !== t.id) {
      track.current = { id: draft.draft_id, saved: contentOf(draft), wrote: false, on: true, since: Date.now() };
      setState("idle");
      return;
    }
    if (!t.on) return;
    if (!t.wrote && Date.now() - t.since < SETTLING_MS) {
      t.saved = contentOf(draft);
      return;
    }
    window.clearTimeout(timer.current);
    if (contentOf(draft) !== t.saved) timer.current = window.setTimeout(() => saveRef.current(false), SAVE_AFTER_MS);
  }, [draft]);

  // Closing the editor saves what's left, and has it copied to the server now.
  useEffect(() => () => saveRef.current(true), []);

  const forget = () => {
    window.clearTimeout(timer.current);
    track.current = { id: null, saved: "", wrote: false, on: true, since: 0 };
    setState("idle");
  };
  return {
    state,
    hold: () => {
      window.clearTimeout(timer.current);
      track.current.on = false;
    },
    release: () => {
      track.current.on = true;
      saveRef.current(false);
    },
    sent: forget,
    discard: () => {
      const d = latest.current;
      forget();
      if (!d?.draft_id) return;
      const id = d.draft_id;
      api.deleteMailDraft(id).catch(() => {});
      if (blank(d)) return;
      toast("Draft deleted", {
        action: {
          label: "Undo",
          onClick: () =>
            api.saveMailDraft(id, d).then(
              () => reopenDraft(origin, d),
              (e) => toast.error((e as Error).message),
            ),
        },
      });
    },
  };
}

/** A quiet line saying the draft is kept: "Saved to Drafts". */
export function DraftSavedNote({ saving, fallback }: { saving: DraftSaving; fallback?: string }) {
  const text =
    saving.state === "saved" || saving.state === "saving"
      ? "Saved to Drafts"
      : saving.state === "error"
        ? "Not saved yet. Mimi will try again as you write."
        : fallback;
  if (!text) return <span />;
  return (
    <span className={cn("type-footnote", saving.state === "error" ? "text-destructive" : "text-faint")}>{text}</span>
  );
}

// --- The Drafts view -------------------------------------------------------------------

/** The sidebar's "Drafts" entry, with how many there are. */
export function DraftsNavItem({ active, onClick }: { active: boolean; onClick: () => void }) {
  const n = useMailDrafts().data?.length ?? 0;
  return (
    <button
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-9 items-center gap-2.5 rounded-[8px] px-2.5 text-left type-callout transition-colors",
        active ? "bg-fill font-medium" : "text-foreground/85 hover:bg-[rgb(118_118_128/0.07)]",
      )}
    >
      <FilePen className={cn("size-4 shrink-0", active ? "text-foreground" : "text-muted-foreground")} />
      <span className="min-w-0 flex-1 truncate">Drafts</span>
      {n > 0 && <span className="type-footnote font-medium text-muted-foreground tabular-nums">{n}</span>}
    </button>
  );
}

function changed(ms: number) {
  const d = new Date(ms);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (d.getFullYear() !== now.getFullYear())
    return d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
}

const toLine = (to: string[]) =>
  to.length === 0 ? "No recipients yet" : `To ${to.map((a) => a.replace(/\s*<[^>]*>$/, "")).join(", ")}`;

/** Deletes a draft from the list, with Undo (it's saved again, files and all). */
async function deleteDraft(info: MailDraftInfo) {
  try {
    const d = await api.mailDraft(info.id).catch(() => null);
    await api.deleteMailDraft(info.id);
    toast("Draft deleted", {
      action: d
        ? {
            label: "Undo",
            onClick: () => api.saveMailDraft(info.id, d).catch((e) => toast.error((e as Error).message)),
          }
        : undefined,
    });
  } catch (e) {
    toast.error((e as Error).message);
  }
}

/** The middle column in the Drafts view: every draft, latest first. */
export function DraftList({
  selected,
  onOpen,
}: {
  selected: string | null;
  onOpen: (d: MailDraftInfo) => void;
}) {
  const drafts = useMailDrafts();
  const list = drafts.data ?? [];
  return (
    <div className="flex h-full w-[360px] shrink-0 flex-col pt-[68px] shadow-[inset_-0.5px_0_var(--separator)] max-lg:w-[320px]">
      <div className="px-4 pb-3">
        <h2 className="type-title">Drafts</h2>
        <p className="type-subhead text-muted-foreground">
          Emails you started. Your other mail apps see them too.
        </p>
      </div>
      <div role="listbox" aria-label="Drafts" className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-4">
        {!drafts.isLoading && list.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-6 pt-16 text-center type-callout text-muted-foreground">
            No drafts. What you write is kept here until you send it.
          </div>
        )}
        {list.map((d) => (
          <ContextMenu key={d.id}>
            <ContextMenuTrigger asChild>
              <button
                role="option"
                aria-selected={d.id === selected}
                onClick={() => onOpen(d)}
                className={cn(
                  "flex flex-col gap-0.5 rounded-[10px] px-3 py-2.5 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/45",
                  d.id === selected ? "bg-[rgb(118_118_128/0.14)]" : "hover:bg-[rgb(118_118_128/0.07)]",
                )}
              >
                <span className="flex items-baseline gap-2">
                  <span className="min-w-0 flex-1 truncate type-callout font-medium">{toLine(d.to)}</span>
                  <span className="shrink-0 type-footnote text-faint tabular-nums">{changed(d.updated_at)}</span>
                </span>
                <span className="flex items-center gap-1.5">
                  <span className="min-w-0 flex-1 truncate type-subhead">{d.subject || "No subject"}</span>
                  {d.files > 0 && <Paperclip className="size-3.5 shrink-0 text-faint" aria-label="Has files" />}
                </span>
                <span className="line-clamp-2 type-subhead text-muted-foreground">
                  {d.snippet || (d.from_elsewhere ? "Written in another mail app" : "No text yet")}
                </span>
              </button>
            </ContextMenuTrigger>
            <ContextMenuContent className="w-[200px]">
              <ContextMenuItem onSelect={() => onOpen(d)}>
                <MailOpen /> Open
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem variant="destructive" onSelect={() => void deleteDraft(d)}>
                <Trash2 /> Delete draft
              </ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        ))}
      </div>
    </div>
  );
}

/** The reader side of the Drafts view before one is chosen. */
export function NoDraftOpen() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 pt-[60px] text-center">
      <IconTile size="lg" className="bg-fill text-muted-foreground">
        <FilePen />
      </IconTile>
      <p className="type-headline">Choose a draft</p>
      <p className="max-w-sm type-callout text-muted-foreground">
        Carry on where you left off. A reply opens in its conversation.
      </p>
    </div>
  );
}
