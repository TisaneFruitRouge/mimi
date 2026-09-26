import { useEffect, useRef } from "react";
import { CalendarDays, Loader2 } from "lucide-react";
import { motion } from "motion/react";
import { cn } from "cn";

import type { MentionCandidate } from "@/bindings/MentionCandidate";
import { ChannelIcons, PersonAvatar } from "@/components/people";

/**
 * The @ suggestions, floating above the composer. Focus stays in the textarea: keys are
 * handled by `useMentions`, the mouse picks with mousedown so the caret isn't lost.
 */
export function MentionPicker({
  query,
  candidates,
  loading,
  highlight,
  onHighlight,
  onPick,
}: {
  query: string;
  candidates: MentionCandidate[];
  loading: boolean;
  highlight: number;
  onHighlight: (i: number) => void;
  onPick: (c: MentionCandidate) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${highlight}"]`)?.scrollIntoView({ block: "nearest" });
  }, [highlight]);

  const people = candidates.filter((c) => c.kind === "person");
  const events = candidates.filter((c) => c.kind === "event");
  const indexOf = (c: MentionCandidate) => candidates.indexOf(c);

  return (
    <motion.div
      role="listbox"
      aria-label="People and events"
      initial={{ opacity: 0, scale: 0.97, y: 4 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 520, damping: 36 }}
      className="absolute bottom-full left-0 z-30 mb-2 w-[380px] max-w-full origin-bottom-left overflow-hidden rounded-[14px] bg-background shadow-[var(--shadow-float)]"
    >
      <div ref={list} className="max-h-[320px] overflow-y-auto p-1.5">
        {loading && (
          <div className="flex items-center gap-2 px-3 py-3 type-subhead text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" /> Looking…
          </div>
        )}
        {!loading && candidates.length === 0 && (
          <p className="px-3 py-3 type-subhead text-muted-foreground">
            {query
              ? `No one and nothing called “${query}”.`
              : "Add people or connect a calendar in Connections to mention them here."}
          </p>
        )}
        {people.length > 0 && <Heading>People</Heading>}
        {people.map((c) => (
          <Row key={`p-${c.id}`} c={c} index={indexOf(c)} active={indexOf(c) === highlight} onHighlight={onHighlight} onPick={onPick}>
            <PersonAvatar id={c.id} name={c.label} size="sm" />
            <span className="min-w-0 flex-1 truncate">
              {c.label}
              {c.detail && <span className="text-faint"> · {c.detail}</span>}
            </span>
            <ChannelIcons channels={c.channels} />
          </Row>
        ))}
        {events.length > 0 && <Heading>Events</Heading>}
        {events.map((c) => (
          <Row key={`e-${c.id}`} c={c} index={indexOf(c)} active={indexOf(c) === highlight} onHighlight={onHighlight} onPick={onPick}>
            <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-event-soft text-event">
              <CalendarDays className="size-3.5" />
            </span>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate">{c.label}</span>
              {c.detail && <span className="truncate type-footnote text-muted-foreground">{c.detail}</span>}
            </span>
          </Row>
        ))}
      </div>
      <div className="flex gap-3 bg-canvas px-3 py-1.5 type-footnote text-faint shadow-[inset_0_0.5px_0_var(--separator)]">
        <span>↑↓ to choose</span>
        <span>↵ to add</span>
        <span>esc to close</span>
      </div>
    </motion.div>
  );
}

function Heading({ children }: { children: React.ReactNode }) {
  return <div className="px-2.5 pt-2 pb-1 type-footnote font-semibold text-muted-foreground">{children}</div>;
}

function Row({
  c,
  index,
  active,
  onHighlight,
  onPick,
  children,
}: {
  c: MentionCandidate;
  index: number;
  active: boolean;
  onHighlight: (i: number) => void;
  onPick: (c: MentionCandidate) => void;
  children: React.ReactNode;
}) {
  return (
    <div
      role="option"
      aria-selected={active}
      data-index={index}
      onMouseEnter={() => onHighlight(index)}
      onMouseDown={(e) => {
        e.preventDefault();
        onPick(c);
      }}
      className={cn(
        "flex cursor-pointer items-center gap-2.5 rounded-[8px] px-2.5 py-2 type-callout",
        active && "bg-fill",
      )}
    >
      {children}
    </div>
  );
}
