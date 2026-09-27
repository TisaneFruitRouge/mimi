import { useState } from "react";
import { Loader2, Send } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";

/** "sam@example.com, Bo <bo@example.net>" ⇄ a list of addresses. */
export const joinAddresses = (list: string[]) => list.join(", ");
export const splitAddresses = (raw: string) =>
  raw
    .split(/[,;\n]/)
    .map((s) => s.trim())
    .filter(Boolean);

/**
 * A message being written, laid out like a mail app's compose sheet: To, Cc and Subject
 * lines over the text. Everything stays editable until it's sent.
 */
export function DraftEditor({
  draft,
  onChange,
  autoFocus,
  className,
}: {
  draft: MailDraft;
  onChange: (d: MailDraft) => void;
  autoFocus?: "to" | "body";
  className?: string;
}) {
  // The address fields are edited as text and parsed on the way out.
  const [to, setTo] = useState(() => joinAddresses(draft.to));
  const [cc, setCc] = useState(() => joinAddresses(draft.cc));
  const [showCc, setShowCc] = useState(draft.cc.length > 0);
  return (
    <div className={cn("flex flex-col", className)}>
      <Line label="To">
        <input
          value={to}
          onChange={(e) => {
            setTo(e.target.value);
            onChange({ ...draft, to: splitAddresses(e.target.value) });
          }}
          aria-label="To"
          autoFocus={autoFocus === "to"}
          className="min-w-0 flex-1 bg-transparent type-callout outline-none"
        />
        {!showCc && (
          <button type="button" onClick={() => setShowCc(true)} className="type-subhead text-faint hover:text-foreground">
            Cc
          </button>
        )}
      </Line>
      {showCc && (
        <Line label="Cc">
          <input
            value={cc}
            onChange={(e) => {
              setCc(e.target.value);
              onChange({ ...draft, cc: splitAddresses(e.target.value) });
            }}
            aria-label="Cc"
            className="min-w-0 flex-1 bg-transparent type-callout outline-none"
          />
        </Line>
      )}
      <Line label="Subject">
        <input
          value={draft.subject}
          onChange={(e) => onChange({ ...draft, subject: e.target.value })}
          aria-label="Subject"
          className="min-w-0 flex-1 bg-transparent type-callout font-medium outline-none"
        />
      </Line>
      <textarea
        value={draft.body}
        onChange={(e) => onChange({ ...draft, body: e.target.value })}
        aria-label="Message"
        autoFocus={autoFocus === "body"}
        placeholder="Write your message"
        className="field-sizing-content min-h-40 w-full resize-none bg-transparent px-4 py-3 type-body leading-[1.5] outline-none placeholder:text-[#a1a1a6]"
      />
    </div>
  );
}

function Line({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex h-10 items-center gap-2 px-4 shadow-[inset_0_-0.5px_0_var(--separator)]">
      <span className="w-14 shrink-0 type-subhead text-faint">{label}</span>
      {children}
    </label>
  );
}

/** Sends a draft the user has read: their click is the approval. */
export function useSendDraft(onSent?: () => void) {
  const [sending, setSending] = useState(false);
  const send = async (draft: MailDraft) => {
    if (draft.to.length === 0) {
      toast.error("Add at least one recipient.");
      return;
    }
    setSending(true);
    try {
      await api.sendMail(draft);
      toast.success(draft.to.length === 1 ? `Sent to ${draft.to[0]}` : "Sent");
      onSent?.();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setSending(false);
    }
  };
  return { sending, send };
}

export function SendButton({ sending, onClick, disabled }: { sending: boolean; onClick: () => void; disabled?: boolean }) {
  return (
    <Button variant="lime" onClick={onClick} disabled={sending || disabled} className="min-w-[92px]">
      {sending ? <Loader2 className="animate-spin" /> : <Send />}
      Send
    </Button>
  );
}
