import { useState } from "react";
import { Check, ChevronRight, CircleAlert, Copy } from "lucide-react";
import { cn } from "cn";

import type { Message } from "@/bindings/Message";
import { AssistantMark } from "@/components/brand";
import { LocalityIcon } from "@/components/locality-badge";
import { Markdown } from "@/components/markdown";
import { localityShort } from "@/lib/format";
import { useModelInfo } from "@/lib/queries";

export function MessageView({ message }: { message: Message }) {
  if (message.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] rounded-[18px] rounded-br-md bg-[#ecece8] px-4 py-2.5 text-[15px] leading-relaxed whitespace-pre-wrap">
          {message.content}
        </div>
      </div>
    );
  }
  return <AssistantMessage message={message} />;
}

function AssistantMessage({ message }: { message: Message }) {
  const info = useModelInfo();
  const streaming = message.status === "streaming";
  const thinking = streaming && !message.content;

  return (
    <div className="group flex gap-3.5">
      <AssistantMark />
      <div className="flex min-w-0 flex-1 flex-col gap-2 pt-0.5">
        {(message.reasoning || thinking) && (
          <Reasoning text={message.reasoning} active={thinking} />
        )}
        {message.content && (
          <div className={cn(streaming && "streaming-caret")}>
            <Markdown>{message.content}</Markdown>
          </div>
        )}
        {message.status === "error" && (
          <div className="flex items-start gap-2.5 rounded-xl border border-[#f1c9c1] bg-[#fdf2ef] px-3.5 py-2.5 text-[13.5px] text-destructive">
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            <span>{message.error ?? "Something went wrong while writing this reply."}</span>
          </div>
        )}
        {!streaming && (
          <div className="flex h-6 items-center gap-2 font-mono text-[11.5px] text-faint">
            {message.model && <span>{info(message.model.model).name}</span>}
            {message.locality && (
              <span className="inline-flex items-center gap-1">
                · <LocalityIcon locality={message.locality} className="size-3" />
                {localityShort[message.locality]}
              </span>
            )}
            {message.status === "cancelled" && <span>· stopped</span>}
            {message.status === "interrupted" && <span>· cut off</span>}
            {message.content && <CopyButton text={message.content} />}
          </div>
        )}
      </div>
    </div>
  );
}

function Reasoning({ text, active }: { text: string; active: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-col gap-2">
      <button
        onClick={() => text && setOpen(!open)}
        className="flex items-center gap-1.5 self-start font-mono text-[12px] text-faint hover:text-foreground"
      >
        <span className={cn(active && "shimmer")}>{active ? "thinking…" : "thought it through"}</span>
        {text && <ChevronRight className={cn("size-3.5 transition-transform", open && "rotate-90")} />}
      </button>
      {open && (
        <div className="max-h-72 overflow-y-auto rounded-xl bg-subtle px-3.5 py-3 text-[13px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
          {text}
        </div>
      )}
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() =>
        navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        })
      }
      aria-label="Copy reply"
      className="ml-1 rounded-md p-1 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-subtle hover:text-foreground focus-visible:opacity-100"
    >
      {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
    </button>
  );
}
