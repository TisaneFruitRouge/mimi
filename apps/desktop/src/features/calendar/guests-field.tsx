import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { motion } from "motion/react";
import { cn } from "cn";

import type { GuestSuggestion } from "@/bindings/GuestSuggestion";
import { PersonAvatar } from "@/components/people";
import { api, keys } from "@/lib/api";

/** Someone invited: a name when known, and the address the invitation goes to. */
export type GuestEntry = { name: string | null; email: string };

/** What the daemon reads: "Sam Carter <sam@example.com>" or a bare address. */
export function guestText(g: GuestEntry) {
  return g.name ? `${g.name.replace(/[<>",;]/g, "")} <${g.email}>` : g.email;
}

const EMAIL = /^[^\s@<>",;:]+@[^\s@<>",;:]+\.[^\s@<>",;:]+$/;

/**
 * Guests as removable chips, and a field that suggests people from People by name or
 * address (each of their addresses) or takes an address typed out. Nobody is emailed
 * from here: on saving, Google invites them on Google calendars, and elsewhere sending
 * is offered once the event is saved.
 */
export function GuestsField({
  value,
  onChange,
  disabled = false,
}: {
  value: GuestEntry[];
  onChange: (guests: GuestEntry[]) => void;
  disabled?: boolean;
}) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const query = text.trim();
  const suggestions = useQuery({
    queryKey: keys.guestSuggestions(query),
    queryFn: () => api.guestSuggestions(query),
    enabled: open && query.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
  const taken = new Set(value.map((g) => g.email.toLowerCase()));
  const options: GuestSuggestion[] = query
    ? (suggestions.data ?? []).filter((s) => !taken.has(s.email.toLowerCase()))
    : [];
  const typed = EMAIL.test(query) && !taken.has(query.toLowerCase()) ? query : null;
  const count = options.length + (typed && !options.some((o) => o.email === typed.toLowerCase()) ? 1 : 0);
  useEffect(() => setHighlight(0), [query]);

  const add = (g: GuestEntry) => {
    if (!taken.has(g.email.toLowerCase())) onChange([...value, { ...g, email: g.email.toLowerCase() }]);
    setText("");
    input.current?.focus();
  };
  const pick = (i: number) => {
    if (i < options.length) add({ name: options[i].name, email: options[i].email });
    else if (typed) add({ name: null, email: typed });
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && count > 0) {
      e.preventDefault();
      setOpen(true);
      setHighlight((h) => (h + 1) % count);
    } else if (e.key === "ArrowUp" && count > 0) {
      e.preventDefault();
      setHighlight((h) => (h - 1 + count) % count);
    } else if ((e.key === "Enter" || e.key === "Tab" || e.key === ",") && query) {
      if (count > 0) {
        e.preventDefault();
        pick(highlight);
      } else if (e.key !== "Tab") {
        e.preventDefault();
      }
    } else if (e.key === "Backspace" && !text && value.length > 0) {
      onChange(value.slice(0, -1));
    } else if (e.key === "Escape" && open && query) {
      e.stopPropagation();
      setText("");
    }
  };

  return (
    <div className="relative">
      <div
        className={cn(
          "flex min-h-9 flex-wrap items-center gap-1.5 rounded-[10px] bg-background px-2 py-1.5 shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_2px_rgb(0_0_0/0.05)] focus-within:ring-[3px] focus-within:ring-ring/45",
          disabled && "opacity-60",
        )}
        onClick={() => input.current?.focus()}
      >
        {value.map((g) => (
          <span
            key={g.email}
            title={g.email}
            className="inline-flex h-6 max-w-full items-center gap-1 rounded-full bg-fill pr-1 pl-2.5 type-subhead"
          >
            <span className="truncate">{g.name ?? g.email}</span>
            {!disabled && (
              <button
                type="button"
                aria-label={`Remove ${g.name ?? g.email}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onChange(value.filter((v) => v.email !== g.email));
                }}
                className="flex size-4 items-center justify-center rounded-full text-faint hover:bg-[rgb(118_118_128/0.18)] hover:text-foreground"
              >
                <X className="size-3" />
              </button>
            )}
          </span>
        ))}
        <input
          ref={input}
          value={text}
          disabled={disabled}
          onChange={(e) => {
            setText(e.target.value.replace(/,/g, ""));
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onKeyDown={onKeyDown}
          placeholder={value.length === 0 ? "Add people by name or email" : ""}
          aria-label="Guests"
          role="combobox"
          aria-expanded={open && count > 0}
          aria-autocomplete="list"
          className="h-6 min-w-[140px] flex-1 bg-transparent px-1 type-callout outline-none placeholder:text-faint"
        />
      </div>
      {open && count > 0 && (
        <motion.div
          role="listbox"
          aria-label="People"
          initial={{ opacity: 0, scale: 0.97, y: -4 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          transition={{ type: "spring", stiffness: 520, damping: 36 }}
          className="absolute top-full right-0 left-0 z-30 mt-1.5 max-h-[240px] origin-top overflow-y-auto rounded-[14px] bg-background p-1.5 shadow-[var(--shadow-float)]"
        >
          {options.map((s, i) => (
            <div
              key={`${s.person_id}-${s.email}`}
              role="option"
              aria-selected={i === highlight}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(i);
              }}
              onMouseEnter={() => setHighlight(i)}
              className={cn(
                "flex cursor-default items-center gap-2.5 rounded-[9px] px-2.5 py-1.5",
                i === highlight && "bg-fill",
              )}
            >
              <PersonAvatar id={s.person_id} name={s.name} size="sm" />
              <span className="min-w-0 flex-1">
                <span className="block truncate type-callout">{s.name}</span>
                <span className="block truncate type-footnote text-muted-foreground">{s.email}</span>
              </span>
            </div>
          ))}
          {typed && !options.some((o) => o.email === typed.toLowerCase()) && (
            <div
              role="option"
              aria-selected={highlight === options.length}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(options.length);
              }}
              onMouseEnter={() => setHighlight(options.length)}
              className={cn(
                "flex cursor-default items-center gap-2.5 rounded-[9px] px-2.5 py-2 type-callout",
                highlight === options.length && "bg-fill",
              )}
            >
              Invite <span className="font-medium">{typed}</span>
            </div>
          )}
        </motion.div>
      )}
    </div>
  );
}
