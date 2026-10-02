import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Loader2, Paperclip, Send } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import { AddressField, addressText, parseAddresses } from "@/components/address-field";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { DraftAttachments, readAttachments } from "@/features/mail/draft-attachments";
import { api, keys } from "@/lib/api";

/**
 * A message being written, laid out like a mail app's compose sheet: To, Cc, Bcc and
 * Subject lines over the text, and the attached files under it. Everything stays
 * editable until it's sent. The address lines suggest people from People as their name
 * or address is typed; files come from the paperclip, a paste, or a drop.
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
  const [showCc, setShowCc] = useState(draft.cc.length > 0);
  const [showBcc, setShowBcc] = useState(draft.bcc.length > 0);
  const [dropping, setDropping] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  // Files are read in the background: add them to the draft as it is by then.
  const latest = useRef(draft);
  latest.current = draft;
  const attach = async (files: File[]) => {
    if (files.length === 0) return;
    try {
      const read = await readAttachments(files, latest.current.attachments);
      onChange({ ...latest.current, attachments: [...latest.current.attachments, ...read] });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  const carriesFiles = (e: React.DragEvent) => e.dataTransfer.types.includes("Files");
  return (
    <div
      className={cn("flex flex-col transition-shadow", dropping && "shadow-[inset_0_0_0_2px_var(--color-lime)]", className)}
      onDragOver={(e) => {
        if (!carriesFiles(e)) return;
        // Files dropped here are for the email, not the chat around a draft card.
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = "copy";
        setDropping(true);
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropping(false);
      }}
      onDrop={(e) => {
        if (!carriesFiles(e)) return;
        // Taken here: the chat around a draft card sees it handled.
        e.preventDefault();
        setDropping(false);
        void attach(Array.from(e.dataTransfer.files));
      }}
    >
      <FromLine draft={draft} onChange={onChange} />
      <Line label="To" field>
        <AddressField
          variant="plain"
          label="To"
          value={draft.to.flatMap(parseAddresses)}
          onChange={(to) => onChange({ ...draft, to: to.map(addressText) })}
          autoFocus={autoFocus === "to"}
        />
        {!showCc && (
          <button type="button" onClick={() => setShowCc(true)} className="type-subhead text-faint hover:text-foreground">
            Cc
          </button>
        )}
        {!showBcc && (
          <button type="button" onClick={() => setShowBcc(true)} className="type-subhead text-faint hover:text-foreground">
            Bcc
          </button>
        )}
      </Line>
      {showCc && (
        <Line label="Cc" field>
          <AddressField
            variant="plain"
            label="Cc"
            value={draft.cc.flatMap(parseAddresses)}
            onChange={(cc) => onChange({ ...draft, cc: cc.map(addressText) })}
          />
        </Line>
      )}
      {showBcc && (
        <Line label="Bcc" field>
          <AddressField
            variant="plain"
            label="Bcc"
            value={draft.bcc.flatMap(parseAddresses)}
            onChange={(bcc) => onChange({ ...draft, bcc: bcc.map(addressText) })}
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
        <button
          type="button"
          onClick={() => picker.current?.click()}
          aria-label="Attach files"
          title="Attach files"
          className="flex size-7 items-center justify-center rounded-full text-faint hover:bg-fill hover:text-foreground"
        >
          <Paperclip className="size-4" />
        </button>
      </Line>
      <input
        ref={picker}
        type="file"
        multiple
        hidden
        onChange={(e) => {
          void attach(Array.from(e.target.files ?? []));
          e.target.value = "";
        }}
      />
      <textarea
        value={draft.body}
        onChange={(e) => onChange({ ...draft, body: e.target.value })}
        onPaste={(e) => {
          const files = Array.from(e.clipboardData.files);
          if (files.length === 0) return;
          // A picture alone is attached; text that comes with it still pastes.
          if (!e.clipboardData.types.includes("text/plain")) e.preventDefault();
          void attach(files);
        }}
        aria-label="Message"
        autoFocus={autoFocus === "body"}
        placeholder="Write your message"
        className="field-sizing-content min-h-40 w-full resize-none bg-transparent px-4 py-3 type-body leading-[1.5] outline-none placeholder:text-[#a1a1a6]"
      />
      <DraftAttachments
        attachments={draft.attachments}
        onRemove={(i) => onChange({ ...draft, attachments: draft.attachments.filter((_, j) => j !== i) })}
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

/** One line of the sheet. Address lines are no `<label>`: it would click their chips. */
function Line({ label, field, children }: { label: string; field?: boolean; children: React.ReactNode }) {
  const Tag = field ? "div" : "label";
  return (
    <Tag className="flex min-h-10 items-center gap-2 px-4 shadow-[inset_0_-0.5px_0_var(--separator)]">
      <span className="w-14 shrink-0 type-subhead text-faint">{label}</span>
      {children}
    </Tag>
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
