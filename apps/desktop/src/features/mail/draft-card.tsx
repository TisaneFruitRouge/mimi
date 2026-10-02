import { useState } from "react";
import { motion } from "motion/react";
import { Check, Mail } from "lucide-react";

import type { Action } from "@/bindings/Action";
import type { MailDraft } from "@/bindings/MailDraft";
import { DraftEditor, SendButton, useSendDraft } from "@/features/mail/draft-editor";

/** Tools that leave a draft in the chat for the user to check and send. */
export const DRAFT_TOOLS = new Set(["mail_draft_reply", "mail_compose"]);

export function draftOf(a: Action): MailDraft | null {
  const d = (a.output as { draft?: MailDraft } | null)?.draft;
  // Drafts from before Bcc and attachments existed have neither.
  return d && Array.isArray(d.to) ? { ...d, bcc: d.bcc ?? [], attachments: d.attachments ?? [] } : null;
}

// Drafts sent from this browser, so reopening the chat doesn't offer to send them again.
const SENT_KEY = "mimi.mail.sent-drafts";
function sentBefore(id: string): boolean {
  try {
    return (JSON.parse(localStorage.getItem(SENT_KEY) ?? "[]") as string[]).includes(id);
  } catch {
    return false;
  }
}
function rememberSent(id: string) {
  try {
    const list = (JSON.parse(localStorage.getItem(SENT_KEY) ?? "[]") as string[]).slice(-200);
    localStorage.setItem(SENT_KEY, JSON.stringify([...list, id]));
  } catch {
    // Only a convenience.
  }
}

/** A draft email in the chat: editable, and sent only when the user presses Send. */
export function DraftCard({ action }: { action: Action }) {
  const initial = draftOf(action);
  const [draft, setDraft] = useState<MailDraft | null>(initial);
  const [sent, setSent] = useState(() => sentBefore(action.id));
  const { sending, send } = useSendDraft(() => {
    rememberSent(action.id);
    setSent(true);
  });
  if (!draft) return null;
  if (sent) {
    return (
      <div className="flex items-center gap-3 rounded-[14px] bg-background px-3.5 py-2.5 type-callout shadow-[var(--shadow-card)]">
        <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-private-soft text-private">
          <Check className="size-3" strokeWidth={3} />
        </span>
        <span className="min-w-0 flex-1 truncate">
          Sent “{draft.subject || "(no subject)"}” to {draft.to.join(", ")}
        </span>
      </div>
    );
  }
  return (
    <motion.div
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ type: "spring", stiffness: 420, damping: 32 }}
      className="overflow-hidden rounded-[20px] bg-background shadow-[var(--shadow-raised)]"
    >
      <div className="flex items-center gap-2.5 px-4 pt-3.5 pb-2">
        <span className="flex size-7 items-center justify-center rounded-[8px] bg-[#efe9fb] text-[#6146ad]">
          <Mail className="size-4" />
        </span>
        <span className="type-subhead font-medium text-muted-foreground">Draft email · not sent</span>
      </div>
      <DraftEditor draft={draft} onChange={setDraft} className="border-t-[0.5px] border-separator" />
      <div className="flex items-center justify-between gap-3 border-t-[0.5px] border-separator bg-subtle/60 px-4 py-2.5">
        <span className="type-footnote text-faint">Check it over: it goes out only when you press Send.</span>
        <SendButton sending={sending} onClick={() => send(draft)} disabled={draft.to.length === 0} />
      </div>
    </motion.div>
  );
}
