import { type RefObject, useEffect, useMemo, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

import type { Mention } from "@/bindings/Mention";
import type { MentionCandidate } from "@/bindings/MentionCandidate";
import { activeTrigger, tokenRanges } from "@/features/chat/mentions/ranges";
import { api, keys } from "@/lib/api";

/**
 * @ mentions in a plain textarea: tracks which "@label"s are tokens, opens the picker
 * while an "@query" is being typed, and makes tokens behave as one unit when deleted.
 */
export function useMentions({
  text,
  setText,
  area,
}: {
  text: string;
  setText: (t: string) => void;
  area: RefObject<HTMLTextAreaElement | null>;
}) {
  const [mentions, setMentions] = useState<Mention[]>([]);
  const [trigger, setTrigger] = useState<{ start: number; query: string } | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [highlight, setHighlight] = useState(0);

  const tokens = useMemo(() => tokenRanges(text, mentions), [text, mentions]);

  // Mentions whose "@label" was edited away are no longer mentions.
  useEffect(() => {
    if (tokens.length !== mentions.length) setMentions(tokens.map((t) => t.mention));
  }, [tokens, mentions.length]);

  const query = trigger?.query ?? "";
  const [debounced, setDebounced] = useState(query);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(query), 120);
    return () => clearTimeout(t);
  }, [query]);
  const open = trigger !== null && trigger.start !== dismissed;
  const results = useQuery({
    queryKey: keys.mentions(debounced),
    queryFn: () => api.mentions(debounced),
    enabled: open,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
  const candidates = useMemo(() => results.data ?? [], [results.data]);
  useEffect(() => setHighlight(0), [debounced]);

  /** Re-reads the "@query" at the caret. Call after input and caret moves. */
  const refresh = (value = text) => {
    const el = area.current;
    if (!el) return;
    const caret = el.selectionStart === el.selectionEnd ? el.selectionStart : -1;
    const next = caret < 0 ? null : activeTrigger(value, caret, tokenRanges(value, mentions));
    setTrigger(next);
    if (!next) setDismissed(null);
  };

  const pick = (c: MentionCandidate) => {
    const el = area.current;
    if (!trigger || !el) return;
    const caret = el.selectionStart;
    const inserted = `@${c.label} `;
    const next = text.slice(0, trigger.start) + inserted + text.slice(caret);
    setMentions((m) => [...m, { kind: c.kind, id: c.id, label: c.label }]);
    setText(next);
    setTrigger(null);
    const at = trigger.start + inserted.length;
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(at, at);
    });
  };

  /** Keyboard handling. Returns true when the key was used. */
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (open && candidates.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const step = e.key === "ArrowDown" ? 1 : -1;
        setHighlight((h) => (h + step + candidates.length) % candidates.length);
        return true;
      }
      if ((e.key === "Enter" || e.key === "Tab") && !e.shiftKey && !e.nativeEvent.isComposing) {
        e.preventDefault();
        pick(candidates[Math.min(highlight, candidates.length - 1)]);
        return true;
      }
    }
    if (open && e.key === "Escape") {
      e.preventDefault();
      setDismissed(trigger!.start);
      return true;
    }
    // A token deletes as a whole.
    const el = e.currentTarget;
    if ((e.key === "Backspace" || e.key === "Delete") && el.selectionStart === el.selectionEnd) {
      const caret = el.selectionStart;
      const token = tokens.find((t) =>
        e.key === "Backspace" ? caret > t.start && caret <= t.end : caret >= t.start && caret < t.end,
      );
      if (token) {
        e.preventDefault();
        // Take one neighbouring space along, so "a @Sam b" becomes "a b", not "a  b".
        const end = text[token.end] === " " && (token.start === 0 || text[token.start - 1] === " ") ? token.end + 1 : token.end;
        setText(text.slice(0, token.start) + text.slice(end));
        setMentions((m) => m.filter((x) => x !== token.mention));
        requestAnimationFrame(() => el.setSelectionRange(token.start, token.start));
        return true;
      }
    }
    return false;
  };

  /** Opens the picker from a button: types an "@" at the caret. */
  const start = () => {
    const el = area.current;
    if (!el) return;
    const caret = el.selectionStart ?? text.length;
    const needsSpace = caret > 0 && !/\s/.test(text[caret - 1]);
    const insert = needsSpace ? " @" : "@";
    const next = text.slice(0, caret) + insert + text.slice(caret);
    setText(next);
    const at = caret + insert.length;
    setTrigger({ start: at - 1, query: "" });
    setDismissed(null);
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(at, at);
    });
  };

  return {
    mentions: tokens.map((t) => t.mention),
    tokens,
    open,
    query,
    candidates,
    loading: results.isFetching && candidates.length === 0,
    highlight,
    setHighlight,
    pick,
    refresh,
    onKeyDown,
    start,
    close: () => trigger && setDismissed(trigger.start),
    restore: (m: Mention[]) => setMentions(m),
    reset: () => {
      setMentions([]);
      setTrigger(null);
    },
  };
}
