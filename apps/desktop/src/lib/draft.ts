import type { Mention } from "@/bindings/Mention";

/**
 * A message waiting to be put in the composer, e.g. from "Ask Mimi about this" on a
 * panel. The panel sets it and opens a new chat; the chat takes it once, on mount.
 */
export interface Draft {
  text: string;
  mentions: Mention[];
}

let pending: Draft | null = null;

export function setDraft(draft: Draft) {
  pending = draft;
}

export function takeDraft(): Draft | null {
  const draft = pending;
  pending = null;
  return draft;
}

/** What a mention is typed with: @ for people and events, # for email. */
export function mentionSigil(kind: Mention["kind"]): "@" | "#" {
  return kind === "mail_thread" || kind === "mail_message" ? "#" : "@";
}

/** "@Label " (or "#Subject " for email), with the mention that makes it a pill. */
export function mentionDraft(kind: Mention["kind"], id: string, label: string): Draft {
  return { text: `${mentionSigil(kind)}${label} `, mentions: [{ kind, id, label }] };
}
