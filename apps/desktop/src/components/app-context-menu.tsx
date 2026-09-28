import { useEffect, useState } from "react";
import { Copy, MessageSquarePlus, PanelRightOpen, Search, Settings, Sparkles } from "lucide-react";

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { Draft } from "@/lib/draft";
import { mod } from "@/lib/platform";
import { useAssistantName } from "@/lib/queries";

/**
 * Right-clicks open Mimi's own menus, never the webview's (Back, Reload…).
 *
 * Things with a menu of their own (a conversation in the mail list, a message, a
 * folder, a chat message…) wrap themselves in `ContextMenu`; the innermost one wins.
 * Everywhere else this one opens: Copy and Ask about the selected text, and the app's
 * main commands. Text fields keep the system's editing menu (cut, copy, paste,
 * spelling), which is the relevant one there.
 */
export function AppContextMenu({
  children,
  onNewChat,
  onSearch,
  onSettings,
  onAsk,
}: {
  children: React.ReactNode;
  onNewChat: () => void;
  onSearch: () => void;
  onSettings: () => void;
  onAsk: (draft: Draft) => void;
}) {
  const assistant = useAssistantName();
  const [selection, setSelection] = useState("");

  // In text fields, stop the event before React sees it: no menu of ours opens, and the
  // default isn't prevented, so the system's editing menu shows.
  useEffect(() => {
    const onMenu = (e: MouseEvent) => {
      if (isEditable(e.target)) e.stopPropagation();
    };
    window.addEventListener("contextmenu", onMenu, true);
    return () => window.removeEventListener("contextmenu", onMenu, true);
  }, []);

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild onContextMenu={() => setSelection(selectedText())}>
        <div className="contents">{children}</div>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-[220px]">
        {selection && (
          <>
            <ContextMenuItem onSelect={() => copyText(selection)}>
              <Copy /> Copy
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => onAsk(quoteDraft(selection))}>
              <Sparkles /> Ask {assistant} about this
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        <ContextMenuItem onSelect={onNewChat}>
          <MessageSquarePlus /> New chat <ContextMenuShortcut>{mod}N</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuItem onSelect={onSearch}>
          <Search /> Search <ContextMenuShortcut>{mod}K</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuItem onSelect={onSettings}>
          <Settings /> Settings <ContextMenuShortcut>{mod},</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function isEditable(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement) return !target.readOnly;
  if (target instanceof HTMLInputElement) {
    return !target.readOnly && !["button", "checkbox", "radio", "range", "submit", "reset", "color", "file"].includes(target.type);
  }
  return false;
}

/** The text selected in the page, if any. */
export function selectedText() {
  return window.getSelection()?.toString().trim() ?? "";
}

/** Copies text, with the old way as a fallback where the clipboard API is refused. */
export function copyText(text: string) {
  navigator.clipboard?.writeText(text).catch(() => document.execCommand("copy"));
}

/** A new chat that starts by quoting `text`, for "Ask … about this". */
export function quoteDraft(text: string): Draft {
  const quoted = text
    .slice(0, 4000)
    .split("\n")
    .map((l) => `> ${l}`)
    .join("\n");
  return { text: `${quoted}\n\n`, mentions: [] };
}

/**
 * Right-click on a thing (a person, an event…): open it, or start a chat about it.
 * `children` must be a single element that can hold a ref.
 */
export function ThingMenu({
  children,
  onOpen,
  onAsk,
  more,
}: {
  children: React.ReactElement;
  onOpen: () => void;
  onAsk?: () => void;
  /** Further items (e.g. a destructive one), shown after a separator. */
  more?: React.ReactNode;
}) {
  const assistant = useAssistantName();
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-[210px]">
        <ContextMenuItem onSelect={onOpen}>
          <PanelRightOpen /> Open
        </ContextMenuItem>
        {onAsk && (
          <ContextMenuItem onSelect={onAsk}>
            <Sparkles /> Ask {assistant} about this
          </ContextMenuItem>
        )}
        {more && (
          <>
            <ContextMenuSeparator />
            {more}
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
