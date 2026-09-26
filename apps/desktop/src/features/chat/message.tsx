import { useState } from "react";
import { ChevronRight, CircleAlert, Sparkles } from "lucide-react";
import { cn } from "cn";

import type { Message } from "@/bindings/Message";
import { LocalityBadge } from "@/components/locality-badge";
import { Markdown } from "@/components/markdown";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

export function MessageView({ message }: { message: Message }) {
  if (message.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[85%] rounded-2xl rounded-br-md bg-muted px-4 py-2.5 text-sm leading-relaxed whitespace-pre-wrap">
          {message.content}
        </div>
      </div>
    );
  }

  const streaming = message.status === "streaming";
  const waiting = streaming && !message.content && !message.reasoning;

  return (
    <div className="group flex flex-col gap-2">
      {message.reasoning && (
        <Reasoning text={message.reasoning} active={streaming && !message.content} />
      )}
      {waiting && <TypingDots />}
      {message.content && (
        <div className={cn(streaming && "streaming-caret")}>
          <Markdown>{message.content}</Markdown>
        </div>
      )}
      {message.status === "error" && (
        <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <span>{message.error ?? "Something went wrong while writing this reply."}</span>
        </div>
      )}
      {!streaming && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100">
          {message.model && <span>{message.model.model}</span>}
          {message.locality && <LocalityBadge locality={message.locality} compact />}
          {message.status === "cancelled" && <span>· Stopped</span>}
          {message.status === "interrupted" && <span>· Cut off when the assistant stopped</span>}
        </div>
      )}
    </div>
  );
}

function Reasoning({ text, active }: { text: string; active: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
        <Sparkles className={cn("size-3.5", active && "animate-pulse text-orange-500")} />
        {active ? "Thinking…" : "Thought process"}
        <ChevronRight className={cn("size-3.5 transition-transform", open && "rotate-90")} />
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="mt-2 border-l-2 pl-3 text-sm leading-relaxed whitespace-pre-wrap text-muted-foreground">
          {text}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function TypingDots() {
  return (
    <div className="flex h-6 items-center gap-1" aria-label="The assistant is replying">
      {[0, 150, 300].map((delay) => (
        <span
          key={delay}
          className="size-1.5 animate-bounce rounded-full bg-muted-foreground/60"
          style={{ animationDelay: `${delay}ms` }}
        />
      ))}
    </div>
  );
}
