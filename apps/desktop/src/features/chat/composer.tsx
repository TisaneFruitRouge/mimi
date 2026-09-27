import { forwardRef, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, AtSign, Blocks, MessageSquarePlus, Plus, Sparkles } from "lucide-react";
import { cn } from "cn";

import type { Mention } from "@/bindings/Mention";
import { LocalityIcon } from "@/components/locality-badge";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { MentionPicker } from "@/features/chat/mentions/mention-picker";
import { MentionHighlights } from "@/features/chat/mentions/mention-text";
import { useMentions } from "@/features/chat/mentions/use-mentions";
import type { Section } from "@/features/shell/top-bar";
import { localityExplanation } from "@/lib/format";
import { mod } from "@/lib/platform";
import { useActiveModel } from "@/lib/queries";

export interface ComposerHandle {
  setText: (text: string) => void;
  /** Text with its @ mentions already made into pills, e.g. from "Ask Mimi about this". */
  setDraft: (text: string, mentions: Mention[]) => void;
}

/**
 * The message box: a floating card with the text field on top and a quiet toolbar
 * below. Entry points on the left (`@` mentions, `/` actions), privacy and send on the
 * right.
 */
export const Composer = forwardRef<
  ComposerHandle,
  {
    replying: boolean;
    onSend: (text: string, mentions: Mention[]) => Promise<boolean>;
    onStop: () => void;
    onNewConversation: () => void;
    onSection: (s: Section) => void;
  }
>(function Composer({ replying, onSend, onStop, onNewConversation, onSection }, ref) {
  const [text, setText] = useState("");
  const [actionsOpen, setActionsOpen] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const highlights = useRef<HTMLDivElement>(null);
  const active = useActiveModel();
  const mention = useMentions({ text, setText, area });

  useImperativeHandle(ref, () => {
    const fill = (t: string, mentions: Mention[]) => {
      mention.reset();
      mention.restore(mentions);
      setText(t);
      requestAnimationFrame(() => {
        area.current?.focus();
        area.current?.setSelectionRange(t.length, t.length);
      });
    };
    return { setText: (t) => fill(t, []), setDraft: fill };
  });

  // Grow with the content, up to a limit.
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
  }, [text]);

  const submit = async () => {
    const value = text.trim();
    if (!value || replying || !active) return;
    const mentions = mention.mentions;
    setText("");
    mention.reset();
    if (!(await onSend(value, mentions))) {
      setText(value);
      mention.restore(mentions);
    }
  };

  const action = (fn: () => void) => () => {
    setActionsOpen(false);
    fn();
  };

  const canSend = !!text.trim() && !!active;

  return (
    <div className="mx-auto w-full max-w-[720px] px-6 pb-5">
      <div className="relative rounded-[24px] bg-background shadow-[var(--shadow-raised)] transition-shadow duration-300 focus-within:shadow-[0_0_0_0.5px_rgb(86_118_13/0.28),0_0_0_3px_rgb(200_242_93/0.22),0_8px_24px_-6px_rgb(0_0_0/0.12)]">
        {mention.open && (
          <MentionPicker
            query={mention.query}
            candidates={mention.candidates}
            loading={mention.loading}
            highlight={mention.highlight}
            onHighlight={mention.setHighlight}
            onPick={mention.pick}
          />
        )}
        <div className="relative">
          <MentionHighlights ref={highlights} text={text} mentions={mention.mentions} className={fieldText} />
          <textarea
            ref={area}
            value={text}
            onChange={(e) => {
              // A lone "/" opens the actions menu, like a command line.
              if (e.target.value === "/" && text === "") {
                setActionsOpen(true);
                return;
              }
              setText(e.target.value);
              mention.refresh(e.target.value);
            }}
            onKeyDown={(e) => {
              if (mention.onKeyDown(e)) return;
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit();
              }
            }}
            onSelect={() => mention.refresh()}
            onBlur={() => mention.close()}
            onScroll={(e) => {
              if (highlights.current) highlights.current.scrollTop = e.currentTarget.scrollTop;
            }}
            placeholder={active ? "Ask anything" : "Choose a model to start"}
            aria-label="Message"
            rows={1}
            autoFocus
            className={cn(
              fieldText,
              "relative block max-h-60 min-h-[52px] w-full resize-none bg-transparent outline-none placeholder:text-[#a1a1a6]",
            )}
          />
        </div>
        <div className="flex items-center gap-1 px-3 pt-1 pb-3">
          <Tooltip>
            <TooltipTrigger asChild>
              <ToolbarChip icon={<AtSign />} label="Mention a person or event" onClick={mention.start} />
            </TooltipTrigger>
            <TooltipContent>Mention a person or event · or type @</TooltipContent>
          </Tooltip>

          <Popover open={actionsOpen} onOpenChange={setActionsOpen}>
            <Tooltip>
              <TooltipTrigger asChild>
                <PopoverTrigger asChild>
                  <ToolbarChip icon={<Plus />} label="More" />
                </PopoverTrigger>
              </TooltipTrigger>
              <TooltipContent>More · or type /</TooltipContent>
            </Tooltip>
            <PopoverContent align="start" className="w-[240px] p-1.5">
              <MenuItem icon={<MessageSquarePlus />} hint={`${mod}N`} onClick={action(onNewConversation)}>
                New chat
              </MenuItem>
              <MenuItem icon={<Sparkles />} onClick={action(() => onSection("models"))}>
                Change model
              </MenuItem>
              <MenuItem icon={<Blocks />} onClick={action(() => onSection("connections"))}>
                Connections
              </MenuItem>
            </PopoverContent>
          </Popover>

          <div className="flex-1" />
          <PrivacyChip onManage={() => onSection("models")} />

          <AnimatePresence mode="popLayout" initial={false}>
            {replying ? (
              <motion.button
                key="stop"
                initial={{ scale: 0.6, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.6, opacity: 0 }}
                transition={{ type: "spring", stiffness: 600, damping: 30 }}
                onClick={onStop}
                aria-label="Stop replying"
                className="pressable ml-1 flex size-8 items-center justify-center rounded-full bg-foreground text-background hover:bg-foreground/85"
              >
                <span className="size-2.5 rounded-[2.5px] bg-current" />
              </motion.button>
            ) : (
              <motion.button
                key="send"
                initial={{ scale: 0.6, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.6, opacity: 0 }}
                transition={{ type: "spring", stiffness: 600, damping: 30 }}
                onClick={submit}
                disabled={!canSend}
                aria-label="Send"
                className={cn(
                  "pressable ml-1 flex size-8 items-center justify-center rounded-full",
                  canSend
                    ? "bg-lime text-lime-ink shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(86_118_13/0.3)] hover:brightness-[0.97]"
                    : "bg-fill text-[#a1a1a6]",
                )}
              >
                <ArrowUp className="size-[17px]" strokeWidth={2.6} />
              </motion.button>
            )}
          </AnimatePresence>
        </div>
      </div>
      {active?.provider.locality === "cloud" && (
        <p className="mt-2.5 text-center type-footnote text-cloud">
          Messages go to {active.provider.name}, a cloud service.
        </p>
      )}
    </div>
  );
});

/** Box and type shared by the textarea and the mention highlights behind it. */
const fieldText = "px-5 pt-4 pb-1 type-body";

/** A quiet round button in the composer toolbar; its label shows as a tooltip. */
const ToolbarChip = forwardRef<
  HTMLButtonElement,
  { icon: React.ReactNode; label: string } & React.ComponentProps<"button">
>(function ToolbarChip({ icon, label, className, ...props }, ref) {
  return (
    <button
      ref={ref}
      aria-label={label}
      {...props}
      className={cn(
        "pressable flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-fill hover:text-foreground data-[state=open]:bg-fill data-[state=open]:text-foreground [&_svg]:size-[17px]",
        className,
      )}
    >
      {icon}
    </button>
  );
});

function MenuItem({
  icon,
  hint,
  children,
  onClick,
}: {
  icon: React.ReactNode;
  hint?: string;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-[7px] text-left text-[14px] hover:bg-fill [&_svg]:size-4 [&_svg]:text-muted-foreground"
    >
      {icon}
      <span className="flex-1">{children}</span>
      {hint && <span className="type-footnote text-faint">{hint}</span>}
    </button>
  );
}

/** Says, in one word, whether what you write stays private; explains on click. */
function PrivacyChip({ onManage }: { onManage: () => void }) {
  const active = useActiveModel();
  const [open, setOpen] = useState(false);
  if (!active) return null;
  const locality = active.provider.locality;
  const tone = {
    device: "text-private hover:bg-private-soft data-[state=open]:bg-private-soft",
    network: "text-network hover:bg-network-soft data-[state=open]:bg-network-soft",
    cloud: "bg-cloud-soft text-cloud hover:brightness-[0.98] data-[state=open]:brightness-[0.98]",
  }[locality];

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "pressable flex h-8 items-center gap-1.5 rounded-full px-3 text-[13px] font-medium",
            tone,
          )}
        >
          <LocalityIcon locality={locality} className="size-3.5" />
          {locality === "cloud" ? "Cloud" : "Private"}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[280px] p-4">
        <p className="type-callout font-medium">
          {locality === "cloud" ? `Using ${active.provider.name}` : "Your conversations stay private"}
        </p>
        <p className="mt-1 type-subhead text-muted-foreground">{localityExplanation[locality]}</p>
        <Button
          size="sm"
          variant="secondary"
          className="mt-3"
          onClick={() => {
            setOpen(false);
            onManage();
          }}
        >
          <Sparkles /> Choose a model
        </Button>
      </PopoverContent>
    </Popover>
  );
}
