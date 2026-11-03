import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { FileText, X } from "lucide-react";

import type { NewMailAttachment } from "@/bindings/NewMailAttachment";
import { attachmentUrl } from "@/lib/transport";

/** What the attachments of one email can add up to (the daemon's limit). */
const MAX_TOTAL_BYTES = 20 * 1024 * 1024;

/** "340 KB", "2.4 MB". */
function fileSize(bytes: number) {
  if (bytes < 1e6) return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

/**
 * How big an attached file is: from its base64, or, for one the assistant attached by
 * reference (fetched when it's sent), as the daemon measured it.
 */
function sizeOf(a: NewMailAttachment) {
  if (a.source) return a.source.size ?? 0;
  return Math.floor((a.data.length * 3) / 4);
}

/** Where a file the assistant attached comes from. */
function originOf(a: NewMailAttachment): string | null {
  if (a.source?.kind === "email") return "from an email";
  if (a.source?.kind === "chat") return "from this chat";
  return null;
}

/**
 * Files read for a draft, after the ones it has. Throws, with words for the user, when
 * they'd go over the limit or can't be read.
 */
export async function readAttachments(files: File[], have: NewMailAttachment[]): Promise<NewMailAttachment[]> {
  const total = have.reduce((n, a) => n + sizeOf(a), 0) + files.reduce((n, f) => n + f.size, 0);
  if (total > MAX_TOTAL_BYTES) {
    throw new Error("Attachments can add up to 20 MB in one email.");
  }
  return Promise.all(files.map(read));
}

function read(file: File): Promise<NewMailAttachment> {
  // A picture pasted from the clipboard comes as "image.png".
  const name = file.name && file.name !== "image.png" ? file.name : "Pasted image.png";
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () =>
      resolve({
        name,
        mime: file.type || null,
        data: String(reader.result).replace(/^data:[^,]*,/, ""),
        source: null,
      });
    reader.onerror = () => reject(new Error(`“${name}” couldn't be read. Try attaching it again.`));
    reader.readAsDataURL(file);
  });
}

/** Pictures this page can show from a `data:` URL (the CSP allows those, not `blob:`). */
const SHOWN = /^image\/(png|jpeg|gif|webp)$/;

/** A picture's thumbnail: its own data, or a photo from the chat fetched from the daemon. */
function useThumbnail(a: NewMailAttachment): string | null {
  const chatPhoto = a.source?.kind === "chat" ? a.source.attachment : null;
  const [fetched, setFetched] = useState<string | null>(null);
  useEffect(() => {
    if (!chatPhoto) return;
    let live = true;
    attachmentUrl(chatPhoto, a.mime ?? "image/jpeg").then(
      (u) => live && setFetched(u),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [chatPhoto, a.mime]);
  if (chatPhoto) return fetched;
  return !a.source && a.mime && SHOWN.test(a.mime) ? `data:${a.mime};base64,${a.data}` : null;
}

/** The files attached to a draft, each with a way to take it out again. */
export function DraftAttachments({
  attachments,
  onRemove,
}: {
  attachments: NewMailAttachment[];
  onRemove: (index: number) => void;
}) {
  if (attachments.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-2 px-4 pb-3">
      {attachments.map((a, i) => (
        <AttachedFile key={`${i}-${a.name}`} attachment={a} onRemove={() => onRemove(i)} />
      ))}
    </div>
  );
}

function AttachedFile({ attachment: a, onRemove }: { attachment: NewMailAttachment; onRemove: () => void }) {
  const thumbnail = useThumbnail(a);
  const size = sizeOf(a);
  const origin = originOf(a);
  return (
    <motion.span
      layout
      initial={{ opacity: 0, scale: 0.94 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ type: "spring", stiffness: 480, damping: 34 }}
      className="inline-flex h-11 max-w-[260px] items-center gap-2 rounded-[10px] bg-fill pr-1 pl-1.5"
    >
      {thumbnail ? (
        <img src={thumbnail} alt="" draggable={false} className="size-8 shrink-0 rounded-[6px] object-cover" />
      ) : (
        <span className="flex size-8 shrink-0 items-center justify-center rounded-[6px] bg-background text-muted-foreground">
          <FileText className="size-4" />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate type-subhead">{a.name}</span>
        <span className="block truncate type-footnote text-faint">
          {[size > 0 ? fileSize(size) : null, origin].filter(Boolean).join(" · ")}
        </span>
      </span>
      <button
        type="button"
        aria-label={`Remove ${a.name}`}
        onClick={onRemove}
        className="flex size-5 shrink-0 items-center justify-center rounded-full text-faint hover:bg-[rgb(118_118_128/0.18)] hover:text-foreground"
      >
        <X className="size-3" />
      </button>
    </motion.span>
  );
}
