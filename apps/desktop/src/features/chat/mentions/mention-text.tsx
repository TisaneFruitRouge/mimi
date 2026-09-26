import { forwardRef } from "react";
import { CalendarDays } from "lucide-react";
import { cn } from "cn";

import type { Mention } from "@/bindings/Mention";
import { segments } from "@/features/chat/mentions/ranges";

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
            <span
              key={i}
              className={cn(
                "rounded-[5px]",
                s.mention.kind === "person"
                  ? "bg-lime-soft shadow-[0_0_0_2px_var(--lime-soft)]"
                  : "bg-event-soft shadow-[0_0_0_2px_var(--event-soft)]",
              )}
            >
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
            title={s.mention.kind === "event" ? "An event from your calendar" : "Someone from your contacts"}
            className={cn(
              "inline-flex items-center gap-1 rounded-md px-1.5 py-px align-baseline font-medium",
              s.mention.kind === "person" ? "bg-lime-soft text-lime-deep" : "bg-event-soft text-event",
            )}
          >
            {s.mention.kind === "event" && <CalendarDays className="size-3.5 self-center" />}
            {s.mention.label}
          </span>
        ),
      )}
    </>
  );
}
