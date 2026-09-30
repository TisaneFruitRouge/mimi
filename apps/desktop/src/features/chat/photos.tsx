import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { Image as ImageIcon, X } from "lucide-react";
import { cn } from "cn";

import type { Attachment } from "@/bindings/Attachment";
import type { NewAttachment } from "@/bindings/NewAttachment";
import { attachmentUrl } from "@/lib/transport";

/** Most photos with one message (the daemon's limit). */
export const MAX_PHOTOS = 10;
/** Largest photo the daemon takes, before it shrinks it. */
const MAX_PHOTO_BYTES = 20 * 1024 * 1024;

/** A photo picked in the composer, not sent yet. */
export interface DraftPhoto {
  key: string;
  file: File;
}

let nextKey = 0;

export function draftPhoto(file: File): DraftPhoto {
  return { key: `photo-${++nextKey}`, file };
}

/**
 * A small `data:` thumbnail of a picked photo (the page's CSP allows `data:` pictures,
 * not `blob:`); `null` when this browser can't draw it.
 */
function useThumbnail(file: File): string | null | undefined {
  const [thumb, setThumb] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    createImageBitmap(file)
      .then((bitmap) => {
        const scale = 128 / Math.min(bitmap.width, bitmap.height);
        const canvas = document.createElement("canvas");
        canvas.width = Math.max(1, Math.round(bitmap.width * Math.min(scale, 1)));
        canvas.height = Math.max(1, Math.round(bitmap.height * Math.min(scale, 1)));
        canvas.getContext("2d")?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
        bitmap.close();
        if (live) setThumb(canvas.toDataURL("image/png"));
      })
      .catch(() => live && setThumb(null));
    return () => {
      live = false;
    };
  }, [file]);
  return thumb;
}

/** The image files among dropped or pasted files. */
export function imageFiles(files: Iterable<File> | ArrayLike<File>): File[] {
  return Array.from(files).filter((f) => f.type.startsWith("image/") || /\.(heic|heif)$/i.test(f.name));
}

/**
 * A photo ready to send. Big ones are made smaller first when this browser can read them
 * (upload stays small; the assistant shrinks every photo again anyway and removes what
 * the file says about where it was taken).
 */
export async function encodePhoto(photo: DraftPhoto): Promise<NewAttachment> {
  let blob: Blob = photo.file;
  let mime = photo.file.type || null;
  try {
    const bitmap = await createImageBitmap(photo.file);
    const long = Math.max(bitmap.width, bitmap.height);
    if (photo.file.size > 4 * 1024 * 1024 || long > 3000) {
      const scale = Math.min(1, 2400 / long);
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(bitmap.width * scale);
      canvas.height = Math.round(bitmap.height * scale);
      canvas.getContext("2d")?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      const smaller = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.9));
      if (smaller) {
        blob = smaller;
        mime = "image/jpeg";
      }
    }
    bitmap.close();
  } catch {
    // Not something this browser can draw (e.g. HEIC): send it as it is, and let the
    // assistant say whether it can read it.
  }
  if (blob.size > MAX_PHOTO_BYTES) {
    throw new Error(`“${photo.file.name}” is too large. Photos can be up to 20 MB.`);
  }
  return { data: await toBase64(blob), name: photo.file.name || null, mime };
}

function toBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(new Error("A photo couldn't be read. Try adding it again."));
    reader.readAsDataURL(blob);
  });
}

/** Where a sent photo can be shown from, once known. */
function usePhotoUrl(attachment: Attachment): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    attachmentUrl(attachment.id, attachment.mime).then(
      (u) => live && setUrl(u),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [attachment.id, attachment.mime]);
  return url;
}

/** The photos of a sent message: one shown whole, several as a grid of squares. */
export function MessagePhotos({ attachments }: { attachments: Attachment[] }) {
  const [open, setOpen] = useState<Attachment | null>(null);
  const photos = attachments.filter((a) => a.kind === "image");
  if (!photos.length) return null;
  const single = photos.length === 1;
  return (
    <>
      <div
        className={cn(
          "grid gap-1.5",
          single ? "grid-cols-1" : photos.length === 2 || photos.length === 4 ? "grid-cols-2" : "grid-cols-3",
        )}
      >
        {photos.map((p) => (
          <PhotoThumb key={p.id} attachment={p} single={single} onOpen={() => setOpen(p)} />
        ))}
      </div>
      <Lightbox attachment={open} onClose={() => setOpen(null)} />
    </>
  );
}

function PhotoThumb({
  attachment,
  single,
  onOpen,
}: {
  attachment: Attachment;
  single: boolean;
  onOpen: () => void;
}) {
  const url = usePhotoUrl(attachment);
  const { width, height } = attachment;
  // A single photo keeps its shape, within a sensible box.
  const ratio = Math.min(Math.max(width && height ? width / height : 4 / 3, 0.5), 2.6);
  const style = single ? { width: ratio >= 1 ? 280 : Math.round(280 * ratio), aspectRatio: ratio } : undefined;
  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={`Open ${attachment.name}`}
      style={style}
      className={cn(
        "pressable relative overflow-hidden rounded-[14px] bg-subtle shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.06)] focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-lime/40",
        !single && "size-[112px]",
      )}
    >
      {url && (
        <img
          src={url}
          alt={attachment.name}
          draggable={false}
          className="absolute inset-0 size-full object-cover"
        />
      )}
    </button>
  );
}

/** A photo on its own, over everything; any click or Esc closes it. */
function Lightbox({ attachment, onClose }: { attachment: Attachment | null; onClose: () => void }) {
  return (
    <DialogPrimitive.Root open={!!attachment} onOpenChange={(o) => !o && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-[rgb(0_0_0/0.55)] backdrop-blur-[6px] duration-300 data-[state=open]:animate-in data-[state=open]:fade-in-0" />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          onClick={onClose}
          className="fixed inset-0 z-50 flex items-center justify-center p-10 outline-none"
        >
          <DialogPrimitive.Title className="sr-only">{attachment?.name ?? "Photo"}</DialogPrimitive.Title>
          {attachment && <LightboxPhoto attachment={attachment} />}
          <DialogPrimitive.Close
            aria-label="Close"
            className="material-thick pressable absolute top-5 right-5 flex size-9 items-center justify-center rounded-full text-foreground shadow-[var(--shadow-raised)]"
          >
            <X className="size-4" strokeWidth={2.2} />
          </DialogPrimitive.Close>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function LightboxPhoto({ attachment }: { attachment: Attachment }) {
  const url = usePhotoUrl(attachment);
  if (!url) return null;
  return (
    <motion.img
      src={url}
      alt={attachment.name}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ type: "spring", stiffness: 420, damping: 34 }}
      className="max-h-full max-w-full rounded-[14px] object-contain shadow-[var(--shadow-float)]"
    />
  );
}

/** Photos waiting in the composer, each with a way to take it out again. */
export function DraftPhotos({ photos, onRemove }: { photos: DraftPhoto[]; onRemove: (key: string) => void }) {
  return (
    <div className="flex flex-wrap gap-2 px-4 pt-3.5">
      {photos.map((p) => (
        <motion.div
          key={p.key}
          layout
          initial={{ opacity: 0, scale: 0.9 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: "spring", stiffness: 480, damping: 34 }}
          className="group relative size-16 shrink-0"
        >
          <DraftThumb file={p.file} />
          <button
            type="button"
            onClick={() => onRemove(p.key)}
            aria-label={`Remove ${p.file.name || "photo"}`}
            className="pressable absolute -top-1.5 -right-1.5 flex size-5 items-center justify-center rounded-full bg-foreground/80 text-background shadow-[0_1px_3px_rgb(0_0_0/0.2)] backdrop-blur hover:bg-foreground"
          >
            <X className="size-3" strokeWidth={2.6} />
          </button>
        </motion.div>
      ))}
    </div>
  );
}

function DraftThumb({ file }: { file: File }) {
  const thumb = useThumbnail(file);
  const box = "size-full rounded-[12px] bg-subtle shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.06)]";
  if (thumb) return <img src={thumb} alt={file.name} draggable={false} className={cn(box, "object-cover")} />;
  // Still drawing it, or a kind of picture this browser can't show.
  return (
    <div className={cn(box, "flex items-center justify-center text-faint")} title={file.name}>
      {thumb === null && <ImageIcon className="size-5" />}
    </div>
  );
}

/** "Choose one that can in Models", with Models as a way there. */
export function ModelsLink({ onModels }: { onModels?: () => void }) {
  if (!onModels) return <>Models</>;
  return (
    <button type="button" onClick={onModels} className="font-medium text-foreground/80 underline-offset-2 hover:underline">
      Models
    </button>
  );
}
