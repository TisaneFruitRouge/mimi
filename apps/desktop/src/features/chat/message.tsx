import { useState } from "react";
import { Check, CircleAlert, Copy, ExternalLink, Link, Mic, Volume2 } from "lucide-react";
import { cn } from "cn";

import type { Action } from "@/bindings/Action";
import type { Message } from "@/bindings/Message";
import { Actions } from "@/features/chat/actions";
import { MentionText } from "@/features/chat/mentions/mention-text";
import { MessagePhotos, ModelsLink } from "@/features/chat/photos";
import { ReadAloudButton } from "@/features/chat/read-aloud";
import { AssistantAvatar } from "@/components/assistant-avatar";
import { LocalityIcon } from "@/components/locality-badge";
import { Markdown } from "@/components/markdown";
import { copyText, selectedText } from "@/components/app-context-menu";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { toggleSpeaking } from "@/lib/speech";
import { openExternal } from "@/lib/transport";

export function MessageView({ message, onModels }: { message: Message; onModels?: () => void }) {
  if (message.role === "user") {
    const photos = message.attachments ?? [];
    return (
      <MessageMenu text={message.content}>
        <div className="flex flex-col items-end gap-1.5 pl-16">
          {photos.length > 0 && <MessagePhotos attachments={photos} />}
          {message.content && (
            <div className="flex items-end gap-2">
              {message.spoken && (
                <Mic className="mb-3 size-3.5 shrink-0 text-faint" aria-label="Said out loud" />
              )}
              <div className="rounded-[20px] rounded-br-[6px] bg-[#e9e9ee] px-4 py-2.5 type-body whitespace-pre-wrap">
                <MentionText text={message.content} mentions={message.mentions} />
              </div>
            </div>
          )}
          {message.attachments_unseen && photos.length > 0 && (
            <p className="max-w-[460px] text-right type-footnote text-faint">
              This model couldn't see {photos.length === 1 ? "this photo" : "these photos"}. Choose one
              that can in <ModelsLink onModels={onModels} />.
            </p>
          )}
        </div>
      </MessageMenu>
    );
  }
  return (
    <MessageMenu text={message.content} readAloud={message.status !== "streaming" ? message.id : undefined}>
      <div>
        <AssistantMessage message={message} />
      </div>
    </MessageMenu>
  );
}

/** Right-click on a message: copy it or the selected part, open or copy a link, and for
 * replies, read it aloud. */
function MessageMenu({
  text,
  readAloud,
  children,
}: {
  text: string;
  /** The reply's id, to read it aloud. */
  readAloud?: string;
  children: React.ReactElement;
}) {
  const [link, setLink] = useState<string | null>(null);
  const [selection, setSelection] = useState("");
  return (
    <ContextMenu>
      <ContextMenuTrigger
        asChild
        onContextMenu={(e) => {
          const a = (e.target as Element).closest?.("a[href]") as HTMLAnchorElement | null;
          setLink(a?.href ?? null);
          setSelection(selectedText());
        }}
      >
        {children}
      </ContextMenuTrigger>
      <ContextMenuContent className="w-[210px]">
        {link && (
          <>
            <ContextMenuItem onSelect={() => openExternal(link)}>
              <ExternalLink /> Open link
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => copyText(link)}>
              <Link /> Copy link address
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        {selection && (
          <ContextMenuItem onSelect={() => copyText(selection)}>
            <Copy /> Copy
          </ContextMenuItem>
        )}
        <ContextMenuItem disabled={!text} onSelect={() => copyText(text)}>
          <Copy /> Copy message
        </ContextMenuItem>
        {readAloud && text && (
          <ContextMenuItem onSelect={() => toggleSpeaking(readAloud, text)}>
            <Volume2 /> Read aloud
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

function AssistantMessage({ message }: { message: Message }) {
  const streaming = message.status === "streaming";
  const acting = message.actions.some((a) =>
    ["pending_approval", "approved", "running"].includes(a.status),
  );
  const thinking = streaming && !message.content && !acting;

  return (
    <div className="group flex flex-col gap-2.5">
      {thinking && <Thinking />}
      {segments(message).map((seg, i, all) =>
        seg.kind === "text" ? (
          <div key={i} className={cn(streaming && i === all.length - 1 && "streaming-caret")}>
            <Markdown>{seg.text}</Markdown>
          </div>
        ) : (
          <Actions key={seg.actions[0].id} actions={seg.actions} />
        ),
      )}
      {message.status === "error" && (
        <div className="flex items-start gap-2.5 rounded-[14px] bg-[#fdeeec] px-4 py-3 type-callout text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <span>{message.error ?? "Something went wrong while writing this reply."}</span>
        </div>
      )}
      {!streaming && (
        <div className="-mt-1 flex h-7 items-center gap-2 type-footnote text-faint">
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
          {message.content && <ReadAloudButton id={message.id} text={message.content} className="-ml-2.5" />}
        </div>
      )}
    </div>
  );
}

/** Shown before the first words arrive: the assistant, small, mulling it over. It's a
 * status, not an avatar on the message: it goes away as soon as the reply starts. */
function Thinking() {
  return (
    <div className="flex h-7 items-center gap-2" aria-label="Thinking">
      <AssistantAvatar size={26} mood="thinking" decorative className="-ml-0.5" />
      <span className="shimmer type-callout">Thinking</span>
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
      className="-ml-1.5 flex size-7 items-center justify-center rounded-full opacity-0 transition-opacity group-hover:opacity-100 hover:bg-fill hover:text-foreground focus-visible:opacity-100"
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
