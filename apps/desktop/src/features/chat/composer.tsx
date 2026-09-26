import { forwardRef, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { ArrowUp, AtSign, Blocks, Check, MessageSquarePlus, Slash, Sparkles, Square } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import { LocalityIcon } from "@/components/locality-badge";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import type { Section } from "@/features/shell/top-bar";
import { api } from "@/lib/api";
import { localityShort } from "@/lib/format";
import { sameModel, useActiveModel, useAllModels, useModelInfo, useSettings } from "@/lib/queries";

export interface ComposerHandle {
  setText: (text: string) => void;
}

export const Composer = forwardRef<
  ComposerHandle,
  {
    replying: boolean;
    onSend: (text: string) => Promise<boolean>;
    onStop: () => void;
    onNewConversation: () => void;
    onSection: (s: Section) => void;
  }
>(function Composer({ replying, onSend, onStop, onNewConversation, onSection }, ref) {
  const [text, setText] = useState("");
  const [actionsOpen, setActionsOpen] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const active = useActiveModel();

  useImperativeHandle(ref, () => ({
    setText: (t) => {
      setText(t);
      requestAnimationFrame(() => {
        area.current?.focus();
        area.current?.setSelectionRange(t.length, t.length);
      });
    },
  }));

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
    setText("");
    if (!(await onSend(value))) setText(value);
  };

  const action = (fn: () => void) => () => {
    setActionsOpen(false);
    fn();
  };

  return (
    <div className="mx-auto w-full max-w-[760px] px-5 pb-5">
      <div className="rounded-[20px] border bg-background shadow-[0_1px_2px_rgba(22,23,26,0.04),0_12px_32px_-8px_rgba(22,23,26,0.12)] transition-shadow focus-within:border-[#cfd8b5] focus-within:shadow-[0_1px_2px_rgba(22,23,26,0.04),0_16px_40px_-8px_rgba(22,23,26,0.16)]">
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
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={active ? "Ask anything, or type / for actions" : "Choose a model to start"}
          aria-label="Message"
          rows={1}
          autoFocus
          className="block max-h-60 min-h-[52px] w-full resize-none bg-transparent px-[18px] pt-4 pb-1 text-[15px] leading-relaxed outline-none placeholder:text-faint"
        />
        <div className="flex items-center gap-1.5 px-2.5 pt-1 pb-2.5">
          <Popover>
            <PopoverTrigger asChild>
              <ToolbarChip icon={<AtSign />} label="people, events, apps" />
            </PopoverTrigger>
            <PopoverContent align="start" className="w-72 p-4">
              <p className="text-sm font-medium">Mention people, events and apps</p>
              <p className="mt-1 text-[13px] leading-relaxed text-muted-foreground">
                Once your calendar, contacts or messaging apps are connected, type @ to point
                the assistant at exactly what you mean.
              </p>
              <Button size="sm" variant="outline" className="mt-3" onClick={() => onSection("connections")}>
                <Blocks /> Open Connections
              </Button>
            </PopoverContent>
          </Popover>

          <Popover open={actionsOpen} onOpenChange={setActionsOpen}>
            <PopoverTrigger asChild>
              <ToolbarChip icon={<Slash />} label="actions" mono />
            </PopoverTrigger>
            <PopoverContent align="start" className="w-60 p-1.5">
              <MenuItem icon={<MessageSquarePlus />} onClick={action(onNewConversation)}>
                New conversation
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
          <ModelChip onManage={() => onSection("models")} />

          {replying ? (
            <Button
              size="icon"
              onClick={onStop}
              aria-label="Stop replying"
              className="size-9 rounded-[11px]"
            >
              <Square className="size-3.5 fill-current" />
            </Button>
          ) : (
            <button
              onClick={submit}
              disabled={!text.trim() || !active}
              aria-label="Send"
              className="flex size-9 items-center justify-center rounded-[11px] bg-lime text-lime-ink shadow-[inset_0_0_0_1px_rgba(0,0,0,0.06)] transition hover:brightness-95 disabled:bg-subtle disabled:text-faint disabled:shadow-none"
            >
              <ArrowUp className="size-[18px]" strokeWidth={2.4} />
            </button>
          )}
        </div>
      </div>
      {active?.provider.locality === "cloud" && (
        <p className="mt-2 text-center text-xs text-cloud">
          Messages go to {active.provider.name}, a cloud service.
        </p>
      )}
    </div>
  );
});

const ToolbarChip = forwardRef<
  HTMLButtonElement,
  { icon: React.ReactNode; label: string; mono?: boolean } & React.ComponentProps<"button">
>(function ToolbarChip({ icon, label, mono, className, ...props }, ref) {
  return (
    <button
      ref={ref}
      {...props}
      className={cn(
        "flex h-8 items-center gap-1.5 rounded-[9px] bg-subtle px-2.5 text-[12.5px] text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground data-[state=open]:bg-secondary data-[state=open]:text-foreground [&_svg]:size-3.5",
        mono && "font-mono text-[12px]",
        className,
      )}
    >
      {icon}
      {label}
    </button>
  );
});

function MenuItem({
  icon,
  children,
  onClick,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-[13.5px] hover:bg-subtle [&_svg]:size-4 [&_svg]:text-muted-foreground"
    >
      {icon}
      {children}
    </button>
  );
}

/** Shows the active model and where it runs; switches models in place. */
function ModelChip({ onManage }: { onManage: () => void }) {
  const active = useActiveModel();
  const settings = useSettings().data;
  const { options } = useAllModels();
  const info = useModelInfo();
  const [open, setOpen] = useState(false);

  const choose = async (ref: (typeof options)[number]["ref"]) => {
    if (!settings) return;
    setOpen(false);
    try {
      await api.putSettings({ ...settings, default_model: ref });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  const tone = active
    ? {
        device: "border-[#bfe3d4] text-private",
        network: "border-[#c5d8f5] text-network",
        cloud: "border-[#f0d6ad] text-cloud bg-cloud-soft/60",
      }[active.provider.locality]
    : "text-faint";

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "flex h-8 items-center gap-1.5 rounded-[9px] border px-2.5 text-[12.5px] font-medium transition-colors hover:bg-subtle",
            tone,
          )}
        >
          {active && <LocalityIcon locality={active.provider.locality} className="size-3.5" />}
          {active ? `${active.name} · ${localityShort[active.provider.locality]}` : "No model"}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 p-1.5">
        <div className="max-h-80 overflow-y-auto">
          {options.map((o) => (
            <button
              key={`${o.ref.provider_id}/${o.ref.model}`}
              onClick={() => choose(o.ref)}
              className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left hover:bg-subtle"
            >
              <LocalityIcon
                locality={o.locality}
                className={cn(
                  "size-4 shrink-0",
                  { device: "text-private", network: "text-network", cloud: "text-cloud" }[o.locality],
                )}
              />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-[13.5px]">{info(o.ref.model).name}</span>
                <span className="truncate text-[11.5px] text-faint">{o.providerName}</span>
              </span>
              {sameModel(o.ref, active?.ref) && <Check className="size-4" />}
            </button>
          ))}
        </div>
        <div className="mt-1 border-t pt-1">
          <button
            onClick={() => {
              setOpen(false);
              onManage();
            }}
            className="w-full rounded-md px-2.5 py-2 text-left text-[13px] text-muted-foreground hover:bg-subtle hover:text-foreground"
          >
            Manage models…
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
