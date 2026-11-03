import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { motion } from "motion/react";
import { Popover as PopoverPrimitive } from "radix-ui";
import { cn } from "cn";

import type { EmailSuggestion } from "@/bindings/EmailSuggestion";
import { PersonAvatar } from "@/components/people";
import { api, keys } from "@/lib/api";

/** Someone an email or an invitation goes to: a name when known, and their address. */
export type AddressEntry = { name: string | null; email: string };

/** What the daemon reads: "Sam Carter <sam@example.com>" or a bare address. */
export function addressText(a: AddressEntry) {
  const name = a.name?.replace(/[<>",;]/g, "").replace(/\s+/g, " ").trim();
  return name ? `${name} <${a.email}>` : a.email;
}

/** The other way: "Sam Carter <sam@example.com>", `"Carter, Sam" <…>` or a bare address. */
export function parseAddress(raw: string): AddressEntry {
  const m = raw.match(/^\s*"?([^"<]*?)"?\s*<([^<>]+)>\s*$/);
  return m ? { name: m[1].trim() || null, email: m[2].trim() } : { name: null, email: raw.trim() };
}

/** A list ("sam@example.com, \"Carter, Sam\" <sam@example.net>") split where it should be. */
export function parseAddresses(raw: string): AddressEntry[] {
  return (raw.match(/(?:"[^"]*"|<[^>]*>|[^,;\n])+/g) ?? []).map(parseAddress).filter((a) => a.email);
}

const EMAIL = /^[^\s@<>",;:]+@[^\s@<>",;:]+\.[^\s@<>",;:]+$/;

/**
 * Addresses as removable chips, and a field that suggests people from People by name
 * or address (each of their addresses) or takes an address typed out. "@Sam" works
 * like "Sam", as in chat. Used for an email's To and Cc and an event's Guests.
 */
export function AddressField({
  value,
  onChange,
  label,
  placeholder,
  typedAction = "Add",
  variant = "boxed",
  autoFocus,
  disabled = false,
}: {
  value: AddressEntry[];
  onChange: (addresses: AddressEntry[]) => void;
  /** Names the field for screen readers: "To", "Guests"… */
  label: string;
  placeholder?: string;
  /** The verb for a typed-out address in the list: "Add sam@example.com". */
  typedAction?: string;
  /** `boxed` is a form field; `plain` sits in a line of a mail compose sheet. */
  variant?: "boxed" | "plain";
  autoFocus?: boolean;
  disabled?: boolean;
}) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const query = text.trim().replace(/^@/, "");
  const suggestions = useQuery({
    queryKey: keys.emailSuggestions(query),
    queryFn: () => api.emailSuggestions(query),
    enabled: open && query.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
  const taken = new Set(value.map((a) => a.email.toLowerCase()));
  const options: EmailSuggestion[] = query
    ? (suggestions.data ?? []).filter((s) => !taken.has(s.email.toLowerCase()))
    : [];
  const typed =
    EMAIL.test(query) && !taken.has(query.toLowerCase()) && !options.some((o) => o.email === query.toLowerCase())
      ? query
      : null;
  const count = options.length + (typed ? 1 : 0);
  useEffect(() => setHighlight(0), [query]);

  const add = (more: AddressEntry[]) => {
    const fresh = more
      .map((a) => ({ ...a, email: a.email.toLowerCase() }))
      .filter((a, i, all) => !taken.has(a.email) && all.findIndex((b) => b.email === a.email) === i);
    if (fresh.length > 0) onChange([...value, ...fresh]);
  };
  const pick = (i: number) => {
    if (i < options.length) add([{ name: options[i].name, email: options[i].email }]);
    else if (typed) add([{ name: null, email: typed }]);
    setText("");
    input.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && count > 0) {
      e.preventDefault();
      setOpen(true);
      setHighlight((h) => (h + 1) % count);
    } else if (e.key === "ArrowUp" && count > 0) {
      e.preventDefault();
      setHighlight((h) => (h - 1 + count) % count);
    } else if ((e.key === "Enter" || e.key === "Tab" || e.key === "," || e.key === ";") && query) {
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

  // A pasted list ("sam@example.com, Bo <bo@example.net>") becomes chips; anything that
  // isn't an address stays in the field to fix.
  const onPaste = (e: React.ClipboardEvent<HTMLInputElement>) => {
    const pasted = e.clipboardData.getData("text");
    if (!/[,;\n]/.test(pasted)) return;
    e.preventDefault();
    const parts = parseAddresses(pasted);
    add(parts.filter((a) => EMAIL.test(a.email)));
    setText(
      parts
        .filter((a) => !EMAIL.test(a.email))
        .map(addressText)
        .join(", "),
    );
  };

  return (
    <PopoverPrimitive.Root open={open && count > 0}>
      <PopoverPrimitive.Anchor asChild>
        <div
          className={cn(
            "flex min-w-0 flex-1 flex-wrap items-center gap-1.5",
            variant === "boxed" &&
              "min-h-9 rounded-[10px] bg-background px-2 py-1.5 shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_2px_rgb(0_0_0/0.05)] focus-within:ring-[3px] focus-within:ring-ring/45",
            variant === "plain" && "py-2",
            disabled && "opacity-60",
          )}
          onClick={() => input.current?.focus()}
        >
          {value.map((a) => {
            const valid = EMAIL.test(a.email);
            return (
              <span
                key={a.email}
                title={valid ? a.email : `“${a.email}” isn't an email address.`}
                className={cn(
                  "inline-flex h-6 max-w-full items-center gap-1 rounded-full pr-1 pl-2.5 type-subhead",
                  valid ? "bg-fill" : "bg-destructive/10 text-destructive",
                )}
              >
                <span className="truncate">{a.name ?? a.email}</span>
                {!disabled && (
                  <button
                    type="button"
                    aria-label={`Remove ${a.name ?? a.email}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      onChange(value.filter((v) => v.email !== a.email));
                    }}
                    className="flex size-4 items-center justify-center rounded-full text-faint hover:bg-[rgb(118_118_128/0.18)] hover:text-foreground"
                  >
                    <X className="size-3" />
                  </button>
                )}
              </span>
            );
          })}
          <input
            ref={input}
            value={text}
            disabled={disabled}
            autoFocus={autoFocus}
            onChange={(e) => {
              setText(e.target.value.replace(/[,;]/g, ""));
              setOpen(true);
            }}
            onFocus={() => setOpen(true)}
            onBlur={() => {
              setOpen(false);
              // An address typed out and left counts, so it isn't lost on Send or Save.
              if (typed) {
                add([{ name: null, email: typed }]);
                setText("");
              }
            }}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            placeholder={value.length === 0 ? placeholder : ""}
            aria-label={label}
            role="combobox"
            aria-expanded={open && count > 0}
            aria-autocomplete="list"
            className="h-6 min-w-[140px] flex-1 bg-transparent px-1 type-callout outline-none placeholder:text-faint"
          />
        </div>
      </PopoverPrimitive.Anchor>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          side="bottom"
          align="start"
          sideOffset={6}
          onOpenAutoFocus={(e) => e.preventDefault()}
          onCloseAutoFocus={(e) => e.preventDefault()}
          className="z-50 w-(--radix-popover-trigger-width) max-w-[420px] min-w-[260px]"
        >
          <motion.div
            role="listbox"
            aria-label="People"
            initial={{ opacity: 0, scale: 0.97, y: -4 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={{ type: "spring", stiffness: 520, damping: 36 }}
            className="max-h-[240px] origin-top overflow-y-auto rounded-[14px] bg-background p-1.5 shadow-[var(--shadow-float)]"
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
            {typed && (
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
                {typedAction} <span className="font-medium">{typed}</span>
              </div>
            )}
          </motion.div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
