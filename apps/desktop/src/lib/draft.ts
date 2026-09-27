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

/** "@Label " for a person or an event, with the mention that makes it a pill. */
export function mentionDraft(kind: Mention["kind"], id: string, label: string): Draft {
  return { text: `@${label} `, mentions: [{ kind, id, label }] };
}
