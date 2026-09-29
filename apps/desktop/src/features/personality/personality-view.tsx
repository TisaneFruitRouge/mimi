import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Check } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";

import type { Settings } from "@/bindings/Settings";
import { AssistantAvatar } from "@/components/assistant-avatar";
import { Grouped, Page, PageHeader, Section } from "@/components/page";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api, keys } from "@/lib/api";
import { useAssistantName, useSettings } from "@/lib/queries";

import { INSTRUCTIONS_LIMIT, PERSONALITY_LIMIT, personalityPresets } from "./presets";

type Field = "assistant_name" | "personality" | "custom_instructions";
type SaveState = "idle" | "saving" | "saved" | "error";

/**
 * Settings › Personality: the assistant's name, who it is and how it talks, and the
 * user's standing instructions. The daemon puts the last two into every conversation
 * (chat, messaging apps, routines) and the instructions into email drafts; see
 * `crates/core/src/persona.rs`.
 */
export function PersonalityView() {
  const name = useAssistantName();
  const [attentive, setAttentive] = useState(false);
  const assistantName = useSavedText("assistant_name", { debounce: false, required: true });
  const personality = useSavedText("personality");
  const instructions = useSavedText("custom_instructions");
  const preset = personalityPresets.find((p) => p.text === personality.value.trim());

  const choose = (text: string) => {
    const before = personality.value;
    if (before.trim() === text) return;
    personality.set(text, { now: true });
    // Replacing something the user wrote themselves can be undone.
    if (before.trim() && !personalityPresets.some((p) => p.text === before.trim())) {
      toast("Personality replaced", {
        action: { label: "Undo", onClick: () => personality.set(before, { now: true }) },
      });
    }
  };

  return (
    <Page>
      <PageHeader
        title="Personality"
        subtitle={`Who ${name} is, how it talks to you, and what it should always keep in mind.`}
        action={<AssistantAvatar size={64} mood={attentive ? "listening" : "idle"} decorative />}
      />

      {/* The avatar leans in while the user writes to it. */}
      <div onFocus={() => setAttentive(true)} onBlur={() => setAttentive(false)} className="contents">

      <Section title="Name" action={<SaveNote state={assistantName.state} error={assistantName.error} />}>
        <Grouped>
          <form
            className="flex items-center gap-3 px-4 py-3"
            onSubmit={(e) => {
              e.preventDefault();
              assistantName.flush();
            }}
          >
            <Input
              id="assistant-name"
              aria-label="Your assistant's name"
              value={assistantName.value}
              maxLength={40}
              onChange={(e) => assistantName.set(e.target.value)}
              onBlur={assistantName.flush}
              className="h-9 max-w-sm"
            />
          </form>
        </Grouped>
      </Section>

      <Section title="Personality" action={<SaveNote state={personality.state} error={personality.error} />}>
        <div role="radiogroup" aria-label="Starting points" className="flex flex-wrap gap-2">
          {personalityPresets.map((p) => {
            const selected = preset?.id === p.id;
            return (
              <button
                key={p.id}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => choose(p.text)}
                className={cn(
                  "inline-flex h-8 items-center gap-1.5 rounded-full px-3.5 type-subhead font-medium transition-colors",
                  selected
                    ? "bg-fill text-foreground"
                    : "bg-background text-foreground/80 shadow-[0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(0_0_0/0.04)] hover:bg-[rgb(118_118_128/0.06)]",
                )}
              >
                {selected && <Check className="size-3.5" strokeWidth={2.5} />}
                {p.label}
              </button>
            );
          })}
        </div>
        <TextBox
          id="personality"
          label="Personality"
          value={personality.value}
          limit={PERSONALITY_LIMIT}
          onChange={(v) => personality.set(v)}
          onBlur={personality.flush}
          placeholder={`Helpful, direct and warm: that's how ${name} talks now. Pick a starting point above, or describe the personality you'd like in your own words.`}
        />
      </Section>

      <Section
        title="Custom instructions"
        action={<SaveNote state={instructions.state} error={instructions.error} />}
      >
        <TextBox
          id="instructions"
          label="Custom instructions"
          value={instructions.value}
          limit={INSTRUCTIONS_LIMIT}
          onChange={(v) => instructions.set(v)}
          onBlur={instructions.flush}
          placeholder={
            "Answer in French unless I write in English.\n" +
            "Sign my emails with just my first name.\n" +
            "I'm vegetarian, so keep that in mind for recipes and restaurants."
          }
          rows={5}
        />
        <p className="max-w-[640px] type-footnote text-muted-foreground">
          {name} follows these in every conversation, in your messaging apps and in routines, and when it
          drafts an email for you. They can't turn off anything in Permissions. When you use a
          model in the cloud, they're sent along with your messages, so leave out passwords and
          other secrets.
        </p>
      </Section>
      </div>
    </Page>
  );
}

/** A text box with a quiet character count that shows as the limit gets close. */
function TextBox({
  id,
  label,
  value,
  limit,
  onChange,
  onBlur,
  placeholder,
  rows = 4,
}: {
  id: string;
  label: string;
  value: string;
  limit: number;
  onChange: (v: string) => void;
  onBlur: () => void;
  placeholder: string;
  rows?: number;
}) {
  const count = value.length;
  const near = count >= limit * 0.8;
  return (
    <div className="relative">
      <Textarea
        id={id}
        aria-label={label}
        value={value}
        rows={rows}
        maxLength={limit}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onBlur}
        placeholder={placeholder}
        className="min-h-[112px] resize-none rounded-[14px] px-4 pt-3 pb-7 leading-[1.45]"
      />
      {near && (
        <span
          className={cn(
            "pointer-events-none absolute right-3.5 bottom-2 type-footnote tabular-nums",
            count >= limit ? "text-destructive" : "text-faint",
          )}
        >
          {count} / {limit}
        </span>
      )}
    </div>
  );
}

/** "Saving…", then a quiet "Saved" that fades after a moment. */
function SaveNote({ state, error }: { state: SaveState; error: string | null }) {
  const [shown, setShown] = useState(state);
  useEffect(() => {
    setShown(state);
    if (state !== "saved") return;
    const t = window.setTimeout(() => setShown("idle"), 2000);
    return () => window.clearTimeout(t);
  }, [state]);
  return (
    <AnimatePresence>
      {shown !== "idle" && (
        <motion.span
          key={shown}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          className={cn("type-footnote", shown === "error" ? "text-destructive" : "text-muted-foreground")}
        >
          {shown === "saving" ? "Saving…" : shown === "saved" ? "Saved" : (error ?? "Couldn't save")}
        </motion.span>
      )}
    </AnimatePresence>
  );
}

/**
 * One text setting being edited: what's typed stays local and is saved shortly after
 * typing stops (or right away on blur, a preset or leaving the page).
 */
function useSavedText(field: Field, opts: { debounce?: boolean; required?: boolean } = {}) {
  const { debounce = true, required = false } = opts;
  const qc = useQueryClient();
  const settings = useSettings().data;
  const [draft, setDraft] = useState<string | null>(null);
  const [state, setState] = useState<SaveState>("idle");
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<string | null>(null);
  const value = draft ?? settings?.[field] ?? "";

  const save = useCallback(
    async (text: string): Promise<boolean> => {
      window.clearTimeout(timer.current);
      pending.current = null;
      // The latest settings, so saving one field never undoes another.
      const current = qc.getQueryData<Settings>(keys.settings);
      if (!current) return false;
      if (current[field] === text.trim()) return true;
      if (required && !text.trim()) return true;
      setState("saving");
      try {
        const saved = await api.putSettings({ ...current, [field]: text });
        qc.setQueryData(keys.settings, saved);
        setState("saved");
        setError(null);
        return true;
      } catch (e) {
        setState("error");
        setError((e as Error).message);
        return false;
      }
    },
    [qc, field, required],
  );

  const set = (text: string, { now = false } = {}) => {
    setDraft(text);
    setState("idle");
    pending.current = text;
    window.clearTimeout(timer.current);
    if (now) void save(text);
    else if (debounce) timer.current = window.setTimeout(() => void save(text), 800);
  };

  const flush = () => {
    const text = pending.current ?? draft;
    if (text === null) return;
    // Once saved, show what was kept (trimmed), or the saved name if it was left empty.
    // A failed save keeps the text, so nothing typed is lost.
    void save(text).then((ok) => ok && setDraft((d) => (d === text ? null : d)));
  };

  // Leaving the page mid-sentence still saves it.
  useEffect(
    () => () => {
      if (pending.current !== null) void save(pending.current);
    },
    [save],
  );

  return { value, state, error, set, flush };
}
