import { useState } from "react";
import { motion } from "motion/react";
import { Check, Mail } from "lucide-react";

import type { Action } from "@/bindings/Action";
import type { MailDraft } from "@/bindings/MailDraft";
import { DraftEditor, SendButton, useSendDraft } from "@/features/mail/draft-editor";
import { useDraftHome } from "@/features/mail/send-later";
import { useSignature } from "@/features/mail/use-signature";
import { when } from "@/features/reminders/time";

/** Tools that leave a draft in the chat for the user to check and send. */
export const DRAFT_TOOLS = new Set(["mail_draft_reply", "mail_compose"]);

export function draftOf(a: Action): MailDraft | null {
  const d = (a.output as { draft?: MailDraft } | null)?.draft;
  // Drafts from before Bcc and attachments existed have neither. The assistant's files
  // come by reference (`source`): they're fetched when the draft is sent, and the user
  // can take any of them out first.
  if (!d || !Array.isArray(d.to)) return null;
  const attachments = (d.attachments ?? []).map((a) => ({ ...a, source: a.source ?? null }));
  return { ...d, bcc: d.bcc ?? [], attachments };
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
function forgetSent(id: string) {
  try {
    const list = JSON.parse(localStorage.getItem(SENT_KEY) ?? "[]") as string[];
    localStorage.setItem(SENT_KEY, JSON.stringify(list.filter((x) => x !== id)));
  } catch {
    // Only a convenience.
  }
}

/** A draft email in the chat: editable, and sent only when the user presses Send. */
export function DraftCard({ action }: { action: Action }) {
  const initial = draftOf(action);
  const [draft, setDraft] = useState<MailDraft | null>(initial);
  // The user's signature goes under what the assistant wrote, once it's known: added
  // here, never written by the model.
  const { ready, sign } = useSignature();
  const [signed, setSigned] = useState(false);
  if (ready && !signed) {
    setSigned(true);
    if (draft) setDraft(sign(draft));
  }
  const [sent, setSent] = useState(() => sentBefore(action.id));
  // When it was scheduled for, if it was sent with Send later.
  const [later, setLater] = useState<number | null>(null);
  const origin = `card:${action.id}`;
  const { sending, send, sendLater } = useSendDraft(origin, () => {
    rememberSent(action.id);
    setSent(true);
  });
  // Undo (or Edit in Scheduled) brings it back here while the card is on screen.
  useDraftHome(origin, (d) => {
    forgetSent(action.id);
    setDraft(d);
    setLater(null);
    setSent(false);
  });
  if (!draft) return null;
  if (sent) {
    return (
      <div className="flex items-center gap-3 rounded-[14px] bg-background px-3.5 py-2.5 type-callout shadow-[var(--shadow-card)]">
        <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-private-soft text-private">
          <Check className="size-3" strokeWidth={3} />
        </span>
        <span className="min-w-0 flex-1 truncate">
          {later === null ? "Sent" : `Scheduled for ${when(later)}:`} “{draft.subject || "(no subject)"}” to{" "}
          {draft.to.join(", ")}
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
        <SendButton
          sending={sending}
          onClick={() => send(draft)}
          onSchedule={(at) => {
            setLater(at);
            sendLater(draft, at);
          }}
          disabled={draft.to.length === 0}
        />
      </div>
    </motion.div>
  );
}
