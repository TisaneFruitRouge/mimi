import { useLayoutEffect, useRef, useState } from "react";
import { ArrowUp, Square } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

export function Composer({
  replying,
  disabled,
  note,
  onSend,
  onStop,
}: {
  replying: boolean;
  disabled: boolean;
  note: React.ReactNode;
  onSend: (text: string) => Promise<boolean>;
  onStop: () => void;
}) {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);

  // Grow with the content, up to a limit.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  }, [text]);

  const submit = async () => {
    const value = text.trim();
    if (!value || replying || disabled) return;
    setText("");
    if (!(await onSend(value))) setText(value);
  };

  return (
    <div className="mx-auto w-full max-w-3xl px-4 pb-4">
      <div className="rounded-2xl border bg-background shadow-sm transition-shadow focus-within:shadow-md">
        <Textarea
          ref={ref}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder="Message your assistant…"
          rows={1}
          autoFocus
          className="min-h-12 resize-none border-0 bg-transparent px-4 pt-3.5 shadow-none focus-visible:ring-0 dark:bg-transparent"
        />
        <div className="flex items-center justify-between gap-2 px-3 pb-2.5">
          <div className="min-w-0 truncate text-xs text-muted-foreground">{note}</div>
          {replying ? (
            <Button size="icon-sm" variant="secondary" onClick={onStop} aria-label="Stop replying">
              <Square className="fill-current" />
            </Button>
          ) : (
            <Button
              size="icon-sm"
              onClick={submit}
              disabled={!text.trim() || disabled}
              aria-label="Send"
            >
              <ArrowUp />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
