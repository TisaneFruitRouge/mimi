import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Loader2, Send } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { api, keys } from "@/lib/api";

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
      <FromLine draft={draft} onChange={onChange} />
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

/**
 * Which address it's sent from, when there's a choice: every connected account and the
 * aliases mail has arrived at. Replies start from the address the mail was sent to.
 */
function FromLine({ draft, onChange }: { draft: MailDraft; onChange: (d: MailDraft) => void }) {
  const overview = useQuery({ queryKey: keys.mailOverview(), queryFn: () => api.mailOverview() }).data;
  const options = (overview?.accounts ?? []).flatMap((a) =>
    a.addresses.map((x) => ({ connection: a.connection_id, email: x.email })),
  );
  if (options.length < 2) return null;
  const account = overview!.accounts.find((a) => a.connection_id === draft.connection_id) ?? overview!.accounts[0];
  const current = (draft.from ?? account.email).toLowerCase();
  return (
    <Line label="From">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button type="button" className="flex min-w-0 items-center gap-1 type-callout hover:text-foreground">
            <span className="truncate">{current}</span>
            <ChevronDown className="size-3.5 shrink-0 text-faint" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          {options.map((o) => (
            <DropdownMenuItem
              key={`${o.connection}:${o.email}`}
              onSelect={() => onChange({ ...draft, connection_id: o.connection, from: o.email })}
            >
              {o.email}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </Line>
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
