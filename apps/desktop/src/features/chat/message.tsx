import { useState } from "react";
import { Check, CircleAlert, Copy } from "lucide-react";
import { cn } from "cn";

import type { Action } from "@/bindings/Action";
import type { Message } from "@/bindings/Message";
import { AssistantMark } from "@/components/brand";
import { Actions } from "@/features/chat/actions";
import { LocalityIcon } from "@/components/locality-badge";
import { Markdown } from "@/components/markdown";

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
  const streaming = message.status === "streaming";
  const acting = message.actions.some((a) =>
    ["pending_approval", "approved", "running"].includes(a.status),
  );
  const thinking = streaming && !message.content && !acting;

  return (
    <div className="group flex gap-3.5">
      <AssistantMark />
      <div className="flex min-w-0 flex-1 flex-col gap-2 pt-0.5">
        {thinking && <span className="shimmer self-start text-[14px]">Thinking…</span>}
        {segments(message).map((seg, i, all) =>
          seg.kind === "text" ? (
            <div
              key={i}
              className={cn(streaming && i === all.length - 1 && "streaming-caret")}
            >
              <Markdown>{seg.text}</Markdown>
            </div>
          ) : (
            <Actions key={seg.actions[0].id} actions={seg.actions} />
          ),
        )}
        {message.status === "error" && (
          <div className="flex items-start gap-2.5 rounded-xl border border-[#f1c9c1] bg-[#fdf2ef] px-3.5 py-2.5 text-[13.5px] text-destructive">
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            <span>{message.error ?? "Something went wrong while writing this reply."}</span>
          </div>
        )}
        {!streaming && (
          <div className="flex h-6 items-center gap-2 text-[12.5px] text-faint">
            {/* Only cloud use is worth pointing out: private is the default. */}
            {message.locality === "cloud" && (
              <span className="inline-flex items-center gap-1.5 text-cloud">
                <LocalityIcon locality="cloud" className="size-3.5" />
                Answered by a cloud service
              </span>
            )}
            {message.status === "cancelled" && <span>Stopped</span>}
            {message.status === "interrupted" && <span>This reply was cut off</span>}
            {message.content && <CopyButton text={message.content} />}
          </div>
        )}
      </div>
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
      className="-ml-1 rounded-md p-1 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-subtle hover:text-foreground focus-visible:opacity-100"
    >
      {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
    </button>
  );
}

type Segment = { kind: "text"; text: string } | { kind: "actions"; actions: Action[] };

/** The reply's text with its actions placed where they happened. */
function segments(message: Message): Segment[] {
  const chars = Array.from(message.content);
  const actions = [...message.actions].sort((a, b) => a.content_offset - b.content_offset);
  const out: Segment[] = [];
  let at = 0;
  for (const a of actions) {
    const offset = Math.min(a.content_offset, chars.length);
    const text = chars.slice(at, offset).join("");
    if (text.trim()) out.push({ kind: "text", text });
    at = Math.max(at, offset);
    const last = out.at(-1);
    if (last?.kind === "actions") last.actions.push(a);
    else out.push({ kind: "actions", actions: [a] });
  }
  const rest = chars.slice(at).join("");
  if (rest.trim()) out.push({ kind: "text", text: rest });
  return out;
}
