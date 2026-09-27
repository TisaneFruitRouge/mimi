import type { Mention } from "@/bindings/Mention";
import type { MentionSigil } from "@/lib/api";
import { mentionSigil } from "@/lib/draft";

export interface TokenRange {
  start: number;
  end: number;
  mention: Mention;
}

/** A letter, digit or underscore: an "@Sam" followed by one of these isn't a token. */
const wordChar = /[\p{L}\p{N}_]/u;

/**
 * Where each mention's "@label" (or "#subject" for email) sits in `text`, first
 * occurrence per mention, never overlapping, sorted. Mentions whose text is gone have
 * no range.
 */
export function tokenRanges(text: string, mentions: Mention[]): TokenRange[] {
  const ranges: TokenRange[] = [];
  // Longer labels first, so "@Sam Carter" wins over "@Sam".
  const ordered = [...mentions].sort((a, b) => b.label.length - a.label.length);
  for (const mention of ordered) {
    const needle = `${mentionSigil(mention.kind)}${mention.label}`;
    let from = 0;
    while (from <= text.length) {
      const start = text.indexOf(needle, from);
      if (start === -1) break;
      const end = start + needle.length;
      const clash = ranges.some((r) => start < r.end && end > r.start);
      const glued = end < text.length && wordChar.test(text[end]);
      if (!clash && !glued) {
        ranges.push({ start, end, mention });
        break;
      }
      from = start + 1;
    }
  }
  return ranges.sort((a, b) => a.start - b.start);
}

/** Text split into plain runs and mention tokens, for drawing pills. */
export function segments(text: string, mentions: Mention[]) {
  const out: ({ kind: "text"; text: string } | { kind: "token"; text: string; mention: Mention })[] = [];
  let at = 0;
  for (const r of tokenRanges(text, mentions)) {
    if (r.start > at) out.push({ kind: "text", text: text.slice(at, r.start) });
    out.push({ kind: "token", text: text.slice(r.start, r.end), mention: r.mention });
    at = r.end;
  }
  if (at < text.length) out.push({ kind: "text", text: text.slice(at) });
  return out;
}

/**
 * The "@query" or "#query" being typed at the caret, if any: an @ or # at the start or
 * after a space or bracket, not inside an existing token, followed by at most a few
 * words.
 */
export function activeTrigger(
  text: string,
  caret: number,
  tokens: TokenRange[],
): { start: number; query: string; sigil: MentionSigil } | null {
  const before = text.slice(0, caret);
  const at = Math.max(before.lastIndexOf("@"), before.lastIndexOf("#"));
  if (at === -1) return null;
  const sigil = before[at] as MentionSigil;
  if (at > 0 && !/[\s([{"'“]/.test(before[at - 1])) return null;
  if (tokens.some((t) => at >= t.start && at < t.end)) return null;
  const query = before.slice(at + 1);
  if (query.length > 40 || /\n/.test(query) || /^\s/.test(query) || /\s{2,}$/.test(query)) return null;
  return { start: at, query, sigil };
}
