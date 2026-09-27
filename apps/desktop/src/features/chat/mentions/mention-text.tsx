import { forwardRef } from "react";
import { CalendarDays, Mail } from "lucide-react";
import { cn } from "cn";

import type { Mention } from "@/bindings/Mention";
import { segments } from "@/features/chat/mentions/ranges";

const isMail = (m: Mention) => m.kind === "mail_thread" || m.kind === "mail_message";

/** Pill colours per kind: lime for people, blue for events, violet for email. */
function tint(m: Mention) {
  if (m.kind === "person") return { soft: "bg-lime-soft shadow-[0_0_0_2px_var(--lime-soft)]", pill: "bg-lime-soft text-lime-deep" };
  if (isMail(m)) return { soft: "bg-mail-soft shadow-[0_0_0_2px_var(--mail-soft)]", pill: "bg-mail-soft text-mail" };
  return { soft: "bg-event-soft shadow-[0_0_0_2px_var(--event-soft)]", pill: "bg-event-soft text-event" };
}

const titles: Record<Mention["kind"], string> = {
  person: "Someone from your contacts",
  event: "An event from your calendar",
  mail_thread: "An email conversation",
  mail_message: "An email",
};

/**
 * Pills behind a textarea's text: same box, font and wrapping as the textarea, text
 * transparent, only the token backgrounds visible. The textarea stays the real input.
 */
export const MentionHighlights = forwardRef<HTMLDivElement, { text: string; mentions: Mention[]; className: string }>(
  function MentionHighlights({ text, mentions, className }, ref) {
    if (mentions.length === 0) return null;
    return (
      <div
        ref={ref}
        aria-hidden
        className={cn(
          className,
          "pointer-events-none absolute inset-0 overflow-hidden break-words whitespace-pre-wrap text-transparent",
        )}
      >
        {segments(text, mentions).map((s, i) =>
          s.kind === "text" ? (
            <span key={i}>{s.text}</span>
          ) : (
            <span key={i} className={cn("rounded-[5px]", tint(s.mention).soft)}>
              {s.text}
            </span>
          ),
        )}
        {/* Keeps a trailing newline's height, like the textarea. */}
        {"​"}
      </div>
    );
  },
);

/** A sent message's text with its mentions as pills. */
export function MentionText({ text, mentions }: { text: string; mentions: Mention[] }) {
  if (mentions.length === 0) return <>{text}</>;
  return (
    <>
      {segments(text, mentions).map((s, i) =>
        s.kind === "text" ? (
          <span key={i}>{s.text}</span>
        ) : (
          <span
            key={i}
            title={titles[s.mention.kind]}
            className={cn(
              "inline-flex items-center gap-1 rounded-md px-1.5 py-px align-baseline font-medium",
              tint(s.mention).pill,
            )}
          >
            {s.mention.kind === "event" && <CalendarDays className="size-3.5 self-center" />}
            {isMail(s.mention) && <Mail className="size-3.5 self-center" />}
            {s.mention.label}
          </span>
        ),
      )}
    </>
  );
}
