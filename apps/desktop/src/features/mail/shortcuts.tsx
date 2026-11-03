import { useEffect, useRef } from "react";

import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { hasMod, isMac } from "@/lib/platform";

/**
 * Keyboard shortcuts in the Mail panel, as mail apps have them: J and K to move, E to
 * archive, R to reply… They never fire while the user types (a field, the reply box, an
 * editor) or while a dialog or menu is open, and they leave other modifiers to the
 * app's own shortcuts (⌘/Ctrl K, N, 1–4…). Where several conversations are chosen, the
 * actions apply to all of them.
 */

export interface MailShortcutHandlers {
  next: () => void;
  previous: () => void;
  /** ↓ and ↑ outside the list: only when no conversation is open (else they scroll it). */
  arrows: boolean;
  open: () => void;
  /** Esc: clears the selection, else closes the conversation. */
  back: () => void;
  reply: () => void;
  replyAll: () => void;
  forward: () => void;
  compose: () => void;
  archive: () => void;
  remove: () => void;
  toggleRead: () => void;
  toggleFlag: () => void;
  search: () => void;
  /** X: adds the conversation in view to the selection, or takes it out. */
  choose: () => void;
  /** ⌘/Ctrl A in the list. */
  chooseAll: () => void;
  help: () => void;
  /** While a new message is being written, only `?` works: nothing may discard it. */
  writing: boolean;
}

/** Whether the user is typing: a field, a text area, an editor. */
function typing(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || !!target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])");
}

/** Whether a dialog, sheet or menu is open: its keys are its own. */
function overlayOpen() {
  return !!document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');
}

/** The conversation list, where ⌘/Ctrl A selects every conversation in it. */
export const LIST_SELECTOR = '[role="listbox"][aria-label="Conversations"]';

/**
 * Listens for the Mail panel's shortcuts while it's shown. Set the returned ref's
 * `current` to the handlers on each render (`null`: none, while the panel is loading).
 */
export function useMailShortcuts() {
  const latest = useRef<MailShortcutHandlers | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const h = latest.current;
      if (!h || e.defaultPrevented || e.isComposing || e.altKey) return;
      if (typing(e.target) || overlayOpen()) return;
      // The other modifier (Ctrl on macOS, the Windows/Super key elsewhere) is never ours.
      if (isMac ? e.ctrlKey : e.metaKey) return;
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const target = e.target instanceof Element ? e.target : null;
      const run = (f: () => void) => {
        e.preventDefault();
        f();
      };
      if (key === "?") return run(h.help);
      if (h.writing) return;
      if (hasMod(e)) {
        if (key === "f" && !e.shiftKey) run(h.search);
        else if (key === "u" && e.shiftKey) run(h.toggleRead);
        else if (key === "a" && !e.shiftKey && target?.closest(LIST_SELECTOR)) run(h.chooseAll);
        return;
      }
      switch (key) {
        case "j":
          return run(h.next);
        case "k":
          return run(h.previous);
        case "ArrowDown":
          return h.arrows ? run(h.next) : undefined;
        case "ArrowUp":
          return h.arrows ? run(h.previous) : undefined;
        case "Enter":
          // A focused button or link does its own thing.
          if (target?.closest("button, a, [role=button], [role=option], summary")) return;
          return run(h.open);
        case "Escape":
          return run(h.back);
        case "r":
          return run(h.reply);
        case "a":
          return run(h.replyAll);
        case "f":
          return run(h.forward);
        case "c":
        case "n":
          return run(h.compose);
        case "e":
          return run(h.archive);
        case "#":
        case "Delete":
        case "Backspace":
          return run(h.remove);
        case "u":
          return run(h.toggleRead);
        case "s":
          return run(h.toggleFlag);
        case "/":
          return run(h.search);
        case "x":
          return run(h.choose);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return latest;
}

// --- The sheet ------------------------------------------------------------------------

const MOD = isMac ? "⌘" : "Ctrl";
const SHIFT = isMac ? "⇧" : "Shift";

/** Each shortcut: what it does, and its keys (any one of the combinations works). */
const GROUPS: { title: string; items: { label: string; keys: string[][] }[] }[] = [
  {
    title: "Moving around",
    items: [
      { label: "Next conversation", keys: [["J"], ["↓"]] },
      { label: "Previous conversation", keys: [["K"], ["↑"]] },
      { label: "Open the conversation", keys: [["↵"]] },
      { label: "Close it, or clear the selection", keys: [["Esc"]] },
      { label: "Search your mail", keys: [["/"], [MOD, "F"]] },
    ],
  },
  {
    title: "Writing",
    items: [
      { label: "Reply", keys: [["R"]] },
      { label: "Reply to everyone", keys: [["A"]] },
      { label: "Forward", keys: [["F"]] },
      { label: "New message", keys: [["C"], ["N"]] },
    ],
  },
  {
    title: "Tidying up",
    items: [
      { label: "Archive", keys: [["E"]] },
      { label: "Delete", keys: [[isMac ? "⌫" : "Delete"], ["#"]] },
      { label: "Mark as read or unread", keys: [["U"], [SHIFT, MOD, "U"]] },
      { label: "Flag, or remove the flag", keys: [["S"]] },
    ],
  },
  {
    title: "Choosing several",
    items: [
      { label: "Add to the selection, or take out", keys: [["X"]] },
      { label: "Select every conversation in the list", keys: [[MOD, "A"]] },
      { label: "Choose one more", keys: [[`${MOD}-click`]] },
      { label: "Choose all in between", keys: [[`${SHIFT}-click`]] },
    ],
  },
];

function Keys({ keys }: { keys: string[][] }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5">
      {keys.map((combo, i) => (
        <span key={i} className="flex items-center gap-1.5">
          {i > 0 && <span className="type-footnote text-faint">or</span>}
          <span className="flex items-center gap-0.5">
            {combo.map((k) => (
              <kbd
                key={k}
                className="inline-flex h-[22px] min-w-[22px] items-center justify-center rounded-[6px] bg-fill px-1.5 font-sans type-footnote font-medium text-foreground/80"
              >
                {k}
              </kbd>
            ))}
          </span>
        </span>
      ))}
    </span>
  );
}

/** "Keyboard shortcuts": every Mail shortcut in plain words, opened with ? or a button. */
export function ShortcutsSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>
            In Mail, when you aren't typing. With several conversations chosen, they act on all of them.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-5">
          {GROUPS.map((g) => (
            <section key={g.title} className="flex flex-col gap-1.5">
              <h3 className="section-label">{g.title}</h3>
              <div className="grouped">
                {g.items.map((item) => (
                  <div key={item.label} className="flex min-h-10 items-center justify-between gap-4 px-4 py-1.5 type-callout">
                    <span>{item.label}</span>
                    <Keys keys={item.keys} />
                  </div>
                ))}
              </div>
            </section>
          ))}
          <p className="type-footnote text-faint">Press ? in Mail to see these again.</p>
        </div>
      </DialogContent>
    </Dialog>
  );
}
